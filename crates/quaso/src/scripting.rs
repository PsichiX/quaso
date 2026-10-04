use crate::{context::GameContext, gc::DynGc};
use ankha::{
    host::AnkhaCallError,
    library::AnkhaVmScope,
    script::{AnkhaExpression, AnkhaFile, AnkhaPackage},
};
pub use ankha_auri::{
    host::{AuriCall, AuriLendable, AuriResults, AuriValue, IntoAuriValue},
    transformer::{AuriValueTransformer, ValueTransformer},
};
use intuicio_backend_vm::debugger::VmDebuggerHandle;
use intuicio_core::{
    Filter,
    context::Context,
    function::{FunctionHandle, FunctionQuery},
    registry::Registry,
    types::{TypeHandle, TypeQuery},
};
use intuicio_data::managed::gc::DynamicManagedGc;
use std::{collections::HashMap, ptr::NonNull};

const HOST_ACCESS: &str = "quaso";
pub const HOST_MODULE: &str = "host";
const CREATE_METHOD: &str = "create";
const DESTROY_METHOD: &str = "destroy";

pub type ScriptError = AnkhaCallError;
pub type ScriptDebugger = VmDebuggerHandle<AnkhaExpression>;

#[derive(Default)]
struct HostAccess {
    game: Option<NonNull<GameContext<'static>>>,
}

unsafe impl Send for HostAccess {}
unsafe impl Sync for HostAccess {}

pub struct Scripting {
    registry: Registry,
    context: Context,
    files: HashMap<String, AnkhaFile>,
    #[allow(clippy::type_complexity)]
    setups: Vec<Box<dyn Fn(&mut Registry)>>,
    debugger: Option<ScriptDebugger>,
}

impl Default for Scripting {
    fn default() -> Self {
        Self::new(10240, 10240)
    }
}

