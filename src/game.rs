use crate::{
    assets::{
        font::FontAssetSubsystem, shader::ShaderAssetSubsystem, sound::SoundAssetSubsystem,
        texture::TextureAssetSubsystem,
    },
    audio::Audio,
    context::GameContext,
};
#[cfg(not(target_arch = "wasm32"))]
use glutin::{
    event::{Event, WindowEvent},
    window::Window,
};
#[cfg(target_arch = "wasm32")]
use instant::Instant;
use keket::database::AssetDatabase;
use spitfire_draw::{
    context::DrawContext,
    utils::{ShaderRef, Vertex},
};
use spitfire_glow::{app::AppState, graphics::Graphics, renderer::GlowBlending};
use spitfire_gui::context::GuiContext;
use spitfire_input::InputContext;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use std::{
    any::{Any, TypeId},
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
};
#[cfg(target_arch = "wasm32")]
use winit::{
    event::{Event, WindowEvent},
    window::Window,
};

pub trait GameObject {
    #[allow(unused_variables)]
    fn activate(&mut self, context: &mut GameContext) {}

    #[allow(unused_variables)]
    fn deactivate(&mut self, context: &mut GameContext) {}

    #[allow(unused_variables)]
    fn process(&mut self, context: &mut GameContext, delta_time: f32) {}

    #[allow(unused_variables)]
    fn draw(&mut self, context: &mut GameContext) {}
}

#[derive(Default)]
pub enum GameStateChange {
    #[default]
    Continue,
    Swap(Box<dyn GameState>),
    Push(Box<dyn GameState>),
    Pop,
}

#[allow(unused_variables)]
pub trait GameState {
    fn enter(&mut self, context: GameContext) {}

    fn exit(&mut self, context: GameContext) {}

    fn update(&mut self, context: GameContext, delta_time: f32) {}

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {}

    fn draw(&mut self, context: GameContext) {}

    fn draw_gui(&mut self, context: GameContext) {}

    fn event(&mut self, globals: &mut GameGlobals, event: &Event<()>) {}
}

pub trait GameSubsystem {
    fn run(&mut self, context: GameContext, delta_time: f32);
}

#[derive(Default)]
pub struct GameGlobals {
    globals: HashMap<TypeId, RefCell<Box<dyn Any>>>,
}

impl GameGlobals {
    pub fn set<T: 'static>(&mut self, value: T) {
        self.globals
            .insert(TypeId::of::<T>(), RefCell::new(Box::new(value)));
    }

    pub fn unset<T: 'static>(&mut self) {
        self.globals.remove(&TypeId::of::<T>());
    }

    pub fn read<T: 'static>(&self) -> Option<Ref<T>> {
        self.globals
            .get(&TypeId::of::<T>())
            .and_then(|v| v.try_borrow().ok())
            .map(|v| Ref::map(v, |v| v.downcast_ref::<T>().unwrap()))
    }

    pub fn write<T: 'static>(&self) -> Option<RefMut<T>> {
        self.globals
            .get(&TypeId::of::<T>())
            .and_then(|v| v.try_borrow_mut().ok())
            .map(|v| RefMut::map(v, |v| v.downcast_mut::<T>().unwrap()))
    }
}

pub struct GameInstance {
    pub fixed_delta_time: f32,
    pub unfocused_fixed_delta_time_scale: f32,
    pub color_shader: &'static str,
    pub image_shader: &'static str,
    pub text_shader: &'static str,
    pub input_maintain_on_fixed_step: bool,
    draw: DrawContext,
    gui: GuiContext,
    input: InputContext,
    assets: AssetDatabase,
    audio: Audio,
    timer: Instant,
    fixed_timer: Instant,
    states: Vec<Box<dyn GameState>>,
    state_change: GameStateChange,
    subsystems: Vec<Box<dyn GameSubsystem>>,
    globals: GameGlobals,
    focused: bool,
}

impl Default for GameInstance {
    fn default() -> Self {
        Self {
            fixed_delta_time: 1.0 / 60.0,
            unfocused_fixed_delta_time_scale: 1.0,
            color_shader: "color",
            image_shader: "image",
            text_shader: "text",
            input_maintain_on_fixed_step: true,
            draw: Default::default(),
            gui: Default::default(),
            input: Default::default(),
            assets: Default::default(),
            audio: Default::default(),
            timer: Instant::now(),
            fixed_timer: Instant::now(),
            states: Default::default(),
            state_change: Default::default(),
            subsystems: vec![
                Box::new(ShaderAssetSubsystem),
                Box::new(TextureAssetSubsystem),
                Box::new(FontAssetSubsystem),
                Box::new(SoundAssetSubsystem),
            ],
            globals: Default::default(),
            focused: true,
        }
    }
}

impl GameInstance {
    pub fn new(state: impl GameState + 'static) -> Self {
        Self {
            state_change: GameStateChange::Push(Box::new(state)),
            ..Default::default()
        }
    }

