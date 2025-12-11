#[cfg(feature = "editor")]
use crate::editor::EditorInput;
use crate::{
    assets::{
        anim_texture::AnimTextureAssetSubsystem, font::FontAssetSubsystem,
        gltf::GltfAssetSubsystem, shader::ShaderAssetSubsystem, sound::SoundAssetSubsystem,
        texture::TextureAssetSubsystem,
    },
    audio::Audio,
    context::{GameContext, GameSubsystems},
    third_party::{
        Duration, Instant,
        windowing::{
            event::{Event, WindowEvent},
            window::Window,
        },
    },
    value::{DynPtr, DynVal, Ptr, Val},
};
use anput::{scheduler::GraphScheduler, universe::Universe};
use gilrs::Gilrs;
use intuicio_data::managed::DynamicManagedLazy;
use keket::database::AssetDatabase;
use moirai::jobs::{JobHandle, JobLocation, JobOptions, JobQueue, Jobs};
use nodio::graph::Graph;
use spitfire_draw::{
    context::DrawContext,
    utils::{ShaderRef, Vertex},
};
use spitfire_glow::{
    app::{AppControl, AppState},
    graphics::Graphics,
    renderer::GlowBlending,
};
use spitfire_gui::context::GuiContext;
use spitfire_input::InputContext;
use std::{
    any::{Any, TypeId},
    cell::LazyCell,
    collections::BTreeMap,
    pin::Pin,
};
#[cfg(feature = "editor")]
use vek::{Rect, Vec2};

pub(crate) const CONTEXT_META: &str = "game_context";
pub(crate) const DELTA_TIME_META: &str = "delta_time";
pub(crate) const NEXT_FRAME_QUEUE_META: &str = "game_next_frame_queue";

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

impl GameStateChange {
    pub fn is_change(&self) -> bool {
        !matches!(self, GameStateChange::Continue)
    }
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

    fn timeline(
        &mut self,
        context: GameContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
        Box::pin(async {})
    }
}

pub trait GameSubsystem {
    #[allow(unused_variables)]
    fn update(&mut self, context: GameContext, delta_time: f32) {}

    #[allow(unused_variables)]
    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {}

    #[allow(unused_variables)]
    fn draw(&mut self, context: GameContext) {}

    #[allow(unused_variables)]
    fn draw_gui(&mut self, context: GameContext) {}

    #[allow(unused_variables)]
    fn event(&mut self, globals: &mut GameGlobals, event: &Event<()>) {}

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[cfg(feature = "editor")]
#[derive(Default)]
pub struct EditorGlobals {
    pub(crate) is_editing: bool,
    pub(crate) viewport_rectangle: Rect<f32, f32>,
    pub(crate) input: EditorInput,
}

#[cfg(feature = "editor")]
impl EditorGlobals {
    pub fn is_editing(&self) -> bool {
        self.is_editing
    }

    pub fn viewport_rectangle(&self) -> Rect<f32, f32> {
        self.viewport_rectangle
    }

    pub fn window_screen_to_viewport_screen(
        &self,
        graphics: &Graphics<Vertex>,
        point: Vec2<f32>,
    ) -> Vec2<f32> {
        let view = (point - self.viewport_rectangle.position()) / self.viewport_rectangle.extent();
        graphics.state.main_camera.screen_size * view
    }

    pub fn input(&self) -> &EditorInput {
        &self.input
    }
}

pub struct GameGlobals {
    globals: BTreeMap<TypeId, DynVal>,
    is_touch_device: LazyCell<bool>,
    #[cfg(feature = "editor")]
    pub editor: EditorGlobals,
}

impl Default for GameGlobals {
    fn default() -> Self {
        Self {
            globals: Default::default(),
            is_touch_device: LazyCell::new(|| {
                #[cfg(target_arch = "wasm32")]
                {
                    use wasm_bindgen::prelude::*;
                    use web_sys::window;

                    let Some(window) = window() else {
                        return false;
                    };
                    if js_sys::Reflect::has(&window, &JsValue::from_str("ontouchstart"))
                        .unwrap_or(false)
                    {
                        return true;
                    }
                    if window.navigator().max_touch_points() > 0 {
                        return true;
                    }
                    false
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    cfg!(target_os = "android") || cfg!(target_os = "ios")
                }
            }),
            #[cfg(feature = "editor")]
            editor: Default::default(),
        }
    }
}

impl GameGlobals {
    pub fn set<T: 'static>(&mut self, value: T) {
        self.globals.insert(TypeId::of::<T>(), DynVal::new(value));
    }