impl Scripting {
    pub fn new(stack_capacity: usize, registers_capacity: usize) -> Self {
        let mut context = Context::new(stack_capacity, registers_capacity);
        context.set_custom(HOST_ACCESS, HostAccess::default());
        Self {
            registry: Self::base_registry(),
            context,
            files: Default::default(),
            setups: Default::default(),
            debugger: Default::default(),
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    pub fn debugger(&self) -> Option<&ScriptDebugger> {
        self.debugger.as_ref()
    }

    // The debugger is written into each function body when the package is
    // installed, so a change here takes effect on the next `reinstall`.
    pub fn set_debugger(&mut self, debugger: Option<ScriptDebugger>) {
        self.debugger = debugger;
    }

    pub fn add_setup(&mut self, setup: impl Fn(&mut Registry) + 'static) {
        setup(&mut self.registry);
        self.setups.push(Box::new(setup));
    }

    pub fn add_file(&mut self, name: impl ToString, file: AnkhaFile) {
        self.files.insert(name.to_string(), file);
    }

    pub fn remove_file(&mut self, name: &str) -> Option<AnkhaFile> {
        self.files.remove(name)
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &AnkhaFile)> {
        self.files.iter().map(|(name, file)| (name.as_str(), file))
    }

    pub fn reinstall(&mut self) {
        let mut registry = Self::base_registry();
        for setup in &self.setups {
            setup(&mut registry);
        }
        let mut package = AnkhaPackage::default();
        package.files.extend(
            self.files
                .iter()
                .map(|(name, file)| (name.to_owned(), file.to_owned())),
        );
        package.install::<AnkhaVmScope>(&mut registry, self.debugger.clone());
        self.registry = registry;
    }

    fn base_registry() -> Registry {
        let mut registry = Registry::default().with_basic_types();
        ankha::library::install(&mut registry);
        ankha_auri::library::install(&mut registry);
        registry
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

    pub fn call<'a, 'env>(&'a mut self, function: &FunctionHandle) -> ScriptCall<'a, 'env> {
        ScriptCall {
            scripting: self,
            inner: AuriCall::new(function.clone()),
        }
    }

    pub fn call_function<'a, 'env>(
        &'a mut self,
        module: &str,
        name: &str,
    ) -> Result<ScriptCall<'a, 'env>, ScriptError> {
        let inner = AuriCall::find(&self.registry, module, name)?;
        Ok(ScriptCall {
            scripting: self,
            inner,
        })
    }

    fn set_game(&mut self, pointer: Option<NonNull<GameContext<'static>>>) {
        if let Some(access) = self.context.custom_mut::<HostAccess>(HOST_ACCESS) {
            access.game = pointer;
        }
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

    pub fn describe(value: &DynGc) -> String {
        macro_rules! describe_as {
        ($($type:ty),+ $(,)?) => {
            $(
                if value.is::<$type>() {
                    return value.0.read::<true, $type>().to_string();
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

    pub fn pop_gc(context: &mut Context) -> Option<DynGc> {
        context.stack().pop::<DynamicManagedGc>().map(DynGc::from)
    }

    pub fn push_gc(context: &mut Context, value: DynGc) -> bool {
        context.stack().push(value.0)
    }
}

pub struct ScriptCall<'a, 'env> {
    scripting: &'a mut Scripting,
    inner: AuriCall<'env>,
}

impl<'a, 'env> ScriptCall<'a, 'env> {
    pub fn value(mut self, value: impl IntoAuriValue) -> Self {
        self.inner = self.inner.value(value);
        self
    }

    pub fn gc(mut self, value: DynGc) -> Self {
        self.inner = self.inner.gc(value.0);
        self
    }

    /// # Safety
    ///
    /// A script can move ownership out of the handle and keep the handle alive
    /// past `value`. Pass a value only to a script that does not do this.
    pub unsafe fn gc_borrowed<T>(self, value: &'env mut T) -> Self {
        self.gc(unsafe { DynGc::borrowed(value) })
    }

    pub fn lent(mut self, value: impl IntoAuriValue) -> Self {
        self.inner = self.inner.lent(value);
        self
    }

    pub fn lent_mut<T: AuriLendable>(mut self, value: &'env mut T) -> Self {
        self.inner = self.inner.lent_mut(value);
        self
    }

    pub fn into_inner(self) -> AuriCall<'env> {
        self.inner
    }

    pub fn run(self, game: &mut GameContext) -> Result<AuriResults<'env>, ScriptError> {
        let Self { scripting, inner } = self;
        let pointer = NonNull::new((game as *mut GameContext).cast::<GameContext<'static>>());
        scripting.set_game(pointer);
        let results = inner.call(&mut scripting.context, &scripting.registry);
        scripting.set_game(None);
        results
    }
}

pub struct ScriptObject {
    instance: DynGc,
    module: String,
    type_name: String,
    methods: HashMap<String, Option<FunctionHandle>>,
    destroyed: bool,
}

impl ScriptObject {
    pub fn create<'env>(
        scripting: &mut Scripting,
        game: &mut GameContext,
        module: &str,
        type_name: &str,
        arguments: impl for<'a> FnOnce(ScriptCall<'a, 'env>) -> ScriptCall<'a, 'env>,
    ) -> Result<Self, ScriptError> {
        let function = scripting
            .find_method(module, type_name, CREATE_METHOD)
            .ok_or_else(|| ScriptError::FunctionNotFound {
                module: Some(module.to_owned()),
                name: format!("{type_name}::{CREATE_METHOD}"),
            })?;
        let mut results = arguments(scripting.call(&function)).run(game)?;
        let instance =
            results
                .take()
                .map(DynGc::from)
                .ok_or_else(|| ScriptError::MissingOutput {
                    function: format!("{module}::{type_name}::{CREATE_METHOD}"),
                    index: 0,
                    parameter: "result".to_owned(),
                })?;
        Ok(Self {
            instance,
            module: module.to_owned(),
            type_name: type_name.to_owned(),
            methods: Default::default(),
            destroyed: false,
        })
    }

    pub fn instance(&self) -> &DynGc {
        &self.instance
    }

    pub fn call<'a, 'env>(
        &mut self,
        scripting: &'a mut Scripting,
        method: &str,
    ) -> Option<ScriptCall<'a, 'env>> {
        let function = self.method(&*scripting, method)?;
        Some(scripting.call(&function).gc(self.instance.reference()))
    }

    pub fn destroy(
        &mut self,
        scripting: &mut Scripting,
        game: &mut GameContext,
    ) -> Result<(), ScriptError> {
        if self.destroyed {
            return Ok(());
        }
        self.destroyed = true;
        if let Some(call) = self.call(scripting, DESTROY_METHOD) {
            call.run(game)?;
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn has(&mut self, scripting: &Scripting, method: &str) -> bool {
        self.method(scripting, method).is_some()
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
        scripting.add_file("test.auri", file);
        scripting.reinstall();
        assert!(scripting.find("test", "plain").is_some());
        assert!(scripting.find_method("test", "Thing", "create").is_some());
        assert!(scripting.find_method("test", "Thing", "get").is_some());
    }
}
