use intuicio_backend_vm::scope::VmScope;
use intuicio_core::{context::Context, host::Host, registry::Registry};
use intuicio_frontend_simpleton::{
    Reference, Type, library,
    script::{SimpletonModule, SimpletonPackage, SimpletonScriptExpression},
};
use serde::{Deserialize, Serialize};

#[macro_export]
macro_rules! script_contents {
    ( $const_name:ident => $($path:literal),* ) => {
        const $const_name: &[&str] = &[ $( include_str!($path) ),* ];
    };
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptingConfig {
    pub stack_capacity: usize,
    pub registers_capacity: usize,
}

impl Default for ScriptingConfig {
    fn default() -> Self {
        Self {
            stack_capacity: 10240,
            registers_capacity: 10240,
        }
    }
}

pub fn create_host(
    config: ScriptingConfig,
    script_contents: &[&str],
    registry_setup: impl IntoIterator<Item = fn(&mut Registry)>,
) {
    let package = SimpletonPackage {
        modules: script_contents
            .iter()
            .enumerate()
            .map(|(index, content)| (index.to_string(), SimpletonModule::parse(content).unwrap()))
            .collect(),
    };
    let mut registry = Registry::default();
    library::install(&mut registry);
    for setup in registry_setup.into_iter() {
        (setup)(&mut registry);
    }
    package
        .compile()
        .install::<VmScope<SimpletonScriptExpression>>(&mut registry, None);
    let context = Context::new(config.stack_capacity, config.registers_capacity);
    let host = Host::new(context, registry.into());
    if host.push_global().is_err() {
        panic!("Could not make global scripting host!");
    }
}

pub fn destroy_host() {
    Host::pop_global();
}

pub fn call_function(name: &str, module_name: &str, args: &[Reference]) -> Reference {
    Host::with_global(|host| {
        let Some(handle) = host.find_function(name, module_name, None) else {
            return Reference::null();
        };
        let (context, registry) = host.context_and_registry();
        for arg in args.iter().rev() {
            context.stack().push(arg.clone());
        }
        handle.invoke(context, registry);
        context.stack().pop().unwrap_or_default()
    })
    .unwrap_or_default()
}

pub fn call_object(object: Reference, name: &str, args: &[Reference]) -> Reference {
    Host::with_global(move |host| {
        let Some(ty) = object.type_of() else {
            return Reference::null();
        };
        let Some(ty) = ty.handle() else {
            return Reference::null();
        };
        let Some(handle) = host.find_function(name, ty.module_name().unwrap_or_default(), None)
        else {
            return Reference::null();
        };
        let (context, registry) = host.context_and_registry();
        for arg in args.iter().rev() {
            context.stack().push(arg.clone());
        }
        context.stack().push(object);
        handle.invoke(context, registry);
        context.stack().pop().unwrap_or_default()
    })
    .unwrap_or_default()
}

pub fn new(type_name: &str, module_name: &str) -> Reference {
    new_init(type_name, module_name, &[])
}

pub fn new_init(type_name: &str, module_name: &str, properties: &[(&str, Reference)]) -> Reference {
    Host::with_global(move |host| {
        let ty = Reference::new_type(
            Type::by_name(type_name, module_name, host.registry()).unwrap(),
            host.registry(),
        );
        let properties = Reference::new_map(
            properties
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
            host.registry(),
        );
        library::reflect::new(ty, properties)
    })
    .unwrap_or_default()
}

pub fn new_typed<T: 'static>(value: T) -> Reference {
    Host::with_global(move |host| Reference::new(value, host.registry())).unwrap_or_default()
}

pub fn get(object: Reference, field: &str) -> Reference {
    object
        .read_object()
        .unwrap()
        .read_field::<Reference>(field)
        .unwrap()
        .clone()
}

pub fn set(mut object: Reference, field: &str, value: Reference) {
    *object
        .write_object()
        .unwrap()
        .write_field::<Reference>(field)
        .unwrap() = value;
}
