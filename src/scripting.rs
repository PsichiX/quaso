use crate::{context::GameContext, game::GameState};
use ankha::{
    library::AnkhaVmScope,
    script::{AnkhaFile, AnkhaPackage},
};
use intuicio_core::{
    Filter,
    context::Context,
    function::{
        Function, FunctionBody, FunctionHandle, FunctionParameter, FunctionQuery, FunctionSignature,
    },
    registry::Registry,
    types::{TypeHandle, TypeQuery},
};
use intuicio_data::managed::gc::DynamicManagedGc;
use std::{collections::HashMap, ptr::NonNull};

const HOST_ACCESS: &str = "quaso";

pub const HOST_MODULE: &str = "host";
pub const CREATE_METHOD: &str = "create";
pub const DESTROY_METHOD: &str = "destroy";

#[derive(Default)]
struct HostAccess {
    game: Option<NonNull<GameContext<'static>>>,
}

unsafe impl Send for HostAccess {}
unsafe impl Sync for HostAccess {}

pub struct Scripting {
    registry: Registry,
    context: Context,
}

impl Default for Scripting {
    fn default() -> Self {
        Self::new(10240, 10240)
    }
}

impl Scripting {
    pub fn new(stack_capacity: usize, registers_capacity: usize) -> Self {
        let mut registry = Registry::default().with_basic_types();
        ankha::library::install(&mut registry);
        ankha_auri::library::install(&mut registry);
        let mut context = Context::new(stack_capacity, registers_capacity);
        context.set_custom(HOST_ACCESS, HostAccess::default());
        Self { registry, context }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    pub fn install(&mut self, files: impl IntoIterator<Item = (String, AnkhaFile)>) {
        let mut package = AnkhaPackage::default();
        package.files.extend(files);
        package.install::<AnkhaVmScope>(&mut self.registry, None);
    }

    pub fn find(&self, module: &str, function: &str) -> Option<FunctionHandle> {
        self.registry.find_function(FunctionQuery {
            name: Some(function.into()),
            module_name: Filter::Matching(module.into()),
            ..Default::default()
        })
    }

    pub fn find_method(
        &self,
        module: &str,
        type_name: &str,
        method: &str,
    ) -> Option<FunctionHandle> {
        self.registry.find_function(FunctionQuery {
            name: Some(method.into()),
            module_name: Filter::Matching(module.into()),
            type_query: Filter::Matching(TypeQuery {
                name: Some(type_name.into()),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    pub fn call(
        &mut self,
        game: &mut GameContext,
        function: &FunctionHandle,
        arguments: impl IntoIterator<Item = DynamicManagedGc>,
    ) -> Option<DynamicManagedGc> {
        let mut arguments = arguments.into_iter().collect::<Vec<_>>();
        while let Some(argument) = arguments.pop() {
            self.context.stack().push(argument);
        }
        let pointer = NonNull::new((game as *mut GameContext).cast::<GameContext<'static>>());
        self.set_game(pointer);
        function.invoke(&mut self.context, &self.registry);
        self.set_game(None);
        self.context.stack().pop::<DynamicManagedGc>()
    }

    fn set_game(&mut self, pointer: Option<NonNull<GameContext<'static>>>) {
        if let Some(access) = self.context.custom_mut::<HostAccess>(HOST_ACCESS) {
            access.game = pointer;
        }
    }

    pub fn add_host_function(
        registry: &mut Registry,
        name: &str,
        inputs: usize,
        body: impl Fn(&mut Context, &Registry) -> DynamicManagedGc + Send + Sync + 'static,
    ) {
        let gc = Self::gc_type(registry);
        let mut signature = FunctionSignature::new(name)
            .with_module_name(HOST_MODULE)
            .with_output(FunctionParameter::new("result", gc.clone()));
        for index in 0..inputs {
            signature = signature.with_input(FunctionParameter::new(
                format!("argument{index}"),
                gc.clone(),
            ));
        }
        registry.add_function(Function::new(
            signature,
            FunctionBody::closure(move |context: &mut Context, registry: &Registry| {
                let result = body(context, registry);
                context.stack().push(result);
            }),
        ));
    }

    pub fn with_game<R>(
        context: &mut Context,
        name: &str,
        f: impl FnOnce(&mut GameContext) -> R,
    ) -> R {
        let game = context
            .custom::<HostAccess>(HOST_ACCESS)
            .and_then(|access| access.game)
            .unwrap_or_else(|| panic!("`{name}` was called outside of a script call!"));
        // The pointer came from a borrow that outlives this call, and nothing else
        // holds a borrow of the game while a script runs.
        f(unsafe { game.as_ptr().as_mut().unwrap() })
    }

    pub fn pop_argument(context: &mut Context, name: &str) -> DynamicManagedGc {
        context
            .stack()
            .pop::<DynamicManagedGc>()
            .unwrap_or_else(|| panic!("`{name}` got a stack value that is not gc managed!"))
    }

    pub fn number(value: &DynamicManagedGc, name: &str) -> f64 {
        macro_rules! read_as {
        ($($type:ty),+ $(,)?) => {
            $(
                if value.is::<$type>() {
                    return *value.read::<true, $type>() as f64;
                }
            )+
        };
    }
        read_as!(f64, f32, i64, i32, i16, i8, isize, u64, u32, u16, u8, usize);
        panic!("`{name}` got a value that is not a number!")
    }

    pub fn text_of(value: &DynamicManagedGc, name: &str) -> String {
        if value.is::<String>() {
            return value.read::<true, String>().to_owned();
        }
        panic!("`{name}` got a value that is not a string!")
    }

    pub fn describe(value: &DynamicManagedGc) -> String {
        macro_rules! describe_as {
        ($($type:ty),+ $(,)?) => {
            $(
                if value.is::<$type>() {
                    return value.read::<true, $type>().to_string();
                }
            )+
        };
    }
        if !value.exists() {
            return "<dead>".to_owned();
        }
        if value.is::<()>() {
            return "()".to_owned();
        }
        describe_as!(
            String, bool, char, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize,
            f32, f64,
        );
        "<value>".to_owned()
    }

    pub fn gc_type(registry: &Registry) -> TypeHandle {
        registry
            .find_type(TypeQuery::of::<DynamicManagedGc>())
            .expect("Could not find `DynamicManagedGc` type, install the ankha library first!")
    }
}

pub struct ScriptObject {
    instance: DynamicManagedGc,
    module: String,
    type_name: String,
    methods: HashMap<String, Option<FunctionHandle>>,
    destroyed: bool,
}

impl ScriptObject {
    pub fn create(
        scripting: &mut Scripting,
        game: &mut GameContext,
        module: &str,
        type_name: &str,
        arguments: impl IntoIterator<Item = DynamicManagedGc>,
    ) -> Option<Self> {
        let function = scripting.find_method(module, type_name, CREATE_METHOD)?;
        let instance = scripting.call(game, &function, arguments)?;
        Some(Self {
            instance,
            module: module.to_owned(),
            type_name: type_name.to_owned(),
            methods: Default::default(),
            destroyed: false,
        })
    }

    pub fn instance(&self) -> &DynamicManagedGc {
        &self.instance
    }

    pub fn destroy(
        &mut self,
        scripting: &mut Scripting,
        game: &mut GameContext,
    ) -> Option<DynamicManagedGc> {
        if self.destroyed {
            return None;
        }
        self.destroyed = true;
        self.call(scripting, game, DESTROY_METHOD, [])
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn has(&mut self, scripting: &Scripting, method: &str) -> bool {
        self.method(scripting, method).is_some()
    }

    pub fn call(
        &mut self,
        scripting: &mut Scripting,
        game: &mut GameContext,
        method: &str,
        arguments: impl IntoIterator<Item = DynamicManagedGc>,
    ) -> Option<DynamicManagedGc> {
        let function = self.method(scripting, method)?;
        let mut values = vec![self.instance.reference()];
        values.extend(arguments);
        scripting.call(game, &function, values)
    }

    // A miss is cached too, so a method the script does not declare costs one
    // registry walk and not one walk per frame.
    fn method(&mut self, scripting: &Scripting, method: &str) -> Option<FunctionHandle> {
        if let Some(function) = self.methods.get(method) {
            return function.clone();
        }
        let function = scripting.find_method(&self.module, &self.type_name, method);
        self.methods.insert(method.to_owned(), function.clone());
        function
    }
}

pub struct ScriptedGameState {
    scripting: Scripting,
    state: Option<DynamicManagedGc>,
    enter: Option<FunctionHandle>,
    exit: Option<FunctionHandle>,
    update: Option<FunctionHandle>,
    draw: Option<FunctionHandle>,
}

impl ScriptedGameState {
    pub fn new(scripting: Scripting, module: &str) -> Self {
        Self {
            enter: scripting.find(module, "enter"),
            exit: scripting.find(module, "exit"),
            update: scripting.find(module, "update"),
            draw: scripting.find(module, "draw"),
            scripting,
            state: None,
        }
    }

    pub fn scripting(&self) -> &Scripting {
        &self.scripting
    }

    pub fn scripting_mut(&mut self) -> &mut Scripting {
        &mut self.scripting
    }

    fn state(&self) -> DynamicManagedGc {
        match self.state.as_ref() {
            Some(state) => state.reference(),
            None => DynamicManagedGc::new(()),
        }
    }
}

impl GameState for ScriptedGameState {
    fn enter(&mut self, mut context: GameContext) {
        if let Some(function) = self.enter.clone() {
            self.state = self.scripting.call(&mut context, &function, []);
        }
    }

    fn exit(&mut self, mut context: GameContext) {
        if let Some(function) = self.exit.clone() {
            let state = self.state();
            self.scripting.call(&mut context, &function, [state]);
        }
    }

    fn fixed_update(&mut self, mut context: GameContext, delta_time: f32) {
        if let Some(function) = self.update.clone() {
            let state = self.state();
            self.scripting.call(
                &mut context,
                &function,
                [state, DynamicManagedGc::new(delta_time as f64)],
            );
        }
    }

    fn draw(&mut self, mut context: GameContext) {
        if let Some(function) = self.draw.clone() {
            let state = self.state();
            self.scripting.call(&mut context, &function, [state]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ankha::parser::AnkhaContentParser;

    // No host functions, because ankha resolves every call when the script is
    // installed. A script calling one that is not registered yet panics there.
    const SOURCE: &str = r#"
        mod test {
            struct Thing {
                value
            }

            fn Thing::create() {
                Thing { value: 1i }
            }

            fn Thing::get(self) {
                self.value
            }

            fn plain(x) {
                x
            }
        }
    "#;

    #[test]
    fn test_script_installs() {
        let file = AnkhaContentParser::default()
            .with_setup(ankha_auri::install)
            .parse_file_content(SOURCE)
            .unwrap();
        let mut scripting = Scripting::default();
        scripting.install([("test.auri".to_owned(), file)]);
        assert!(scripting.find("test", "plain").is_some());
        assert!(scripting.find_method("test", "Thing", "create").is_some());
        assert!(scripting.find_method("test", "Thing", "get").is_some());
    }
}