    pub fn with_fixed_time_step(mut self, value: f32) -> Self {
        self.fixed_delta_time = value;
        self
    }

    pub fn with_fps(mut self, frames_per_second: usize) -> Self {
        self.set_fps(frames_per_second);
        self
    }

    pub fn with_unfocused_fixed_time_step_scale(mut self, value: f32) -> Self {
        self.unfocused_fixed_delta_time_scale = value;
        self
    }

    pub fn with_color_shader(mut self, name: &'static str) -> Self {
        self.color_shader = name;
        self
    }

    pub fn with_image_shader(mut self, name: &'static str) -> Self {
        self.image_shader = name;
        self
    }

    pub fn with_text_shader(mut self, name: &'static str) -> Self {
        self.text_shader = name;
        self
    }

    pub fn with_input_maintain_on_fixed_step(mut self, value: bool) -> Self {
        self.input_maintain_on_fixed_step = value;
        self
    }

    pub fn with_subsystem(mut self, subsystem: impl GameSubsystem + 'static) -> Self {
        self.subsystems.push(Box::new(subsystem));
        self
    }

    pub fn with_globals<T: 'static>(mut self, value: T) -> Self {
        self.globals.set(value);
        self
    }

    pub fn setup_assets(mut self, f: impl FnOnce(&mut AssetDatabase)) -> Self {
        f(&mut self.assets);
        self
    }

    pub fn fps(&self) -> usize {
        (1.0 / self.fixed_delta_time).ceil() as usize
    }

    pub fn set_fps(&mut self, frames_per_second: usize) {
        self.fixed_delta_time = 1.0 / frames_per_second as f32;
    }

    pub fn process_frame(&mut self, graphics: &mut Graphics<Vertex>) {
        let delta_time = self.timer.elapsed().as_secs_f32();

        for subsystem in &mut self.subsystems {
            subsystem.run(
                GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                },
                delta_time,
            );
        }
        self.assets.maintain().unwrap();

        if let Some(state) = self.states.last_mut() {
            self.timer = Instant::now();
            state.update(
                GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                },
                delta_time,
            );
        }

        let fixed_delta_time = self.fixed_timer.elapsed().as_secs_f32();
        let fixed_delta_time_limit = if self.focused {
            self.fixed_delta_time
        } else {
            self.fixed_delta_time * self.unfocused_fixed_delta_time_scale
        };
        let fixed_step = if fixed_delta_time > fixed_delta_time_limit {
            self.fixed_timer = Instant::now();
            if let Some(state) = self.states.last_mut() {
                state.fixed_update(
                    GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                    },
                    fixed_delta_time,
                );
            }
            true
        } else {
            false
        };

        self.draw.begin_frame(graphics);
        self.draw.push_shader(&ShaderRef::name(self.image_shader));
        self.draw.push_blending(GlowBlending::Alpha);
        if let Some(state) = self.states.last_mut() {
            state.draw(GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
            });
        }
        self.gui.begin_frame();
        if let Some(state) = self.states.last_mut() {
            state.draw_gui(GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
            });
        }
        self.gui.end_frame(
            &mut self.draw,
            graphics,
            &ShaderRef::name(self.color_shader),
            &ShaderRef::name(self.image_shader),
            &ShaderRef::name(self.text_shader),
        );
        self.draw.end_frame();
        if !self.input_maintain_on_fixed_step || fixed_step {
            self.input.maintain();
        }

        match std::mem::take(&mut self.state_change) {
            GameStateChange::Continue => {}
            GameStateChange::Swap(mut state) => {
                if let Some(mut state) = self.states.pop() {
                    state.exit(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                    });
                }
                state.enter(GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                });
                self.states.push(state);
                self.timer = Instant::now();
            }
            GameStateChange::Push(mut state) => {
                state.enter(GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                });
                self.states.push(state);
                self.timer = Instant::now();
            }
            GameStateChange::Pop => {
                if let Some(mut state) = self.states.pop() {
                    state.exit(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                    });
                }
                self.timer = Instant::now();
            }
        }
    }

    pub fn process_event(&mut self, event: &Event<()>) -> bool {
        if let Event::WindowEvent { event, .. } = event {
            if let WindowEvent::Focused(focused) = &event {
                self.focused = *focused;
            }
            self.input.on_event(event);
        }
        if let Some(state) = self.states.last_mut() {
            state.event(&mut self.globals, event);
        }
        !self.states.is_empty() || !matches!(self.state_change, GameStateChange::Continue)
    }
}

impl AppState<Vertex> for GameInstance {
    fn on_redraw(&mut self, graphics: &mut Graphics<Vertex>) {
        self.process_frame(graphics);
    }

    fn on_event(&mut self, event: Event<()>, _: &mut Window) -> bool {
        self.process_event(&event)
    }
}
