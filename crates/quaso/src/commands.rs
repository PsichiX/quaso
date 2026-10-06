use crate::{capture, context::GameContext, input, input::InputBudget, ui};
use flume::{Receiver, Sender, TryRecvError, unbounded};
use schemars::{JsonSchema, generate::SchemaSettings};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{collections::HashMap, time::Duration};
use typid::ID;

pub type GameCommandId = ID<GameCommandRequest>;
pub type GameCommandHandler = fn(&mut GameContext, Value) -> Result<Value, String>;

pub struct GameCommandRequest {
    id: GameCommandId,
    name: String,
    arguments: Value,
    response: Option<Sender<GameCommandResponse>>,
}

impl GameCommandRequest {
    pub fn id(&self) -> GameCommandId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &Value {
        &self.arguments
    }
}

#[derive(Debug, Clone)]
pub struct GameCommandResponse {
    pub id: GameCommandId,
    pub name: String,
    pub result: Result<Value, String>,
}

pub struct GameCommandPending {
    id: GameCommandId,
    receiver: Receiver<GameCommandResponse>,
}

impl GameCommandPending {
    pub fn id(&self) -> GameCommandId {
        self.id
    }

    pub fn try_take(&self) -> Option<GameCommandResponse> {
        self.receiver.try_recv().ok()
    }

    pub fn take_blocking(self) -> Result<GameCommandResponse, String> {
        self.receiver
            .recv()
            .map_err(|_| "Game stopped before it answered the command".to_owned())
    }

    pub fn take_timeout(self, timeout: Duration) -> Result<GameCommandResponse, String> {
        self.receiver
            .recv_timeout(timeout)
            .map_err(|_| "Game did not answer the command in time".to_owned())
    }
}

#[derive(Clone)]
pub struct GameCommandsHandle {
    sender: Sender<GameCommandRequest>,
}

impl GameCommandsHandle {
    pub fn call(
        &self,
        name: impl ToString,
        arguments: Value,
    ) -> Result<GameCommandPending, String> {
        let (response, receiver) = unbounded();
        let id = GameCommandId::new();
        self.sender
            .send(GameCommandRequest {
                id,
                name: name.to_string(),
                arguments,
                response: Some(response),
            })
            .map_err(|_| "Game stopped before it took the command".to_owned())?;
        Ok(GameCommandPending { id, receiver })
    }

    pub fn notify(&self, name: impl ToString, arguments: Value) -> Result<GameCommandId, String> {
        let id = GameCommandId::new();
        self.sender
            .send(GameCommandRequest {
                id,
                name: name.to_string(),
                arguments,
                response: None,
            })
            .map_err(|_| "Game stopped before it took the command".to_owned())?;
        Ok(id)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

// A caller with nothing to say sends `null`, and serde reads `null` as a
// missing struct, not as an empty one. Without this every handler would have to
// accept two spellings of "no arguments".
pub fn normalize_arguments(arguments: Value) -> Value {
    match arguments {
        Value::Null => Value::Object(Map::new()),
        arguments => arguments,
    }
}

fn schema_settings() -> SchemaSettings {
    SchemaSettings::draft2020_12().with(|settings| {
        settings.meta_schema = None;
        settings.inline_subschemas = true;
    })
}

// The root title holds the Rust type name of the argument struct. The name says
// nothing to a caller, and it leaks an internal name into the tool list.
fn strip_root_title(mut schema: Value) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.remove("title");
    }
    schema
}

pub fn input_schema_for<Args: JsonSchema>() -> Value {
    strip_root_title(
        schema_settings()
            .for_deserialize()
            .into_generator()
            .into_root_schema_for::<Args>()
            .to_value(),
    )
}

pub fn output_schema_for<Out: JsonSchema>() -> Option<Value> {
    let schema = strip_root_title(
        schema_settings()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<Out>()
            .to_value(),
    );
    // A handler that returns a bare `Value` produces the "anything goes"
    // schema. MCP reads an output schema as a promise about the answer, so a
    // schema that promises nothing must be left out.
    match schema.get("type").and_then(|value| value.as_str()) {
        Some("object") => Some(schema),
        _ => None,
    }
}

