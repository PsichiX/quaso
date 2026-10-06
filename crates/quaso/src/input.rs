use crate::context::GameContext;
use serde_json::{Value, json};
use spitfire_input::{
    GamepadAxis, GamepadButton, InputAction, InputAxis, InputConsume, InputContext, KeyCode,
    MouseButton, VirtualAction, VirtualAxis,
};

// Turns a virtual action into the compact text name the agent protocol uses.
// The same vocabulary parses back with `parse_action`, so a name reported by
// `mappings` can be sent straight back to an injection command.
pub fn action_name(action: &VirtualAction) -> String {
    match action {
        VirtualAction::KeyButton(code) => format!("key:{code:?}"),
        VirtualAction::MouseButton(button) => format!("mouse:{}", mouse_button_name(*button)),
        VirtualAction::Axis(id) => format!("axis:{id}"),
        VirtualAction::GamepadButton(button) => format!("gamepad_button:{button:?}"),
        VirtualAction::GamepadAxis(axis) => format!("gamepad_axis:{axis:?}"),
        VirtualAction::Touch => "touch".to_owned(),
    }
}

pub fn axis_name(axis: &VirtualAxis) -> String {
    match axis {
        VirtualAxis::KeyButton(code) => format!("key:{code:?}"),
        VirtualAxis::MousePositionX => "mouse_position_x".to_owned(),
        VirtualAxis::MousePositionY => "mouse_position_y".to_owned(),
        VirtualAxis::MouseWheelX => "mouse_wheel_x".to_owned(),
        VirtualAxis::MouseWheelY => "mouse_wheel_y".to_owned(),
        VirtualAxis::MouseButton(button) => format!("mouse:{}", mouse_button_name(*button)),
        VirtualAxis::Axis(id) => format!("axis:{id}"),
        VirtualAxis::GamepadButton(button) => format!("gamepad_button:{button:?}"),
        VirtualAxis::GamepadAxis(axis) => format!("gamepad_axis:{axis:?}"),
        VirtualAxis::TouchX => "touch_x".to_owned(),
        VirtualAxis::TouchY => "touch_y".to_owned(),
    }
}

pub fn parse_action(text: &str) -> Result<VirtualAction, String> {
    match split_name(text) {
        ("key", Some(value)) => Ok(VirtualAction::KeyButton(parse_key(value)?)),
        ("mouse", Some(value)) => Ok(VirtualAction::MouseButton(parse_mouse_button(value)?)),
        ("axis", Some(value)) => Ok(VirtualAction::Axis(parse_index(value)?)),
        ("gamepad_button", Some(value)) => {
            Ok(VirtualAction::GamepadButton(parse_gamepad_button(value)?))
        }
        ("gamepad_axis", Some(value)) => Ok(VirtualAction::GamepadAxis(parse_gamepad_axis(value)?)),
        ("touch", None) => Ok(VirtualAction::Touch),
        _ => Err(format!(
            "`{text}` does not name an input action. Expected one of: \
             key:<Name>, mouse:<Left|Right|Middle|number>, axis:<number>, \
             gamepad_button:<Name>, gamepad_axis:<Name>, touch"
        )),
    }
}

pub fn parse_axis(text: &str) -> Result<VirtualAxis, String> {
    match split_name(text) {
        ("key", Some(value)) => Ok(VirtualAxis::KeyButton(parse_key(value)?)),
        ("mouse", Some(value)) => Ok(VirtualAxis::MouseButton(parse_mouse_button(value)?)),
        ("axis", Some(value)) => Ok(VirtualAxis::Axis(parse_index(value)?)),
        ("gamepad_button", Some(value)) => {
            Ok(VirtualAxis::GamepadButton(parse_gamepad_button(value)?))
        }
        ("gamepad_axis", Some(value)) => Ok(VirtualAxis::GamepadAxis(parse_gamepad_axis(value)?)),
        ("mouse_position_x", None) => Ok(VirtualAxis::MousePositionX),
        ("mouse_position_y", None) => Ok(VirtualAxis::MousePositionY),
        ("mouse_wheel_x", None) => Ok(VirtualAxis::MouseWheelX),
        ("mouse_wheel_y", None) => Ok(VirtualAxis::MouseWheelY),
        ("touch_x", None) => Ok(VirtualAxis::TouchX),
        ("touch_y", None) => Ok(VirtualAxis::TouchY),
        _ => Err(format!(
            "`{text}` does not name an input axis. Expected one of: \
             key:<Name>, mouse:<Left|Right|Middle|number>, axis:<number>, \
             gamepad_button:<Name>, gamepad_axis:<Name>, \
             mouse_position_x, mouse_position_y, mouse_wheel_x, mouse_wheel_y, touch_x, touch_y"
        )),
    }
}

