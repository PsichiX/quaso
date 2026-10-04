use gilrs::{Axis, Button, GamepadId, Gilrs};
use spitfire_input::{InputActionOrAxisRef, InputAxis};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[derive(Debug, Clone)]
pub enum GamepadInputAxis {
    Single {
        input: InputActionOrAxisRef,
        deadzone: f32,
    },
    Double {
        negative: InputActionOrAxisRef,
        positive: InputActionOrAxisRef,
        deadzone: f32,
    },
}

impl GamepadInputAxis {
    pub fn single(input: impl Into<InputActionOrAxisRef>, deadzone: f32) -> Self {
        Self::Single {
            input: input.into(),
            deadzone,
        }
    }

    pub fn double(
        negative: impl Into<InputActionOrAxisRef>,
        positive: impl Into<InputActionOrAxisRef>,
        deadzone: f32,
    ) -> Self {
        Self::Double {
            negative: negative.into(),
            positive: positive.into(),
            deadzone,
        }
    }
}

#[derive(Clone)]
pub struct GamepadInput {
    instance: Rc<RefCell<Gilrs>>,
    used_gamepads: Rc<RefCell<HashSet<GamepadId>>>,
    id: Option<GamepadId>,
    pub buttons: HashMap<Button, InputActionOrAxisRef>,
    pub axes: HashMap<Axis, GamepadInputAxis>,
    pub auto_acquire: bool,
}

impl Drop for GamepadInput {
    fn drop(&mut self) {
        self.release();
    }
}

impl GamepadInput {
    pub fn acquire(&mut self) -> bool {
        self.release();
        let mut used_gamepads = self.used_gamepads.borrow_mut();
        for (id, _) in self.instance.borrow().gamepads() {
            if !used_gamepads.contains(&id) {
                used_gamepads.insert(id);
                self.id = Some(id);
                return true;
            }
        }
        false
    }

    pub fn release(&mut self) {
        if let Some(id) = self.id.as_ref() {
            self.used_gamepads.borrow_mut().remove(id);
        }
        self.id = None;
    }

    pub fn id(&self) -> Option<GamepadId> {
        self.id
    }

    pub fn is_connected(&self) -> bool {
        self.id.is_some()
    }

    pub fn button(mut self, id: Button, input: impl Into<InputActionOrAxisRef>) -> Self {
        self.buttons.insert(id, input.into());
        self
    }

    pub fn axis(mut self, id: Axis, input: impl Into<GamepadInputAxis>) -> Self {
        self.axes.insert(id, input.into());
        self
    }

    pub fn auto_acquire(mut self) -> Self {
        self.auto_acquire = true;
        self
    }

    pub fn apply(&mut self) {
        if self.auto_acquire && !self.is_connected() {
            self.acquire();
        }
        if let Some(id) = self.id {
            let instance = self.instance.borrow();
            let mut used_gamepads = self.used_gamepads.borrow_mut();
            if let Some(gamepad) = instance.connected_gamepad(id) {
                for (id, input) in &mut self.buttons {
                    if let Some(data) = gamepad.button_data(*id) {
                        match input {
                            InputActionOrAxisRef::Action(input) => {
                                input.set(input.get().change(data.is_pressed()));
                            }
                            InputActionOrAxisRef::Axis(input) => {
                                input.set(InputAxis(data.value()));
                            }
                            _ => {}
                        }
                    }
                }
                for (id, input) in &mut self.axes {
                    if let Some(data) = gamepad.axis_data(*id) {
                        match input {
                            GamepadInputAxis::Single { input, deadzone } => {
                                let mut value = data.value().abs();
                                if value < *deadzone {
                                    value = 0.0;
                                }
                                match input {
                                    InputActionOrAxisRef::Action(input) => {
                                        input.set(input.get().change(value > 0.5));
                                    }
                                    InputActionOrAxisRef::Axis(input) => {
                                        input.set(InputAxis(value));
                                    }
                                    _ => {}
                                }
                            }
                            GamepadInputAxis::Double {
                                negative,
                                positive,
                                deadzone,
                            } => {
                                let mut value = data.value();
                                if value.abs() < *deadzone {
                                    value = 0.0;
                                }
                                match positive {
                                    InputActionOrAxisRef::Action(input) => {
                                        input.set(input.get().change(value > 0.5));
                                    }
                                    InputActionOrAxisRef::Axis(input) => {
                                        input.set(InputAxis(value.max(0.0)));
                                    }
                                    _ => {}
                                }
                                match negative {
                                    InputActionOrAxisRef::Action(input) => {
                                        input.set(input.get().change(value < -0.5));
                                    }
                                    InputActionOrAxisRef::Axis(input) => {
                                        input.set(InputAxis(-value.min(0.0)));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            } else {
                used_gamepads.remove(&id);
                self.id = None;
            }
        }
    }
}

pub struct GamepadManager {
    instance: Option<Rc<RefCell<Gilrs>>>,
    used_gamepads: Rc<RefCell<HashSet<GamepadId>>>,
}

impl Default for GamepadManager {
    fn default() -> Self {
        Self {
            instance: Gilrs::new()
                .ok()
                .map(|instance| Rc::new(RefCell::new(instance))),
            used_gamepads: Default::default(),
        }
    }
}

impl GamepadManager {
    pub fn is_supported(&self) -> bool {
        self.instance.is_some()
    }

    pub fn request_gamepad(&self) -> Option<GamepadInput> {
        Some(GamepadInput {
            instance: self.instance.as_ref()?.clone(),
            used_gamepads: self.used_gamepads.clone(),
            id: Default::default(),
            buttons: Default::default(),
            axes: Default::default(),
            auto_acquire: Default::default(),
        })
    }

    pub fn maintain(&mut self) {
        if let Some(instance) = self.instance.as_mut() {
            while instance.borrow_mut().next_event().is_some() {}
        }
    }
}