pub fn input_schema_of<Args: JsonSchema, Out>(
    _handler: fn(&mut GameContext, Args) -> Result<Out, String>,
) -> Value {
    input_schema_for::<Args>()
}

pub fn output_schema_of<Args, Out: JsonSchema>(
    _handler: fn(&mut GameContext, Args) -> Result<Out, String>,
) -> Option<Value> {
    output_schema_for::<Out>()
}

// The handler must be a path to a plain `fn` item. The expansion names the
// handler twice: once inside the erasing `fn` item, which can capture nothing,
// and once to read the argument type and the result type.
#[macro_export]
macro_rules! game_command {
    ($name:expr => $handler:path) => {{
        fn erased(
            context: &mut $crate::context::GameContext,
            arguments: $crate::third_party::serde_json::Value,
        ) -> Result<$crate::third_party::serde_json::Value, String> {
            let arguments = $crate::commands::normalize_arguments(arguments);
            let arguments = $crate::third_party::serde_json::from_value(arguments)
                .map_err(|error| format!("Could not read the arguments: {error}"))?;
            let result = $handler(context, arguments)?;
            $crate::third_party::serde_json::to_value(result)
                .map_err(|error| format!("Could not write the result: {error}"))
        }
        $crate::commands::GameCommand::new(
            $name,
            erased,
            $crate::commands::input_schema_of($handler),
            $crate::commands::output_schema_of($handler),
        )
    }};
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GameCommandAnnotations {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
}

impl GameCommandAnnotations {
    pub fn to_json(self) -> Value {
        json!({
            "readOnlyHint": self.read_only,
            "destructiveHint": self.destructive,
            "idempotentHint": self.idempotent,
            "openWorldHint": self.open_world,
        })
    }
}

// `List` needs the whole registry and `Capture` answers a frame later, so
// neither one fits the handler signature. Naming both here keeps the dispatcher
// exhaustive, and keeps both in the registry, where the schema of each lives.
#[derive(Clone, Copy)]
pub(crate) enum GameCommandKind {
    Handler(GameCommandHandler),
    List,
    Capture,
}

pub struct GameCommand {
    name: String,
    title: Option<String>,
    description: String,
    input_schema: Value,
    output_schema: Option<Value>,
    annotations: GameCommandAnnotations,
    pub(crate) kind: GameCommandKind,
}

impl GameCommand {
    pub fn new(
        name: impl ToString,
        handler: GameCommandHandler,
        input_schema: Value,
        output_schema: Option<Value>,
    ) -> Self {
        Self::with_kind(
            name,
            GameCommandKind::Handler(handler),
            input_schema,
            output_schema,
        )
    }

    pub(crate) fn with_kind(
        name: impl ToString,
        kind: GameCommandKind,
        input_schema: Value,
        output_schema: Option<Value>,
    ) -> Self {
        Self {
            name: name.to_string(),
            title: None,
            description: Default::default(),
            input_schema,
            output_schema,
            annotations: Default::default(),
            kind,
        }
    }

    pub fn title(mut self, value: impl ToString) -> Self {
        self.title = Some(value.to_string());
        self
    }

    pub fn description(mut self, value: impl ToString) -> Self {
        self.description = value.to_string();
        self
    }

    pub fn read_only(mut self) -> Self {
        self.annotations.read_only = true;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.annotations.destructive = true;
        self
    }

    pub fn idempotent(mut self) -> Self {
        self.annotations.idempotent = true;
        self
    }

    pub fn open_world(mut self) -> Self {
        self.annotations.open_world = true;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title_text(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn description_text(&self) -> &str {
        &self.description
    }

    pub fn input_schema(&self) -> &Value {
        &self.input_schema
    }

    pub fn output_schema(&self) -> Option<&Value> {
        self.output_schema.as_ref()
    }

    pub fn annotations(&self) -> GameCommandAnnotations {
        self.annotations
    }

    pub fn to_json(&self) -> Value {
        let mut result = json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
            "annotations": self.annotations.to_json(),
        });
        if let Some(object) = result.as_object_mut() {
            if let Some(title) = self.title.as_ref() {
                object.insert("title".to_owned(), json!(title));
            }
            if let Some(schema) = self.output_schema.as_ref() {
                object.insert("outputSchema".to_owned(), schema.clone());
            }
        }
        result
    }
}

fn budget_to_json(budget: Option<InputBudget>) -> Value {
    match budget {
        Some(InputBudget::Frames(frames)) => json!({ "frames": frames }),
        Some(InputBudget::Steps(steps)) => json!({ "steps": steps }),
        None => Value::Null,
    }
}

// Reads how long an injection lasts. `frames` counts render frames and `steps`
// counts fixed update steps, so the two cannot be combined.
fn to_budget(frames: Option<usize>, steps: Option<usize>) -> Result<Option<InputBudget>, String> {
    match (frames, steps) {
        (Some(_), Some(_)) => {
            Err("Give either argument `frames` or argument `steps`, not both".to_owned())
        }
        (Some(frames), None) => Ok(Some(InputBudget::Frames(frames))),
        (None, Some(steps)) => Ok(Some(InputBudget::Steps(steps))),
        (None, None) => Ok(None),
    }
}

// Explains a miss the way the caller can act on it, because an injection that
// matches no ref writes nothing and would otherwise look like a working call.
fn no_match_error(kind: &str, name: &str, mapping: Option<&str>) -> String {
    match mapping {
        Some(mapping) => format!(
            "No mapping named `{mapping}` binds the {kind} `{name}`. Call `input.mappings` to see what is bound"
        ),
        None => format!(
            "No mapping on the stack binds the {kind} `{name}`. Call `input.mappings` to see what is bound"
        ),
    }
}

mod builtins {
    use super::{
        GameCommand, GameCommandKind, NoArgs, budget_to_json, input, input_schema_for,
        no_match_error, to_budget, ui,
    };
    use crate::context::GameContext;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::{Value, json};
    use spitfire_glow::graphics::SCREEN_CAPTURE_TARGET;

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub(super) struct CaptureArgs {
        #[serde(default)]
        #[schemars(
            description = "Name of the capture target. Leave it out to capture what the screen shows."
        )]
        pub target: Option<String>,
        #[serde(default)]
        #[schemars(
            description = "Longest side of the returned image, in pixels. A larger image is scaled down to fit."
        )]
        pub max_size: Option<u32>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct TimeStepArgs {
        #[serde(default)]
        #[schemars(description = "How many fixed steps to run. Defaults to 1.")]
        count: Option<usize>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct TimeScaleArgs {
        #[schemars(
            description = "How fast game time runs. 1 is normal speed, 0.5 is half speed, 0 stops it."
        )]
        value: f32,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct UiTreeArgs {
        #[serde(default)]
        #[schemars(
            description = "How deep to walk the widget tree. Leave it out to report the whole tree."
        )]
        max_depth: Option<usize>,
    }

    // In the `action` and `axis` descriptions below, backticks are load bearing.
    // A test parses every backticked token, so only a spelling the parser
    // accepts may carry them.
    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct InputPressArgs {
        #[schemars(
            description = "Action to hold. The form is a kind and a name, such as `key:KeyD`, `key:Escape`, `mouse:Left`, `gamepad_button:South` or `axis:0`. The kinds without a name are `touch`."
        )]
        action: String,
        #[serde(default)]
        #[schemars(
            description = "Name of one mapping to write to. Leave it out to write to every mapping on the stack."
        )]
        mapping: Option<String>,
        #[serde(default)]
        #[schemars(
            description = "Hold the action for this many render frames. A fixed step can miss a frame budget completely."
        )]
        frames: Option<usize>,
        #[serde(default)]
        #[schemars(
            description = "Hold the action for this many fixed steps. Use this for anything the game reads with `is_pressed`."
        )]
        steps: Option<usize>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct InputReleaseArgs {
        #[schemars(
            description = "Action to release. It takes the same form as the action of input.press, such as `key:KeyD`."
        )]
        action: String,
        #[serde(default)]
        #[schemars(
            description = "Name of one mapping to write to. Leave it out to write to every mapping on the stack."
        )]
        mapping: Option<String>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct InputAxisArgs {
        #[schemars(
            description = "Axis to write. The form is a kind and a name, such as `key:KeyW`, `mouse:Left`, `gamepad_axis:LeftStickX` or `axis:0`. The kinds without a name are `mouse_position_x`, `mouse_position_y`, `mouse_wheel_x`, `mouse_wheel_y`, `touch_x` and `touch_y`."
        )]
        axis: String,
        #[schemars(description = "Value to write to the axis.")]
        value: f32,
        #[serde(default)]
        #[schemars(
            description = "Name of one mapping to write to. Leave it out to write to every mapping on the stack."
        )]
        mapping: Option<String>,
        #[serde(default)]
        #[schemars(description = "Hold the value for this many render frames.")]
        frames: Option<usize>,
        #[serde(default)]
        #[schemars(description = "Hold the value for this many fixed steps.")]
        steps: Option<usize>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct EditorEditingArgs {
        #[schemars(description = "True enters edit mode, false leaves it.")]
        #[allow(dead_code)]
        editing: bool,
    }

    fn time_status(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        let time = &context.globals.time;
        Ok(json!({
            "paused": time.is_paused(),
            "time_scale": time.time_scale(),
            "step": time.step(),
            "total_time": time.total_time(),
            "pending_steps": time.pending_steps(),
            "steps_last_frame": time.steps_last_frame(),
            "dropped_steps": time.dropped_steps(),
        }))
    }

    fn time_pause(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        context.globals.time.pause();
        Ok(json!({ "paused": true }))
    }

    fn time_resume(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        context.globals.time.resume();
        Ok(json!({ "paused": false }))
    }

    fn time_step(context: &mut GameContext, arguments: TimeStepArgs) -> Result<Value, String> {
        context
            .globals
            .time
            .request_steps(arguments.count.unwrap_or(1));
        Ok(json!({ "pending_steps": context.globals.time.pending_steps() }))
    }

    fn time_set_scale(
        context: &mut GameContext,
        arguments: TimeScaleArgs,
    ) -> Result<Value, String> {
        context.globals.time.set_time_scale(arguments.value);
        Ok(json!({ "time_scale": context.globals.time.time_scale() }))
    }

    fn ui_tree(context: &mut GameContext, arguments: UiTreeArgs) -> Result<Value, String> {
        Ok(ui::tree(context, arguments.max_depth))
    }

    fn ui_text(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        Ok(ui::texts(context))
    }

    fn input_mappings(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        Ok(input::mappings(context))
    }

    fn input_status(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        Ok(context.globals.input.status())
    }

    fn input_press(context: &mut GameContext, arguments: InputPressArgs) -> Result<Value, String> {
        let action = input::parse_action(&arguments.action)?;
        let budget = to_budget(arguments.frames, arguments.steps)?;
        let matched =
            input::count_action_refs(context.input, arguments.mapping.as_deref(), &action);
        if matched == 0 {
            return Err(no_match_error(
                "action",
                &arguments.action,
                arguments.mapping.as_deref(),
            ));
        }
        context
            .globals
            .input
            .press(arguments.mapping, action, budget);
        Ok(json!({ "matched": matched, "budget": budget_to_json(budget) }))
    }

    fn input_release(
        context: &mut GameContext,
        arguments: InputReleaseArgs,
    ) -> Result<Value, String> {
        let action = input::parse_action(&arguments.action)?;
        let matched =
            input::count_action_refs(context.input, arguments.mapping.as_deref(), &action);
        context.globals.input.release(arguments.mapping, action);
        Ok(json!({ "matched": matched }))
    }

    fn input_axis(context: &mut GameContext, arguments: InputAxisArgs) -> Result<Value, String> {
        let axis = input::parse_axis(&arguments.axis)?;
        let budget = to_budget(arguments.frames, arguments.steps)?;
        let matched = input::count_axis_refs(context.input, arguments.mapping.as_deref(), &axis);
        if matched == 0 {
            return Err(no_match_error(
                "axis",
                &arguments.axis,
                arguments.mapping.as_deref(),
            ));
        }
        context
            .globals
            .input
            .axis(arguments.mapping, axis, arguments.value, budget);
        Ok(json!({ "matched": matched, "budget": budget_to_json(budget) }))
    }

    fn input_clear(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        context.globals.input.clear();
        Ok(json!({
            "actions": context.globals.input.pending_actions(),
            "axes": context.globals.input.pending_axes(),
        }))
    }

    fn editor_status(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        #[cfg(feature = "editor")]
        {
            Ok(json!({
                "available": true,
                "editing": context.globals.editor.is_editing(),
            }))
        }
        #[cfg(not(feature = "editor"))]
        {
            let _ = context;
            Ok(json!({ "available": false, "editing": false }))
        }
    }

    fn editor_set_editing(
        context: &mut GameContext,
        arguments: EditorEditingArgs,
    ) -> Result<Value, String> {
        #[cfg(feature = "editor")]
        {
            context.globals.editor.set_editing(arguments.editing);
            Ok(json!({ "editing": context.globals.editor.is_editing() }))
        }
        #[cfg(not(feature = "editor"))]
        {
            let _ = (context, arguments);
            Err("This build has no editor. Rebuild with the `editor` feature".to_owned())
        }
    }

    fn app_close(context: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        context.globals.app.request_close();
        Ok(json!({ "close_requested": true }))
    }

    pub(super) fn default_capture_target() -> String {
        SCREEN_CAPTURE_TARGET.to_owned()
    }

    // Both editor commands are registered whatever the build, so the tool list
    // looks the same everywhere. A caller then learns that the editor is missing
    // from an answer, rather than from a missing name.
    pub(super) fn all(list_command: &str, capture_command: &str) -> Vec<GameCommand> {
        vec![
            GameCommand::with_kind(
                list_command,
                GameCommandKind::List,
                input_schema_for::<NoArgs>(),
                None,
            )
            .title("List commands")
            .description(
                "List every command this game answers, with the argument schema of each one.",
            )
            .read_only(),
            GameCommand::with_kind(
                capture_command,
                GameCommandKind::Capture,
                input_schema_for::<CaptureArgs>(),
                None,
            )
            .title("Capture a frame")
            .description(concat!(
                "Capture a rendered frame as a PNG image, returned as base64 text. ",
                "The answer arrives one frame after the request.",
            ))
            .read_only(),
            game_command!("time.status" => time_status)
                .title("Read the clock")
                .description(
                    "Report the clock: paused state, time scale, step size and step counters.",
                )
                .read_only(),
            game_command!("time.pause" => time_pause)
                .title("Pause game time")
                .description(concat!(
                    "Stop game time. Update still runs with a delta of 0, so assets keep ",
                    "loading and the game keeps drawing.",
                ))
                .idempotent(),
            game_command!("time.resume" => time_resume)
                .title("Resume game time")
                .description("Let game time run again after a pause.")
                .idempotent(),
            game_command!("time.step" => time_step)
                .title("Run fixed steps")
                .description(concat!(
                    "Run a set number of fixed update steps. This works while paused, and it ",
                    "is how to move a paused game forward by a known amount. A count above ",
                    "the per frame step cap needs more than one frame to run, so read ",
                    "time.status until pending_steps reaches zero before you read the result.",
                )),
            game_command!("time.set_scale" => time_set_scale)
                .title("Set the time scale")
                .description("Set how fast game time runs. 1 is normal speed.")
                .idempotent(),
            game_command!("ui.tree" => ui_tree)
                .title("Read the widget tree")
                .description(concat!(
                    "Report the GUI widget tree with the screen rect of every widget. A rect is ",
                    "in physical pixels, in the same space as pointer coordinates.",
                ))
                .read_only(),
            game_command!("ui.text" => ui_text)
                .title("Read GUI text")
                .description("Report every text the GUI draws, with its widget id and its rect.")
                .read_only(),
            game_command!("input.mappings" => input_mappings)
                .title("List input mappings")
                .description(
                    "Report the input mapping stack, and the actions and axes each mapping binds.",
                )
                .read_only(),
            game_command!("input.status" => input_status)
                .title("Read injected input")
                .description("Report which injected actions and axes are still held.")
                .read_only(),
            game_command!("input.press" => input_press)
                .title("Press an action")
                .description(concat!(
                    "Hold an input action, as if the player pressed it. Injection bypasses the ",
                    "window event path, so it cannot catch a bug in the input mapping itself.",
                )),
            game_command!("input.release" => input_release)
                .title("Release an action")
                .description("Release an input action that injection holds.")
                .idempotent(),
            game_command!("input.axis" => input_axis)
                .title("Write an axis")
                .description(
                    "Set an input axis, as if the player moved a stick, a trigger or the mouse.",
                )
                .idempotent(),
            game_command!("input.clear" => input_clear)
                .title("Clear injected input")
                .description("Drop every injected action and axis at once.")
                .idempotent(),
            game_command!("editor.status" => editor_status)
                .title("Read editor state")
                .description(
                    "Report whether this build has the editor, and whether it edits right now.",
                )
                .read_only(),
            game_command!("editor.set_editing" => editor_set_editing)
                .title("Enter or leave edit mode")
                .description(concat!(
                    "Enter or leave editor edit mode. Edit mode skips the whole update phase, ",
                    "so the game answers commands but runs no game logic.",
                ))
                .destructive()
                .idempotent(),
            game_command!("app.close" => app_close)
                .title("Close the game")
                .description("Ask the window to close, which ends the game.")
                .destructive()
                .idempotent(),
        ]
    }
}