    pub fn unset<T: 'static>(&mut self) {
        self.globals.remove(&TypeId::of::<T>());
    }

    pub fn access<T: 'static>(&'_ self) -> Option<Ptr<T>> {
        self.globals
            .get(&TypeId::of::<T>())
            .map(|v| v.pointer().into_typed())
    }

    pub fn ensure<T: Default + 'static>(&mut self) -> Ptr<T> {
        self.globals
            .entry(TypeId::of::<T>())
            .or_insert_with(|| DynVal::new(T::default()))
            .pointer()
            .into_typed()
    }

    pub fn is_touch_device(&self) -> bool {
        *self.is_touch_device
    }
}

#[derive(Default)]
pub struct GameJobs {
    jobs: Jobs,
}

impl GameJobs {
    pub fn coroutine<T: Send>(
        &self,
        job: impl Future<Output = T> + Send + Sync + 'static,
    ) -> JobHandle<T> {
        self.spawn(JobLocation::Local, job)
    }

    pub fn coroutine_with_meta<T: Send>(
        &self,
        meta: impl IntoIterator<Item = (String, DynPtr)>,
        job: impl Future<Output = T> + Send + Sync + 'static,
    ) -> JobHandle<T> {
        self.spawn(
            JobOptions::default()
                .location(JobLocation::Local)
                .meta_many(meta.into_iter().map(|(id, ptr)| (id, ptr.into_inner()))),
            job,
        )
    }