pub fn action_state_name(state: InputAction) -> &'static str {
    match state {
        InputAction::Idle => "Idle",
        InputAction::Pressed => "Pressed",
        InputAction::Hold => "Hold",
        InputAction::Released => "Released",
    }
}

// Reports the whole mapping stack, highest priority first.
// `InputContext` checks the stack from the top down, so the first entry here is
// the one that receives an input event first. `index` is the position in the
// stack itself, so it does not follow the array order.
pub fn mappings(context: &GameContext) -> Value {
    let mut entries = context
        .input
        .stack()
        .enumerate()
        .map(|(index, mapping)| {
            let Some(mapping) = mapping.read() else {
                return json!({ "index": index, "locked": true });
            };
            let mut actions = mapping
                .actions
                .iter()
                .map(|(id, reference)| (action_name(id), reference.get()))
                .collect::<Vec<_>>();
            actions.sort_by(|a, b| a.0.cmp(&b.0));
            let mut axes = mapping
                .axes
                .iter()
                .map(|(id, reference)| (axis_name(id), reference.get()))
                .collect::<Vec<_>>();
            axes.sort_by(|a, b| a.0.cmp(&b.0));
            json!({
                "index": index,
                "name": mapping.name.as_ref(),
                "layer": mapping.layer,
                "consume": consume_name(mapping.consume),
                "has_validator": mapping.validator.is_some(),
                "valid": mapping
                    .validator
                    .as_ref()
                    .is_none_or(|validator| validator()),
                "gamepad": mapping.gamepad.map(usize::from),
                "actions": actions
                    .into_iter()
                    .map(|(id, state)| json!({ "id": id, "state": action_state_name(state) }))
                    .collect::<Vec<_>>(),
                "axes": axes
                    .into_iter()
                    .map(|(id, value)| json!({ "id": id, "value": value.0 }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    entries.reverse();
    json!({ "mappings": entries })
}

// Counts the input refs an injection would write to.
// A command handler calls this before it queues an injection, so a name that
// matches nothing is reported to the caller instead of failing silently.
pub fn count_action_refs(
    input: &InputContext,
    mapping: Option<&str>,
    action: &VirtualAction,
) -> usize {
    input
        .stack()
        .filter(|entry| match entry.read() {
            Some(entry) => {
                mapping.is_none_or(|mapping| entry.name.as_ref() == mapping)
                    && entry.actions.contains_key(action)
            }
            None => false,
        })
        .count()
}

pub fn count_axis_refs(input: &InputContext, mapping: Option<&str>, axis: &VirtualAxis) -> usize {
    input
        .stack()
        .filter(|entry| match entry.read() {
            Some(entry) => {
                mapping.is_none_or(|mapping| entry.name.as_ref() == mapping)
                    && entry.axes.contains_key(axis)
            }
            None => false,
        })
        .count()
}

// How long an injection lasts.
//
// `Frames` counts render frames and `Steps` counts fixed update steps. The two
// are not the same: one render frame can run several fixed steps, and a paused
// game runs render frames with no steps at all.
//
// `Steps` is the one an edge triggered read needs. Game code that reads
// `is_pressed()` only sees a press if a fixed step runs while the state is
// `Pressed`, and `input.press` and `time.step` arrive on different frames. So a
// `Frames` press can be missed. A `Steps` press stays `Pressed` until a step
// actually runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBudget {
    Frames(usize),
    Steps(usize),
}

impl InputBudget {
    fn take(&mut self, steps: usize) {
        match self {
            Self::Frames(left) => *left = left.saturating_sub(1),
            Self::Steps(left) => *left = left.saturating_sub(steps),
        }
    }

    fn is_spent(&self) -> bool {
        match self {
            Self::Frames(left) | Self::Steps(left) => *left == 0,
        }
    }

    fn to_json(self) -> Value {
        match self {
            Self::Frames(left) => json!({ "frames_left": left }),
            Self::Steps(left) => json!({ "steps_left": left }),
        }
    }
}

#[derive(Debug, Clone)]
struct ActionInjection {
    mapping: Option<String>,
    action: VirtualAction,
    budget: Option<InputBudget>,
    started: bool,
    stepped: bool,
    releasing: bool,
}

#[derive(Debug, Clone)]
struct AxisInjection {
    mapping: Option<String>,
    axis: VirtualAxis,
    value: f32,
    budget: Option<InputBudget>,
}

// Injects input by writing straight into the refs a mapping holds.
// The write happens once per frame at one fixed point, right after the command
// queue runs and before any game code. That point is also right after the
// previous frame called `InputContext::maintain`, so an injected state survives
// the whole frame and `maintain` then moves it on.
//
// Injection bypasses the window event path, so it also bypasses the consume and
// the layer rules. Injection is therefore deterministic, and it cannot catch a
// bug in the input mapping itself.
#[derive(Debug, Default)]
pub struct GameInputControl {
    actions: Vec<ActionInjection>,
    axes: Vec<AxisInjection>,
    applied_last_frame: usize,
    seen_total_steps: u64,
}

impl GameInputControl {
    // Holds an action down. A `budget` of `None` holds it until `release`.
    pub fn press(
        &mut self,
        mapping: Option<String>,
        action: VirtualAction,
        budget: Option<InputBudget>,
    ) {
        self.actions
            .retain(|entry| entry.mapping != mapping || entry.action != action);
        self.actions.push(ActionInjection {
            mapping,
            action,
            budget: budget.map(clamp_budget),
            started: false,
            stepped: false,
            releasing: false,
        });
    }

    pub fn release(&mut self, mapping: Option<String>, action: VirtualAction) {
        if let Some(entry) = self
            .actions
            .iter_mut()
            .find(|entry| entry.mapping == mapping && entry.action == action)
        {
            entry.releasing = true;
            return;
        }
        self.actions.push(ActionInjection {
            mapping,
            action,
            budget: None,
            started: false,
            stepped: false,
            releasing: true,
        });
    }

    // Writes an axis value. A `budget` of `None` writes the value once, on the
    // next frame, and leaves the value there. That fits a mouse position, which
    // nothing resets. A budget writes the value until the budget runs out and
    // then writes one zero, which fits a stick or a wheel.
    pub fn axis(
        &mut self,
        mapping: Option<String>,
        axis: VirtualAxis,
        value: f32,
        budget: Option<InputBudget>,
    ) {
        self.axes
            .retain(|entry| entry.mapping != mapping || entry.axis != axis);
        self.axes.push(AxisInjection {
            mapping,
            axis,
            value,
            budget: budget.map(clamp_budget),
        });
    }

    // Ends every injection. A held action gets one released write and a held
    // axis gets one zero write, so nothing stays stuck down.
    pub fn clear(&mut self) {
        for entry in &mut self.actions {
            entry.releasing = true;
        }
        for entry in &mut self.axes {
            entry.value = 0.0;
            entry.budget = None;
        }
    }

    pub fn pending_actions(&self) -> usize {
        self.actions.len()
    }

    pub fn pending_axes(&self) -> usize {
        self.axes.len()
    }

    pub fn applied_last_frame(&self) -> usize {
        self.applied_last_frame
    }

    pub fn status(&self) -> Value {
        json!({
            "applied_last_frame": self.applied_last_frame,
            "actions": self
                .actions
                .iter()
                .map(|entry| {
                    let mut value = json!({
                        "action": action_name(&entry.action),
                        "mapping": entry.mapping,
                        "state": action_state_name(entry.next_state()),
                    });
                    merge(&mut value, entry.budget);
                    value
                })
                .collect::<Vec<_>>(),
            "axes": self
                .axes
                .iter()
                .map(|entry| {
                    let mut value = json!({
                        "axis": axis_name(&entry.axis),
                        "mapping": entry.mapping,
                        "value": entry.value,
                    });
                    merge(&mut value, entry.budget);
                    value
                })
                .collect::<Vec<_>>(),
        })
    }

    // `total_steps` is the running fixed step count from
    // `GameTimeControl::total_steps`. The control works out how many steps
    // happened since its own previous call, so it stays correct on a frame that
    // skipped the update phase and ran no steps at all.
    pub fn apply(&mut self, input: &InputContext, total_steps: u64) {
        let steps = total_steps.saturating_sub(self.seen_total_steps) as usize;
        self.seen_total_steps = total_steps;
        let mut applied = 0;
        self.actions.retain_mut(|entry| {
            if entry.started
                && let Some(budget) = entry.budget.as_mut()
            {
                budget.take(steps);
                if steps > 0 {
                    entry.stepped = true;
                }
            }
            let state = entry.next_state();
            applied += write_action(input, entry.mapping.as_deref(), &entry.action, state);
            entry.started = true;
            state != InputAction::Released
        });
        self.axes.retain_mut(|entry| {
            applied += write_axis(input, entry.mapping.as_deref(), &entry.axis, entry.value);
            let Some(budget) = entry.budget.as_mut() else {
                return false;
            };
            budget.take(steps);
            if budget.is_spent() {
                entry.value = 0.0;
                entry.budget = None;
            }
            true
        });
        self.applied_last_frame = applied;
    }
}

impl ActionInjection {
    // A step budget keeps the action `Pressed` until a fixed step has run,
    // because edge triggered game code reads `Pressed` and nothing else.
    fn next_state(&self) -> InputAction {
        if self.releasing || self.budget.is_some_and(|budget| budget.is_spent()) {
            return InputAction::Released;
        }
        if !self.started || (matches!(self.budget, Some(InputBudget::Steps(_))) && !self.stepped) {
            return InputAction::Pressed;
        }
        InputAction::Hold
    }
}

fn clamp_budget(budget: InputBudget) -> InputBudget {
    match budget {
        InputBudget::Frames(count) => InputBudget::Frames(count.max(1)),
        InputBudget::Steps(count) => InputBudget::Steps(count.max(1)),
    }
}

fn merge(value: &mut Value, budget: Option<InputBudget>) {
    let Some(Value::Object(fields)) = budget.map(InputBudget::to_json) else {
        return;
    };
    if let Value::Object(target) = value {
        target.extend(fields);
    }
}

fn write_action(
    input: &InputContext,
    mapping: Option<&str>,
    action: &VirtualAction,
    state: InputAction,
) -> usize {
    let mut count = 0;
    for entry in input.stack() {
        let Some(entry) = entry.read() else {
            continue;
        };
        if mapping.is_some_and(|mapping| entry.name.as_ref() != mapping) {
            continue;
        }
        if let Some(reference) = entry.actions.get(action) {
            reference.set(state);
            count += 1;
        }
    }
    count
}

fn write_axis(
    input: &InputContext,
    mapping: Option<&str>,
    axis: &VirtualAxis,
    value: f32,
) -> usize {
    let mut count = 0;
    for entry in input.stack() {
        let Some(entry) = entry.read() else {
            continue;
        };
        if mapping.is_some_and(|mapping| entry.name.as_ref() != mapping) {
            continue;
        }
        if let Some(reference) = entry.axes.get(axis) {
            reference.set(InputAxis(value));
            count += 1;
        }
    }
    count
}

fn consume_name(consume: InputConsume) -> &'static str {
    match consume {
        InputConsume::None => "None",
        InputConsume::Hit => "Hit",
        InputConsume::All => "All",
    }
}

fn mouse_button_name(button: MouseButton) -> String {
    match button {
        MouseButton::Left => "Left".to_owned(),
        MouseButton::Right => "Right".to_owned(),
        MouseButton::Middle => "Middle".to_owned(),
        MouseButton::Back => "Back".to_owned(),
        MouseButton::Forward => "Forward".to_owned(),
        MouseButton::Other(index) => index.to_string(),
    }
}

fn split_name(text: &str) -> (&str, Option<&str>) {
    match text.split_once(':') {
        Some((kind, value)) => (kind, Some(value)),
        None => (text, None),
    }
}

fn parse_key(text: &str) -> Result<KeyCode, String> {
    serde_json::from_value(Value::String(text.to_owned()))
        .map_err(|_| format!("`{text}` does not name a keyboard key"))
}

fn parse_gamepad_button(text: &str) -> Result<GamepadButton, String> {
    serde_json::from_value(Value::String(text.to_owned()))
        .map_err(|_| format!("`{text}` does not name a gamepad button"))
}

fn parse_gamepad_axis(text: &str) -> Result<GamepadAxis, String> {
    serde_json::from_value(Value::String(text.to_owned()))
        .map_err(|_| format!("`{text}` does not name a gamepad axis"))
}

fn parse_mouse_button(text: &str) -> Result<MouseButton, String> {
    match text {
        "Left" => Ok(MouseButton::Left),
        "Right" => Ok(MouseButton::Right),
        "Middle" => Ok(MouseButton::Middle),
        "Back" => Ok(MouseButton::Back),
        "Forward" => Ok(MouseButton::Forward),
        _ => text
            .parse::<u16>()
            .map(MouseButton::Other)
            .map_err(|_| format!("`{text}` does not name a mouse button")),
    }
}

fn parse_index(text: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .map_err(|_| format!("`{text}` is not a whole number"))
}

#[cfg(test)]
mod tests {
    use super::{
        GameInputControl, InputBudget, action_name, axis_name, count_action_refs, parse_action,
        parse_axis,
    };
    use spitfire_input::{
        InputAction, InputActionRef, InputAxisRef, InputContext, InputMapping, KeyCode,
        MouseButton, VirtualAction, VirtualAxis,
    };

    /// Drives one frame the way the game loop does. `steps` is how many fixed
    /// steps ran since the previous frame, and the running total is what
    /// `apply` actually reads.
    #[derive(Default)]
    struct Frames {
        total: u64,
    }

    impl Frames {
        fn run(&mut self, control: &mut GameInputControl, input: &InputContext, steps: u64) {
            self.total += steps;
            control.apply(input, self.total);
        }
    }

    #[test]
    fn test_action_and_axis_names_round_trip() {
        for action in [
            VirtualAction::KeyButton(KeyCode::Space),
            VirtualAction::MouseButton(MouseButton::Left),
            VirtualAction::MouseButton(MouseButton::Other(7)),
            VirtualAction::Axis(3),
            VirtualAction::Touch,
        ] {
            assert_eq!(parse_action(&action_name(&action)), Ok(action));
        }
        for axis in [
            VirtualAxis::KeyButton(KeyCode::Space),
            VirtualAxis::MousePositionX,
            VirtualAxis::MouseWheelY,
            VirtualAxis::TouchY,
            VirtualAxis::Axis(3),
        ] {
            assert_eq!(parse_axis(&axis_name(&axis)), Ok(axis));
        }
    }

    #[test]
    fn test_unknown_names_are_refused() {
        assert!(parse_action("key:NotAKey").is_err());
        assert!(parse_action("nonsense").is_err());
        assert!(parse_axis("mouse_position_z").is_err());
    }

    /// A press of one frame must read as pressed for exactly one frame, then as
    /// released for exactly one frame, then leave nothing behind.
    #[test]
    fn test_a_press_of_one_frame_presses_then_releases() {
        let mut input = InputContext::default();
        let action = InputActionRef::default();
        input.push_mapping(
            InputMapping::default()
                .name("test")
                .action(VirtualAction::KeyButton(KeyCode::Space), action.clone()),
        );
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.press(
            None,
            VirtualAction::KeyButton(KeyCode::Space),
            Some(InputBudget::Frames(1)),
        );

        frames.run(&mut control, &input, 0);
        assert_eq!(action.get(), InputAction::Pressed);
        assert_eq!(control.applied_last_frame(), 1);
        frames.run(&mut control, &input, 0);
        assert_eq!(action.get(), InputAction::Released);
        assert_eq!(control.pending_actions(), 0);
        action.set(InputAction::Idle);
        frames.run(&mut control, &input, 0);
        assert_eq!(action.get(), InputAction::Idle);
    }

    #[test]
    fn test_a_press_without_frames_holds_until_released() {
        let mut input = InputContext::default();
        let action = InputActionRef::default();
        input.push_mapping(
            InputMapping::default().action(VirtualAction::KeyButton(KeyCode::KeyA), action.clone()),
        );
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.press(None, VirtualAction::KeyButton(KeyCode::KeyA), None);

        frames.run(&mut control, &input, 0);
        assert_eq!(action.get(), InputAction::Pressed);
        for _ in 0..5 {
            frames.run(&mut control, &input, 0);
            assert_eq!(action.get(), InputAction::Hold);
        }
        control.release(None, VirtualAction::KeyButton(KeyCode::KeyA));
        frames.run(&mut control, &input, 0);
        assert_eq!(action.get(), InputAction::Released);
        assert_eq!(control.pending_actions(), 0);
    }

    #[test]
    fn test_an_axis_without_frames_is_written_once() {
        let mut input = InputContext::default();
        let axis = InputAxisRef::default();
        input.push_mapping(InputMapping::default().axis(VirtualAxis::MousePositionX, axis.clone()));
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.axis(None, VirtualAxis::MousePositionX, 640.0, None);

        frames.run(&mut control, &input, 0);
        assert_eq!(axis.get().0, 640.0);
        assert_eq!(control.pending_axes(), 0);
        frames.run(&mut control, &input, 0);
        assert_eq!(axis.get().0, 640.0);
    }

    /// An axis held for n frames must read as the value for n frames, and then
    /// fall back to zero on its own.
    #[test]
    fn test_an_axis_with_frames_falls_back_to_zero() {
        let mut input = InputContext::default();
        let axis = InputAxisRef::default();
        input.push_mapping(InputMapping::default().axis(VirtualAxis::MouseWheelY, axis.clone()));
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.axis(
            None,
            VirtualAxis::MouseWheelY,
            1.0,
            Some(InputBudget::Frames(3)),
        );

        for _ in 0..3 {
            frames.run(&mut control, &input, 0);
            assert_eq!(axis.get().0, 1.0);
        }
        frames.run(&mut control, &input, 0);
        assert_eq!(axis.get().0, 0.0);
        assert_eq!(control.pending_axes(), 0);
    }

    /// A step budget must keep the action pressed while no step runs, because
    /// game code that reads `is_pressed()` only sees the press inside a step.
    #[test]
    fn test_a_step_budget_waits_for_a_step() {
        let mut input = InputContext::default();
        let action = InputActionRef::default();
        input.push_mapping(
            InputMapping::default()
                .action(VirtualAction::KeyButton(KeyCode::Escape), action.clone()),
        );
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.press(
            None,
            VirtualAction::KeyButton(KeyCode::Escape),
            Some(InputBudget::Steps(1)),
        );

        for _ in 0..5 {
            frames.run(&mut control, &input, 0);
            assert_eq!(action.get(), InputAction::Pressed);
        }
        frames.run(&mut control, &input, 1);
        assert_eq!(action.get(), InputAction::Released);
        assert_eq!(control.pending_actions(), 0);
    }

    /// A step budget of n must read as pressed for the first step and as held
    /// for the rest, the way real hardware reads.
    #[test]
    fn test_a_step_budget_of_three_presses_once_then_holds() {
        let mut input = InputContext::default();
        let action = InputActionRef::default();
        input.push_mapping(
            InputMapping::default().action(VirtualAction::KeyButton(KeyCode::KeyD), action.clone()),
        );
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.press(
            None,
            VirtualAction::KeyButton(KeyCode::KeyD),
            Some(InputBudget::Steps(3)),
        );

        frames.run(&mut control, &input, 0);
        assert_eq!(action.get(), InputAction::Pressed);
        frames.run(&mut control, &input, 1);
        assert_eq!(action.get(), InputAction::Hold);
        frames.run(&mut control, &input, 1);
        assert_eq!(action.get(), InputAction::Hold);
        frames.run(&mut control, &input, 1);
        assert_eq!(action.get(), InputAction::Released);
        assert_eq!(control.pending_actions(), 0);
    }

    /// A frame that runs several steps must spend the whole budget at once,
    /// because `time.step` can grant many steps inside one frame.
    #[test]
    fn test_many_steps_in_one_frame_spend_the_budget_at_once() {
        let mut input = InputContext::default();
        let action = InputActionRef::default();
        input.push_mapping(
            InputMapping::default().action(VirtualAction::KeyButton(KeyCode::KeyD), action.clone()),
        );
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.press(
            None,
            VirtualAction::KeyButton(KeyCode::KeyD),
            Some(InputBudget::Steps(2)),
        );

        frames.run(&mut control, &input, 0);
        assert_eq!(action.get(), InputAction::Pressed);
        frames.run(&mut control, &input, 8);
        assert_eq!(action.get(), InputAction::Released);
        assert_eq!(control.pending_actions(), 0);
    }

    /// The editor skips the update phase while it is editing, so the fixed step
    /// count does not move for many frames in a row. A step budget must not
    /// count those frames, or a press would expire without any step running.
    #[test]
    fn test_frames_with_no_steps_do_not_spend_a_step_budget() {
        let mut input = InputContext::default();
        let action = InputActionRef::default();
        input.push_mapping(
            InputMapping::default().action(VirtualAction::KeyButton(KeyCode::KeyD), action.clone()),
        );
        let mut control = GameInputControl::default();
        control.press(
            None,
            VirtualAction::KeyButton(KeyCode::KeyD),
            Some(InputBudget::Steps(1)),
        );

        control.apply(&input, 7);
        assert_eq!(action.get(), InputAction::Pressed);
        for _ in 0..20 {
            control.apply(&input, 7);
            assert_eq!(action.get(), InputAction::Pressed);
        }
        control.apply(&input, 8);
        assert_eq!(action.get(), InputAction::Released);
    }

    #[test]
    fn test_an_axis_with_a_step_budget_falls_back_to_zero() {
        let mut input = InputContext::default();
        let axis = InputAxisRef::default();
        input.push_mapping(InputMapping::default().axis(VirtualAxis::MouseWheelY, axis.clone()));
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.axis(
            None,
            VirtualAxis::MouseWheelY,
            1.0,
            Some(InputBudget::Steps(2)),
        );

        for _ in 0..4 {
            frames.run(&mut control, &input, 0);
            assert_eq!(axis.get().0, 1.0);
        }
        frames.run(&mut control, &input, 1);
        assert_eq!(axis.get().0, 1.0);
        frames.run(&mut control, &input, 1);
        assert_eq!(axis.get().0, 1.0);
        frames.run(&mut control, &input, 0);
        assert_eq!(axis.get().0, 0.0);
        assert_eq!(control.pending_axes(), 0);
    }

    #[test]
    fn test_a_mapping_name_narrows_the_write() {
        let mut input = InputContext::default();
        let wanted = InputActionRef::default();
        let other = InputActionRef::default();
        input.push_mapping(
            InputMapping::default()
                .name("wanted")
                .action(VirtualAction::KeyButton(KeyCode::KeyA), wanted.clone()),
        );
        input.push_mapping(
            InputMapping::default()
                .name("other")
                .action(VirtualAction::KeyButton(KeyCode::KeyA), other.clone()),
        );
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.press(
            Some("wanted".to_owned()),
            VirtualAction::KeyButton(KeyCode::KeyA),
            Some(InputBudget::Frames(1)),
        );
        frames.run(&mut control, &input, 0);

        assert_eq!(wanted.get(), InputAction::Pressed);
        assert_eq!(other.get(), InputAction::Idle);
        assert_eq!(
            count_action_refs(&input, None, &VirtualAction::KeyButton(KeyCode::KeyA)),
            2
        );
        assert_eq!(
            count_action_refs(
                &input,
                Some("wanted"),
                &VirtualAction::KeyButton(KeyCode::KeyA)
            ),
            1
        );
        assert_eq!(
            count_action_refs(
                &input,
                Some("missing"),
                &VirtualAction::KeyButton(KeyCode::KeyA)
            ),
            0
        );
    }

    #[test]
    fn test_clear_releases_everything() {
        let mut input = InputContext::default();
        let action = InputActionRef::default();
        let axis = InputAxisRef::default();
        input.push_mapping(
            InputMapping::default()
                .action(VirtualAction::KeyButton(KeyCode::KeyA), action.clone())
                .axis(VirtualAxis::MouseWheelY, axis.clone()),
        );
        let mut control = GameInputControl::default();
        let mut frames = Frames::default();
        control.press(None, VirtualAction::KeyButton(KeyCode::KeyA), None);
        control.axis(
            None,
            VirtualAxis::MouseWheelY,
            1.0,
            Some(InputBudget::Frames(100)),
        );
        frames.run(&mut control, &input, 0);
        control.clear();
        frames.run(&mut control, &input, 0);

        assert_eq!(action.get(), InputAction::Released);
        assert_eq!(axis.get().0, 0.0);
        assert_eq!(control.pending_actions(), 0);
        assert_eq!(control.pending_axes(), 0);
    }
}