struct DeferredCapture {
    id: GameCommandId,
    name: String,
    target: String,
    max_size: Option<u32>,
    response: Option<Sender<GameCommandResponse>>,
    frames_waited: usize,
}

pub struct GameCommands {
    sender: Sender<GameCommandRequest>,
    receiver: Receiver<GameCommandRequest>,
    commands: HashMap<String, GameCommand>,
    deferred_captures: Vec<DeferredCapture>,
    pub max_commands_per_frame: usize,
    pub max_frames_waiting_for_capture: usize,
    handled_last_frame: usize,
}

impl Default for GameCommands {
    fn default() -> Self {
        let (sender, receiver) = unbounded();
        let mut result = Self {
            sender,
            receiver,
            commands: Default::default(),
            deferred_captures: Default::default(),
            max_commands_per_frame: 64,
            max_frames_waiting_for_capture: 10,
            handled_last_frame: 0,
        };
        for command in builtins::all(Self::LIST_COMMAND, Self::CAPTURE_COMMAND) {
            let _ = result.register(command);
        }
        result
    }
}

impl GameCommands {
    pub const LIST_COMMAND: &'static str = "commands.list";
    pub const CAPTURE_COMMAND: &'static str = "render.capture";

    pub fn with_max_commands_per_frame(mut self, value: usize) -> Self {
        self.max_commands_per_frame = value;
        self
    }