    pub fn spawn<T: Send>(
        &self,
        options: impl Into<JobOptions>,
        job: impl Future<Output = T> + Send + Sync + 'static,
    ) -> JobHandle<T> {
        self.jobs.spawn(options, job).unwrap()
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
    total_timer: Instant,
    frame: usize,
    #[allow(clippy::type_complexity)]
    states: Vec<(Box<dyn GameState>, JobHandle<()>, Val<()>)>,
    state_change: GameStateChange,
    subsystems: Vec<Box<dyn GameSubsystem>>,
    globals: GameGlobals,
    jobs: GameJobs,
    next_frame_queue: JobQueue,
    update_queue: JobQueue,
    next_update_queue: JobQueue,
    fixed_update_queue: JobQueue,
    next_fixed_update_queue: JobQueue,
    draw_queue: JobQueue,
    next_draw_queue: JobQueue,
    draw_gui_queue: JobQueue,
    next_draw_gui_queue: JobQueue,
    universe: Universe,
    graph: Graph,
    focused: bool,
    #[cfg(feature = "editor")]
    editor: crate::editor::Editor,
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
            total_timer: Instant::now(),
            frame: 0,
            states: Default::default(),
            state_change: Default::default(),
            subsystems: vec![
                Box::new(ShaderAssetSubsystem),
                Box::new(TextureAssetSubsystem),
                Box::new(AnimTextureAssetSubsystem),
                Box::new(FontAssetSubsystem),
                Box::new(SoundAssetSubsystem),
                Box::new(GltfAssetSubsystem),
            ],
            globals: Default::default(),
            jobs: Default::default(),
            next_frame_queue: Default::default(),
            update_queue: Default::default(),
            next_update_queue: Default::default(),
            fixed_update_queue: Default::default(),
            next_fixed_update_queue: Default::default(),
            draw_queue: Default::default(),
            next_draw_queue: Default::default(),
            draw_gui_queue: Default::default(),
            next_draw_gui_queue: Default::default(),
            universe: Default::default(),
            graph: Default::default(),
            focused: true,
            #[cfg(feature = "editor")]
            editor: Default::default(),
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

    #[cfg(feature = "editor")]
    pub fn with_editor(mut self, editor: crate::editor::Editor) -> Self {
        self.editor = editor;
        self
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

    pub fn with_jobs(mut self, jobs: Jobs) -> Self {
        self.jobs.jobs = jobs;
        self
    }

    pub fn with_jobs_unnamed_worker(mut self) -> Self {
        self.jobs.jobs.add_unnamed_worker();
        self
    }

    pub fn with_jobs_named_worker(mut self, name: impl ToString) -> Self {
        self.jobs.jobs.add_named_worker(name);
        self
    }

    pub fn with_gamepads(mut self) -> Self {
        self.input = self.input.with_gamepads();
        self
    }

    pub fn with_gamepads_custom(mut self, gamepads: Gilrs) -> Self {
        self.input = self.input.with_gamepads_custom(gamepads);
        self
    }

    pub fn setup_assets(mut self, f: impl FnOnce(&mut AssetDatabase)) -> Self {
        f(&mut self.assets);
        self
    }

    pub fn setup(self, f: impl FnOnce(Self) -> Self) -> Self {
        f(self)
    }

    pub fn fps(&self) -> usize {
        (1.0 / self.fixed_delta_time).ceil() as usize
    }

    pub fn set_fps(&mut self, frames_per_second: usize) {
        self.fixed_delta_time = 1.0 / frames_per_second as f32;
    }

    pub fn process_frame(&mut self, graphics: &mut Graphics<Vertex>) {
        let total_time = self.total_timer.elapsed().as_secs_f32();

        loop {
            match std::mem::take(&mut self.state_change) {
                GameStateChange::Continue => {}
                GameStateChange::Swap(mut state) => {
                    if let Some((mut state, job, state_value)) = self.states.pop() {
                        job.cancel();
                        let state_heartbeat = state_value.heartbeat();
                        state.exit(GameContext {
                            graphics,
                            draw: &mut self.draw,
                            gui: &mut self.gui,
                            input: &mut self.input,
                            state_change: &mut self.state_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            jobs: Some(&self.jobs),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
                            universe: &mut self.universe,
                            graph: &mut self.graph,
                            state_heartbeat: &state_heartbeat,
                            subsystems: GameSubsystems {
                                subsystems: &mut self.subsystems,
                            },
                            time: total_time,
                            frame: self.frame,
                        });
                        if self.state_change.is_change() {
                            continue;
                        }
                    }
                    let state_value = Val::new(());
                    let state_heartbeat = state_value.heartbeat();
                    state.enter(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        jobs: Some(&self.jobs),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    });
                    if self.state_change.is_change() {
                        continue;
                    }
                    let future = state.timeline(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        jobs: Some(&self.jobs),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    });
                    if self.state_change.is_change() {
                        continue;
                    }
                    let job = self.jobs.coroutine(future);
                    self.states.push((state, job, state_value));
                    self.timer = Instant::now();
                }
                GameStateChange::Push(mut state) => {
                    let state_value = Val::new(());
                    let state_heartbeat = state_value.heartbeat();
                    state.enter(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        jobs: Some(&self.jobs),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    });
                    if self.state_change.is_change() {
                        continue;
                    }
                    let future = state.timeline(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        jobs: Some(&self.jobs),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    });
                    if self.state_change.is_change() {
                        continue;
                    }
                    let job = self.jobs.coroutine(future);
                    self.states.push((state, job, state_value));
                    self.timer = Instant::now();
                }
                GameStateChange::Pop => {
                    if let Some((mut state, job, state_value)) = self.states.pop() {
                        let state_heartbeat = state_value.heartbeat();
                        job.cancel();
                        state.exit(GameContext {
                            graphics,
                            draw: &mut self.draw,
                            gui: &mut self.gui,
                            input: &mut self.input,
                            state_change: &mut self.state_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            jobs: Some(&self.jobs),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
                            universe: &mut self.universe,
                            graph: &mut self.graph,
                            state_heartbeat: &state_heartbeat,
                            subsystems: GameSubsystems {
                                subsystems: &mut self.subsystems,
                            },
                            time: total_time,
                            frame: self.frame,
                        });
                    }
                    if self.state_change.is_change() {
                        continue;
                    }
                    self.timer = Instant::now();
                }
            }
            break;
        }

        self.frame += 1;
        #[cfg(feature = "editor")]
        let is_editing = self.globals.editor.is_editing();
        let mut delta_time = self.timer.elapsed().as_secs_f32();
        let jobs_timer = self.timer;
        self.timer = Instant::now();
        let frame_budget = Duration::from_secs_f32(self.fixed_delta_time);
        let Some(state_value) = self.states.last().map(|(_, _, value)| value) else {
            return;
        };
        let state_heartbeat = state_value.heartbeat();

        let mut update_phase = || {
            if let Some((state, _, _)) = self.states.last_mut() {
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
                        jobs: Some(&self.jobs),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    },
                    delta_time,
                );
            }
            for subsystem in &mut self.subsystems {
                subsystem.update(
                    GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        jobs: Some(&self.jobs),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut [],
                        },
                        time: total_time,
                        frame: self.frame,
                    },
                    delta_time,
                );
            }
            self.update_queue.append(&self.next_update_queue);
            while !self.update_queue.is_empty() {
                let mut async_context = GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                    jobs: None,
                    update_queue: &self.next_update_queue,
                    fixed_update_queue: &self.next_fixed_update_queue,
                    draw_queue: &self.next_draw_queue,
                    draw_gui_queue: &self.next_draw_gui_queue,
                    universe: &mut self.universe,
                    graph: &mut self.graph,
                    state_heartbeat: &state_heartbeat,
                    subsystems: GameSubsystems {
                        subsystems: &mut self.subsystems,
                    },
                    time: total_time,
                    frame: self.frame,
                };
                let (async_context_lazy, _async_context_lifetime) =
                    DynamicManagedLazy::make(&mut async_context);
                let (delta_time_lazy, _delta_time_lifetime) =
                    DynamicManagedLazy::make(&mut delta_time);
                let (next_frame_queue_lazy, _next_frame_queue_lifetime) =
                    DynamicManagedLazy::make(&mut self.next_update_queue);
                self.jobs.jobs.run_queue_with_meta(
                    &self.update_queue,
                    [
                        (CONTEXT_META.to_owned(), async_context_lazy),
                        (DELTA_TIME_META.to_owned(), delta_time_lazy),
                        (NEXT_FRAME_QUEUE_META.to_owned(), next_frame_queue_lazy),
                    ],
                );
            }
            #[cfg(feature = "editor")]
            for subsystem in &mut self.editor.subsystems {
                subsystem.update(
                    GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        jobs: Some(&self.jobs),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    },
                    delta_time,
                );
            }

            let mut fixed_delta_time = self.fixed_timer.elapsed().as_secs_f32();
            let fixed_delta_time_limit = if self.focused {
                self.fixed_delta_time
            } else {
                self.fixed_delta_time * self.unfocused_fixed_delta_time_scale
            };

            if fixed_delta_time > fixed_delta_time_limit {
                self.fixed_timer = Instant::now();
                if let Some((state, _, _)) = self.states.last_mut() {
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
                            jobs: Some(&self.jobs),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
                            universe: &mut self.universe,
                            graph: &mut self.graph,
                            state_heartbeat: &state_heartbeat,
                            subsystems: GameSubsystems {
                                subsystems: &mut self.subsystems,
                            },
                            time: total_time,
                            frame: self.frame,
                        },
                        fixed_delta_time,
                    );
                }
                for subsystem in &mut self.subsystems {
                    subsystem.fixed_update(
                        GameContext {
                            graphics,
                            draw: &mut self.draw,
                            gui: &mut self.gui,
                            input: &mut self.input,
                            state_change: &mut self.state_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            jobs: Some(&self.jobs),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
                            universe: &mut self.universe,
                            graph: &mut self.graph,
                            state_heartbeat: &state_heartbeat,
                            subsystems: GameSubsystems {
                                subsystems: &mut [],
                            },
                            time: total_time,
                            frame: self.frame,
                        },
                        fixed_delta_time,
                    );
                }
                self.fixed_update_queue
                    .append(&self.next_fixed_update_queue);
                while !self.fixed_update_queue.is_empty() {
                    let mut async_context = GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        jobs: None,
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        universe: &mut self.universe,
                        graph: &mut self.graph,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    };
                    let (async_context_lazy, _async_context_lifetime) =
                        DynamicManagedLazy::make(&mut async_context);
                    let (delta_time_lazy, _delta_time_lifetime) =
                        DynamicManagedLazy::make(&mut fixed_delta_time);
                    let (next_frame_queue_lazy, _next_frame_queue_lifetime) =
                        DynamicManagedLazy::make(&mut self.next_fixed_update_queue);
                    self.jobs.jobs.run_queue_with_meta(
                        &self.fixed_update_queue,
                        [
                            (CONTEXT_META.to_owned(), async_context_lazy),
                            (DELTA_TIME_META.to_owned(), delta_time_lazy),
                            (NEXT_FRAME_QUEUE_META.to_owned(), next_frame_queue_lazy),
                        ],
                    );
                }
                #[cfg(feature = "editor")]
                for subsystem in &mut self.editor.subsystems {
                    subsystem.fixed_update(
                        GameContext {
                            graphics,
                            draw: &mut self.draw,
                            gui: &mut self.gui,
                            input: &mut self.input,
                            state_change: &mut self.state_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            jobs: Some(&self.jobs),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
                            universe: &mut self.universe,
                            graph: &mut self.graph,
                            state_heartbeat: &state_heartbeat,
                            subsystems: GameSubsystems {
                                subsystems: &mut self.subsystems,
                            },
                            time: total_time,
                            frame: self.frame,
                        },
                        fixed_delta_time,
                    );
                }
                true
            } else {
                false
            }
        };
        #[cfg(feature = "editor")]
        let fixed_step = if is_editing { false } else { update_phase() };
        #[cfg(not(feature = "editor"))]
        let fixed_step = update_phase();
        self.assets.maintain().unwrap();

        self.draw.begin_frame(graphics);
        #[cfg(feature = "editor")]
        self.editor.begin_frame_capture(graphics, &mut self.draw);
        self.draw.push_shader(&ShaderRef::name(self.image_shader));
        self.draw.push_blending(GlowBlending::Alpha);
        if let Some((state, _, _)) = self.states.last_mut() {
            state.draw(GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: Some(&self.jobs),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: total_time,
                frame: self.frame,
            });
        }
        for subsystem in &mut self.subsystems {
            subsystem.draw(GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: Some(&self.jobs),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut [],
                },
                time: total_time,
                frame: self.frame,
            });
        }
        self.draw_queue.append(&self.next_draw_queue);
        while !self.draw_queue.is_empty() {
            let mut async_context = GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: None,
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: total_time,
                frame: self.frame,
            };
            let (async_context_lazy, _async_context_lifetime) =
                DynamicManagedLazy::make(&mut async_context);
            let (delta_time_lazy, _delta_time_lifetime) = DynamicManagedLazy::make(&mut delta_time);
            let (next_frame_queue_lazy, _next_frame_queue_lifetime) =
                DynamicManagedLazy::make(&mut self.next_draw_queue);
            self.jobs.jobs.run_queue_with_meta(
                &self.draw_queue,
                [
                    (CONTEXT_META.to_owned(), async_context_lazy),
                    (DELTA_TIME_META.to_owned(), delta_time_lazy),
                    (NEXT_FRAME_QUEUE_META.to_owned(), next_frame_queue_lazy),
                ],
            );
        }
        #[cfg(feature = "editor")]
        {
            for subsystem in &mut self.editor.subsystems {
                subsystem.draw(GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                    jobs: Some(&self.jobs),
                    update_queue: &self.next_update_queue,
                    fixed_update_queue: &self.next_fixed_update_queue,
                    draw_queue: &self.next_draw_queue,
                    draw_gui_queue: &self.next_draw_gui_queue,
                    universe: &mut self.universe,
                    graph: &mut self.graph,
                    state_heartbeat: &state_heartbeat,
                    subsystems: GameSubsystems {
                        subsystems: &mut self.subsystems,
                    },
                    time: total_time,
                    frame: self.frame,
                });
            }
            self.editor.end_frame_capture(graphics, &mut self.draw);
            self.draw.push_shader(&ShaderRef::name(self.image_shader));
            self.draw.push_blending(GlowBlending::Alpha);
        }
        self.gui.begin_frame();
        #[cfg(feature = "editor")]
        self.editor.begin_gui_capture();
        if let Some((state, _, _)) = self.states.last_mut() {
            state.draw_gui(GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: Some(&self.jobs),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: total_time,
                frame: self.frame,
            });
        }
        for subsystem in &mut self.subsystems {
            subsystem.draw_gui(GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: Some(&self.jobs),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut [],
                },
                time: total_time,
                frame: self.frame,
            });
        }
        self.draw_gui_queue.append(&self.next_draw_gui_queue);
        while !self.draw_gui_queue.is_empty() {
            let mut async_context = GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: None,
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: total_time,
                frame: self.frame,
            };
            let (async_context_lazy, _async_context_lifetime) =
                DynamicManagedLazy::make(&mut async_context);
            let (delta_time_lazy, _delta_time_lifetime) = DynamicManagedLazy::make(&mut delta_time);
            let (next_frame_queue_lazy, _next_frame_queue_lifetime) =
                DynamicManagedLazy::make(&mut self.next_draw_gui_queue);
            self.jobs.jobs.run_queue_with_meta(
                &self.draw_gui_queue,
                [
                    (CONTEXT_META.to_owned(), async_context_lazy),
                    (DELTA_TIME_META.to_owned(), delta_time_lazy),
                    (NEXT_FRAME_QUEUE_META.to_owned(), next_frame_queue_lazy),
                ],
            );
        }
        #[cfg(feature = "editor")]
        {
            for subsystem in &mut self.editor.subsystems {
                subsystem.draw_gui(GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                    jobs: Some(&self.jobs),
                    update_queue: &self.next_update_queue,
                    fixed_update_queue: &self.next_fixed_update_queue,
                    draw_queue: &self.next_draw_queue,
                    draw_gui_queue: &self.next_draw_gui_queue,
                    universe: &mut self.universe,
                    graph: &mut self.graph,
                    state_heartbeat: &state_heartbeat,
                    subsystems: GameSubsystems {
                        subsystems: &mut self.subsystems,
                    },
                    time: total_time,
                    frame: self.frame,
                });
            }
            self.editor.end_gui_capture();
            self.editor.draw_gui(GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: Some(&self.jobs),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: total_time,
                frame: self.frame,
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
        #[cfg(feature = "editor")]
        self.editor.update(graphics, &self.gui, &mut self.globals);
        if !self.input_maintain_on_fixed_step || fixed_step {
            self.input.maintain();
        }

        GraphScheduler::<true>
            .run_systems(
                &self.jobs.jobs,
                &self.universe,
                GraphScheduler::<true>::collect_roots(&self.universe.systems),
                Default::default(),
            )
            .unwrap();
        self.universe.clear_changes();
        self.universe.execute_commands::<true>();

        if !self.next_frame_queue.is_empty() {
            self.jobs.jobs.submit_queue(&self.next_frame_queue);
        }
        while !self.jobs.jobs.queue_is_empty() {
            let mut async_context = GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: None,
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: total_time,
                frame: self.frame,
            };
            let (async_context_lazy, _async_context_lifetime) =
                DynamicManagedLazy::make(&mut async_context);
            let (delta_time_lazy, _delta_time_lifetime) = DynamicManagedLazy::make(&mut delta_time);
            let (next_frame_queue_lazy, _next_frame_queue_lifetime) =
                DynamicManagedLazy::make(&mut self.next_frame_queue);
            self.jobs.jobs.run_local_timeout_with_meta(
                frame_budget,
                [
                    (CONTEXT_META.to_owned(), async_context_lazy),
                    (DELTA_TIME_META.to_owned(), delta_time_lazy),
                    (NEXT_FRAME_QUEUE_META.to_owned(), next_frame_queue_lazy),
                ],
            );
            if jobs_timer.elapsed() >= frame_budget {
                break;
            }
        }
    }

    pub fn process_event(&mut self, event: &Event<()>) -> bool {
        if let Event::WindowEvent { event, .. } = event {
            if let WindowEvent::Focused(focused) = &event {
                self.focused = *focused;
            }
            #[cfg(feature = "editor")]
            {
                self.editor.event(event, &mut self.gui, &mut self.globals);
                self.globals.editor.input.context.on_event(event);
            }
            self.input.on_event(event);
        }
        if let Some((state, _, _)) = self.states.last_mut() {
            state.event(&mut self.globals, event);
        }
        for subsystem in &mut self.subsystems {
            subsystem.event(&mut self.globals, event);
        }
        #[cfg(feature = "editor")]
        for subsystem in &mut self.editor.subsystems {
            subsystem.event(&mut self.globals, event);
        }
        !self.states.is_empty() || !matches!(self.state_change, GameStateChange::Continue)
    }
}

impl AppState<Vertex> for GameInstance {
    fn on_init(&mut self, _graphics: &mut Graphics<Vertex>, _: &mut AppControl) {
        #[cfg(feature = "editor")]
        {
            let temp = Val::new(());
            let state_heartbeat = temp.heartbeat();
            self.editor.initialize(GameContext {
                graphics: _graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                jobs: Some(&self.jobs),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                universe: &mut self.universe,
                graph: &mut self.graph,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: 0.0,
                frame: self.frame,
            });
        }
    }

    fn on_redraw(&mut self, graphics: &mut Graphics<Vertex>, _: &mut AppControl) {
        self.process_frame(graphics);
    }

    fn on_event(&mut self, event: Event<()>, _: &mut Window) -> bool {
        self.process_event(&event)
    }
}