    pub fn with_command(mut self, command: GameCommand) -> Self {
        let _ = self.register(command);
        self
    }

    pub fn handle(&self) -> GameCommandsHandle {
        GameCommandsHandle {
            sender: self.sender.clone(),
        }
    }

    pub fn register(&mut self, command: GameCommand) -> Result<(), String> {
        if self.commands.contains_key(command.name()) {
            return Err(format!(
                "There is already a command named: {}",
                command.name()
            ));
        }
        self.commands.insert(command.name().to_owned(), command);
        Ok(())
    }

    pub fn unregister(&mut self, name: &str) -> bool {
        self.commands.remove(name).is_some()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.commands.keys().map(|name| name.as_str())
    }

    pub fn get(&self, name: &str) -> Option<&GameCommand> {
        self.commands.get(name)
    }

    pub fn descriptors(&self) -> Vec<Value> {
        let mut commands = self.commands.values().collect::<Vec<_>>();
        commands.sort_by_key(|command| command.name());
        commands
            .into_iter()
            .map(|command| command.to_json())
            .collect()
    }

    pub fn handled_last_frame(&self) -> usize {
        self.handled_last_frame
    }

    pub fn pending(&self) -> usize {
        self.receiver.len()
    }

    pub fn process(&mut self, context: &mut GameContext) {
        self.resolve_deferred_captures(context);
        self.handled_last_frame = 0;
        while self.handled_last_frame < self.max_commands_per_frame {
            let mut request = match self.receiver.try_recv() {
                Ok(request) => request,
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            };
            self.handled_last_frame += 1;
            let arguments = std::mem::take(&mut request.arguments);
            let kind = self
                .commands
                .get(request.name.as_str())
                .map(|command| command.kind);
            let result = match kind {
                Some(GameCommandKind::Handler(handler)) => handler(context, arguments),
                Some(GameCommandKind::List) => Ok(json!({ "commands": self.descriptors() })),
                Some(GameCommandKind::Capture) => {
                    match self.defer_capture(context, request, arguments) {
                        Ok(()) => continue,
                        Err((request, error)) => {
                            Self::answer(request, Err(error));
                            continue;
                        }
                    }
                }
                None => Err(format!("There is no command named: {}", request.name)),
            };
            Self::answer(request, result);
        }
    }

    fn answer(request: GameCommandRequest, result: Result<Value, String>) {
        if let Some(response) = request.response {
            let _ = response.send(GameCommandResponse {
                id: request.id,
                name: request.name,
                result,
            });
        }
    }

    fn defer_capture(
        &mut self,
        context: &mut GameContext,
        request: GameCommandRequest,
        arguments: Value,
    ) -> Result<(), (GameCommandRequest, String)> {
        let arguments =
            match serde_json::from_value::<builtins::CaptureArgs>(normalize_arguments(arguments)) {
                Ok(arguments) => arguments,
                Err(error) => {
                    return Err((request, format!("Could not read the arguments: {error}")));
                }
            };
        let target = arguments
            .target
            .unwrap_or_else(builtins::default_capture_target);
        context.graphics.request_capture(target.clone());
        self.deferred_captures.push(DeferredCapture {
            id: request.id,
            name: request.name,
            target,
            max_size: arguments.max_size,
            response: request.response,
            frames_waited: 0,
        });
        Ok(())
    }

    fn resolve_deferred_captures(&mut self, context: &mut GameContext) {
        let max_frames_waiting = self.max_frames_waiting_for_capture;
        self.deferred_captures.retain_mut(|deferred| {
            let result = match context.graphics.take_capture(&deferred.target) {
                Some(frame) => capture::encode_png(&deferred.target, frame, deferred.max_size),
                None => {
                    deferred.frames_waited += 1;
                    if deferred.frames_waited <= max_frames_waiting {
                        return true;
                    }
                    context.graphics.cancel_capture(&deferred.target);
                    Err(format!(
                        "Nothing resolved a capture for target `{}` within {} frames",
                        deferred.target, max_frames_waiting
                    ))
                }
            };
            if let Some(response) = deferred.response.take() {
                let _ = response.send(GameCommandResponse {
                    id: deferred.id,
                    name: std::mem::take(&mut deferred.name),
                    result,
                });
            }
            false
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{GameCommand, GameCommands, NoArgs, input_schema_for, output_schema_for};
    use crate::context::GameContext;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct PingArgs {
        #[schemars(description = "How many times to answer.")]
        count: usize,
        #[serde(default)]
        label: Option<String>,
    }

    #[derive(Debug, Serialize, JsonSchema)]
    struct PingOutput {
        answers: usize,
        label: Option<String>,
    }

    fn ping(_: &mut GameContext, arguments: PingArgs) -> Result<PingOutput, String> {
        Ok(PingOutput {
            answers: arguments.count,
            label: arguments.label,
        })
    }

    fn untyped(_: &mut GameContext, _: NoArgs) -> Result<Value, String> {
        Ok(json!("pong"))
    }

    fn ping_command() -> GameCommand {
        game_command!("game.ping" => ping)
    }

    #[test]
    fn test_commands_refuse_a_duplicate_name() {
        let mut commands = GameCommands::default();

        assert!(commands.register(ping_command()).is_ok());
        assert!(commands.register(ping_command()).is_err());
        assert!(
            commands
                .register(game_command!("time.pause" => untyped))
                .is_err()
        );
    }

    #[test]
    fn test_commands_list_their_names_including_the_built_ins() {
        let mut commands = GameCommands::default();
        commands.register(ping_command()).unwrap();
        let names = commands.names().collect::<Vec<_>>();

        assert!(names.contains(&GameCommands::LIST_COMMAND));
        assert!(names.contains(&GameCommands::CAPTURE_COMMAND));
        assert!(names.contains(&"game.ping"));
        assert!(names.contains(&"time.pause"));
        assert!(names.contains(&"time.step"));
        assert!(names.contains(&"app.close"));
    }

    #[test]
    fn test_commands_unregister_removes_a_name() {
        let mut commands = GameCommands::default();
        commands.register(ping_command()).unwrap();

        assert!(commands.unregister("game.ping"));
        assert!(!commands.unregister("game.ping"));
        assert!(commands.register(ping_command()).is_ok());
    }

    #[test]
    fn test_commands_queue_requests_until_the_game_processes_them() {
        let commands = GameCommands::default();
        let handle = commands.handle();

        assert_eq!(commands.pending(), 0);
        let pending = handle.call("time.status", json!({})).unwrap();
        handle.notify("time.pause", json!({})).unwrap();
        assert_eq!(commands.pending(), 2);
        assert!(pending.try_take().is_none());
    }

    #[test]
    fn test_the_macro_derives_an_object_schema_from_the_argument_type() {
        let command = ping_command();
        let schema = command.input_schema();

        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["properties"]["count"]["type"], json!("integer"));
        assert_eq!(
            schema["properties"]["count"]["description"],
            json!("How many times to answer.")
        );
        assert_eq!(schema["required"], json!(["count"]));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert!(schema.get("title").is_none());
    }

    #[test]
    fn test_a_typed_result_carries_an_output_schema_and_a_json_result_does_not() {
        assert!(ping_command().output_schema().is_some());
        assert!(output_schema_for::<PingOutput>().is_some());
        assert!(output_schema_for::<Value>().is_none());
    }

    #[test]
    fn test_the_no_arguments_schema_takes_an_empty_object() {
        let schema = input_schema_for::<NoArgs>();

        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"], json!(false));
    }

    #[test]
    fn test_a_descriptor_reports_what_the_tool_list_needs() {
        let commands = GameCommands::default();
        let descriptors = commands.descriptors();
        let capture = descriptors
            .iter()
            .find(|value| value["name"] == json!(GameCommands::CAPTURE_COMMAND))
            .unwrap();
        let close = descriptors
            .iter()
            .find(|value| value["name"] == json!("app.close"))
            .unwrap();

        assert!(descriptors.len() > 1);
        assert!(capture["description"].as_str().unwrap().contains("PNG"));
        assert_eq!(capture["title"], json!("Capture a frame"));
        assert_eq!(capture["annotations"]["readOnlyHint"], json!(true));
        assert_eq!(capture["annotations"]["destructiveHint"], json!(false));
        assert!(
            capture["inputSchema"]["properties"]["max_size"]["description"]
                .as_str()
                .is_some()
        );
        assert_eq!(close["annotations"]["destructiveHint"], json!(true));
        assert_eq!(close["annotations"]["readOnlyHint"], json!(false));
    }

    // A description that names a spelling the parser refuses sends an agent
    // straight into a failing call, and the tool list is the only place an agent
    // learns the spelling from.
    #[test]
    fn test_every_example_spelling_in_a_description_parses() {
        let commands = GameCommands::default();
        let examples = |command: &str, argument: &str| -> Vec<String> {
            commands.get(command).unwrap().input_schema()["properties"][argument]["description"]
                .as_str()
                .unwrap()
                .split('`')
                .skip(1)
                .step_by(2)
                .map(|text| text.to_owned())
                .collect()
        };

        for example in examples("input.press", "action") {
            assert!(
                crate::input::parse_action(&example).is_ok(),
                "`{example}` is in the description of `input.press`, and the parser refuses it"
            );
        }
        for example in examples("input.release", "action") {
            assert!(
                crate::input::parse_action(&example).is_ok(),
                "`{example}` is in the description of `input.release`, and the parser refuses it"
            );
        }
        for example in examples("input.axis", "axis") {
            assert!(
                crate::input::parse_axis(&example).is_ok(),
                "`{example}` is in the description of `input.axis`, and the parser refuses it"
            );
        }
    }

    #[test]
    fn test_descriptors_come_out_sorted_by_name() {
        let commands = GameCommands::default();
        let names = commands
            .descriptors()
            .into_iter()
            .map(|value| value["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let mut sorted = names.clone();
        sorted.sort();

        assert_eq!(names, sorted);
    }
}
