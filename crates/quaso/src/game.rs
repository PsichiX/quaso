#[cfg(feature = "editor")]
use crate::editor::EditorInput;
use crate::{
    assets::{
        anim_texture::AnimTextureAssetSubsystem, auri::AuriAssetSubsystem,
        font::FontAssetSubsystem, gltf::GltfAssetSubsystem, shader::ShaderAssetSubsystem,
        sound::SoundAssetSubsystem, texture::TextureAssetSubsystem,
    },
    audio::Audio,
    commands::{GameCommand, GameCommands},
    context::{GameContext, GameSubsystems},
    gc::{DynGc, Gc},
    input::GameInputControl,
    multiplayer::{GameMultiplayer, GameMultiplayerChange, GameNetwork, local::LocalMultiplayer},
    scripting::Scripting,
    third_party::{
        time::{Duration, Instant},
        windowing::{
            event::{Event, WindowEvent},
            window::Window,
        },
    },
};
use gilrs::Gilrs;
use intuicio_data::managed::DynamicManagedLazy;
use keket::database::AssetDatabase;
use moirai::{
    job::{JobHandle, JobLocation, JobOptions},
    jobs::Jobs,
    queue::JobQueue,
};
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
    borrow::Cow,
    cell::LazyCell,
    collections::BTreeMap,
    pin::Pin,
};
use tehuti::peer::{Peer, PeerId};
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

    fn can_reentry_from_background(&self) -> bool {
        false
    }

    fn multiplayer_peer_added(&mut self, context: GameContext, peer: Peer) {}

    fn multiplayer_peer_removed(&mut self, context: GameContext, peer_id: PeerId) {}

    fn custom_event(&mut self, context: GameContext, payload: &mut dyn Any) {}
}

#[macro_export]
macro_rules! game_state_custom_event {
    (@item ($self:expr, $context:expr, $payload:expr) => trait($trait:ident)) => {
        if <Self as $trait>::is($payload) {
            <Self as $trait>::custom_event($self, $context, $payload);
            return;
        }
    };
    (@item ($self:expr, $context:expr, $payload:expr) => fn($method:ident, $payload_type:ident)) => {
        if let Some(payload) = $payload.downcast_mut::<$payload_type>() {
            $self.$method($context, payload);
            return;
        }
    };
    ($(
        $kind:ident ( $( $arg:tt ),* )
    ),*) => {
        fn custom_event(&mut self, context: GameContext, payload: &mut dyn std::any::Any) {
            $(
                $crate::game_state_custom_event!(
                    @item (self, context, payload) => $kind ( $( $arg ),* )
                );
            )*
        }
    };
}

#[allow(unused_variables)]
pub trait GameSubsystem {
    fn update(&mut self, context: GameContext, delta_time: f32) {}

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {}

    fn draw(&mut self, context: GameContext) {}

    fn draw_gui(&mut self, context: GameContext) {}

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

    // Edit mode otherwise only toggles on a real key event, which a command
    // cannot produce. Without this an agent that finds the game in edit mode
    // cannot get it running again.
    pub fn set_editing(&mut self, value: bool) {
        self.is_editing = value;
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

#[derive(Debug, Clone)]
pub struct GameTimeControl {
    paused: bool,
    time_scale: f32,
    pending_steps: usize,
    accumulator: f32,
    total: f64,
    step: f32,
    steps_last_frame: usize,
    total_steps: u64,
    dropped_steps: usize,
}

impl Default for GameTimeControl {
    fn default() -> Self {
        Self {
            paused: false,
            time_scale: 1.0,
            pending_steps: 0,
            accumulator: 0.0,
            total: 0.0,
            step: 0.0,
            steps_last_frame: 0,
            total_steps: 0,
            dropped_steps: 0,
        }
    }
}

impl GameTimeControl {
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, value: bool) {
        self.paused = value;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn time_scale(&self) -> f32 {
        self.time_scale
    }

    pub fn set_time_scale(&mut self, value: f32) {
        self.time_scale = value.max(0.0);
    }

    pub fn request_steps(&mut self, count: usize) {
        self.pending_steps = self.pending_steps.saturating_add(count);
    }

    pub fn pending_steps(&self) -> usize {
        self.pending_steps
    }

    pub fn cancel_pending_steps(&mut self) {
        self.pending_steps = 0;
    }

    pub fn steps_last_frame(&self) -> usize {
        self.steps_last_frame
    }

    // Counts every fixed step the game has run. It only ever grows, so a reader
    // that remembers the last value it saw learns how many steps happened since,
    // no matter which frames it looked on. `steps_last_frame` cannot do that,
    // because it holds a stale value on any frame that skips the update phase.
    pub fn total_steps(&self) -> u64 {
        self.total_steps
    }

    pub fn dropped_steps(&self) -> usize {
        self.dropped_steps
    }

    pub fn total_time(&self) -> f32 {
        self.total as f32
    }

    pub fn step(&self) -> f32 {
        self.step
    }

    fn advance(&mut self, delta_time: f32, unfocused_divisor: f32) -> f32 {
        if self.paused {
            return 0.0;
        }
        let scaled = delta_time * self.time_scale / unfocused_divisor;
        self.accumulator += scaled;
        self.total += scaled as f64;
        scaled
    }

    fn take_steps(&mut self, step: f32, max_per_frame: usize) -> usize {
        self.step = step;
        let mut count = 0;
        if step > 0.0 && max_per_frame > 0 {
            count = self.pending_steps.min(max_per_frame);
            self.pending_steps -= count;
            if self.paused {
                self.total += (count as f32 * step) as f64;
            } else {
                while count < max_per_frame && self.accumulator >= step {
                    self.accumulator -= step;
                    count += 1;
                }
                if self.accumulator >= step {
                    self.dropped_steps += (self.accumulator / step) as usize;
                    self.accumulator %= step;
                }
            }
        }
        self.steps_last_frame = count;
        self.total_steps = self.total_steps.saturating_add(count as u64);
        count
    }
}

#[derive(Debug, Default, Clone)]
pub struct GameAppControl {
    close_requested: bool,
    fullscreen: bool,
    fullscreen_request: Option<bool>,
}

impl GameAppControl {
    pub fn request_close(&mut self) {
        self.close_requested = true;
    }

    pub fn cancel_close(&mut self) {
        self.close_requested = false;
    }

    pub fn is_close_requested(&self) -> bool {
        self.close_requested
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen_request = Some(fullscreen);
    }

    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen_request.unwrap_or(self.fullscreen)
    }
}

pub struct GameGlobals {
    globals: BTreeMap<TypeId, DynGc>,
    is_touch_device: LazyCell<bool>,
    pub time: GameTimeControl,
    pub app: GameAppControl,
    pub input: GameInputControl,
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
            time: Default::default(),
            app: Default::default(),
            input: Default::default(),
            #[cfg(feature = "editor")]
            editor: Default::default(),
        }
    }
}

impl GameGlobals {
    pub fn set<T: 'static>(&mut self, value: T) {
        self.globals.insert(TypeId::of::<T>(), DynGc::new(value));
    }

    pub fn unset<T: 'static>(&mut self) {
        self.globals.remove(&TypeId::of::<T>());
    }

    pub fn access<T: 'static>(&'_ self) -> Option<Gc<T>> {
        self.globals
            .get(&TypeId::of::<T>())
            .map(|v| v.reference().into_typed())
    }

    pub fn ensure<T: Default + 'static>(&mut self) -> Gc<T> {
        self.globals
            .entry(TypeId::of::<T>())
            .or_insert_with(|| DynGc::new(T::default()))
            .reference()
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
        meta: impl IntoIterator<Item = (Cow<'static, str>, DynGc)>,
        job: impl Future<Output = T> + Send + Sync + 'static,
    ) -> JobHandle<T> {
        self.spawn(
            JobOptions::default()
                .location(JobLocation::Local)
                .meta_many(meta.into_iter().map(|(id, gc)| (id, gc.0.into()))),
            job,
        )
    }

    pub fn spawn<T: Send>(
        &self,
        options: impl Into<JobOptions>,
        job: impl Future<Output = T> + Send + Sync + 'static,
    ) -> JobHandle<T> {
        self.jobs.spawn(options, job)
    }

    pub fn jobs(&self) -> &Jobs {
        &self.jobs
    }
}

pub struct GameInstance {
    pub fixed_delta_time: f32,
    pub unfocused_fixed_delta_time_scale: f32,
    pub color_shader: &'static str,
    pub image_shader: &'static str,
    pub text_shader: &'static str,
    pub input_maintain_on_fixed_step: bool,
    pub max_fixed_steps_per_frame: usize,
    commands: GameCommands,
    draw: DrawContext,
    gui: GuiContext,
    input: InputContext,
    assets: AssetDatabase,
    audio: Audio,
    timer: Instant,
    frame: usize,
    #[allow(clippy::type_complexity)]
    states: Vec<(Box<dyn GameState>, JobHandle<()>, Gc<()>)>,
    state_change: GameStateChange,
    subsystems: Vec<Box<dyn GameSubsystem>>,
    globals: GameGlobals,
    scripting: Scripting,
    jobs: GameJobs,
    network: GameNetwork,
    multiplayer: Box<dyn GameMultiplayer>,
    multiplayer_change: GameMultiplayerChange,
    next_frame_queue: JobQueue,
    update_queue: JobQueue,
    next_update_queue: JobQueue,
    fixed_update_queue: JobQueue,
    next_fixed_update_queue: JobQueue,
    draw_queue: JobQueue,
    next_draw_queue: JobQueue,
    draw_gui_queue: JobQueue,
    next_draw_gui_queue: JobQueue,
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
            max_fixed_steps_per_frame: 8,
            commands: Default::default(),
            draw: Default::default(),
            gui: Default::default(),
            input: Default::default(),
            assets: Default::default(),
            audio: Default::default(),
            timer: Instant::now(),
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
                Box::new(AuriAssetSubsystem),
            ],
            globals: Default::default(),
            scripting: Default::default(),
            jobs: Default::default(),
            network: Default::default(),
            multiplayer: Box::new(LocalMultiplayer::default()),
            multiplayer_change: Default::default(),
            next_frame_queue: Default::default(),
            update_queue: Default::default(),
            next_update_queue: Default::default(),
            fixed_update_queue: Default::default(),
            next_fixed_update_queue: Default::default(),
            draw_queue: Default::default(),
            next_draw_queue: Default::default(),
            draw_gui_queue: Default::default(),
            next_draw_gui_queue: Default::default(),
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

    pub fn with_max_fixed_steps_per_frame(mut self, value: usize) -> Self {
        self.max_fixed_steps_per_frame = value;
        self
    }

    pub fn with_commands(mut self, commands: GameCommands) -> Self {
        self.commands = commands;
        self
    }

    // Dropping the returned server does not stop it. The accept thread and every
    // connection thread live until the process ends, which is what a dev only
    // control port wants: the game keeps answering for as long as it runs.
    #[cfg(all(feature = "agent", not(target_arch = "wasm32")))]
    pub fn with_agent_server(self, config: crate::agent::AgentServerConfig) -> Self {
        if let Err(error) = crate::agent::AgentServer::start(self.commands.handle(), config) {
            tracing::event!(
                target: "quaso::agent",
                tracing::Level::ERROR,
                "{}",
                error
            );
        }
        self
    }

    pub fn with_command(mut self, command: GameCommand) -> Self {
        if let Err(error) = self.commands.register(command) {
            tracing::event!(
                target: "quaso::commands",
                tracing::Level::ERROR,
                "{}",
                error
            );
        }
        self
    }

    pub fn with_paused(mut self, value: bool) -> Self {
        self.globals.time.set_paused(value);
        self
    }

    pub fn with_time_scale(mut self, value: f32) -> Self {
        self.globals.time.set_time_scale(value);
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

    pub fn with_jobs_unnamed_worker(mut self, iteration_timeout: Duration) -> Self {
        self.jobs.jobs.add_unnamed_worker(iteration_timeout);
        self
    }

    pub fn with_jobs_named_worker(
        mut self,
        iteration_timeout: Duration,
        name: impl ToString,
    ) -> Self {
        self.jobs.jobs.add_named_worker(iteration_timeout, name);
        self
    }

    pub fn with_network(mut self, network: GameNetwork) -> Self {
        self.network = network;
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

    pub fn time(&self) -> &GameTimeControl {
        &self.globals.time
    }

    pub fn time_mut(&mut self) -> &mut GameTimeControl {
        &mut self.globals.time
    }

    pub fn commands(&self) -> &GameCommands {
        &self.commands
    }

    pub fn commands_mut(&mut self) -> &mut GameCommands {
        &mut self.commands
    }

    pub fn process_frame(&mut self, graphics: &mut Graphics<Vertex>) {
        let total_time = self.globals.time.total_time();

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
                            multiplayer_change: &mut self.multiplayer_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            scripting: Some(&mut self.scripting),
                            jobs: Some(&self.jobs),
                            network: &mut self.network,
                            multiplayer: Some(&mut *self.multiplayer),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
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
                    let state_value = Gc::new(());
                    let state_heartbeat = state_value.heartbeat();
                    state.enter(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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
                    if let Some((state, _, state_value)) = self.states.last_mut()
                        && state.can_reentry_from_background()
                    {
                        let state_heartbeat = state_value.heartbeat();
                        state.exit(GameContext {
                            graphics,
                            draw: &mut self.draw,
                            gui: &mut self.gui,
                            input: &mut self.input,
                            state_change: &mut self.state_change,
                            multiplayer_change: &mut self.multiplayer_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            scripting: Some(&mut self.scripting),
                            jobs: Some(&self.jobs),
                            network: &mut self.network,
                            multiplayer: Some(&mut *self.multiplayer),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
                            state_heartbeat: &state_heartbeat,
                            subsystems: GameSubsystems {
                                subsystems: &mut self.subsystems,
                            },
                            time: total_time,
                            frame: self.frame,
                        });
                    }
                    let state_value = Gc::new(());
                    let state_heartbeat = state_value.heartbeat();
                    state.enter(GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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
                            multiplayer_change: &mut self.multiplayer_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            scripting: Some(&mut self.scripting),
                            jobs: Some(&self.jobs),
                            network: &mut self.network,
                            multiplayer: Some(&mut *self.multiplayer),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
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
                    if let Some((state, _, state_value)) = self.states.last_mut()
                        && state.can_reentry_from_background()
                    {
                        let state_heartbeat = state_value.heartbeat();
                        state.enter(GameContext {
                            graphics,
                            draw: &mut self.draw,
                            gui: &mut self.gui,
                            input: &mut self.input,
                            state_change: &mut self.state_change,
                            multiplayer_change: &mut self.multiplayer_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            scripting: Some(&mut self.scripting),
                            jobs: Some(&self.jobs),
                            network: &mut self.network,
                            multiplayer: Some(&mut *self.multiplayer),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
                            state_heartbeat: &state_heartbeat,
                            subsystems: GameSubsystems {
                                subsystems: &mut self.subsystems,
                            },
                            time: total_time,
                            frame: self.frame,
                        });
                    }
                    self.timer = Instant::now();
                }
            }
            break;
        }

        self.frame += 1;
        #[cfg(feature = "editor")]
        let is_editing = self.globals.editor.is_editing();
        let real_delta_time = self.timer.elapsed().as_secs_f32();
        let jobs_timer = self.timer;
        self.timer = Instant::now();
        let unfocused_divisor = if self.focused {
            1.0
        } else {
            self.unfocused_fixed_delta_time_scale.max(f32::EPSILON)
        };
        let mut delta_time = self
            .globals
            .time
            .advance(real_delta_time, unfocused_divisor);
        let total_time = self.globals.time.total_time();
        let frame_budget = Duration::from_secs_f32(self.fixed_delta_time);
        let Some(state_value) = self.states.last().map(|(_, _, value)| value) else {
            return;
        };
        let state_heartbeat = state_value.heartbeat();

        self.network.maintain();
        if let Some((state, _, _)) = self.states.last_mut() {
            let change = match std::mem::take(&mut self.multiplayer_change) {
                GameMultiplayerChange::None => None,
                GameMultiplayerChange::Set(multiplayer) => Some(multiplayer),
                GameMultiplayerChange::Reset => {
                    Some(Box::new(LocalMultiplayer::default()) as Box<dyn GameMultiplayer>)
                }
            };
            if let Some(multiplayer) = change {
                self.multiplayer.on_cleanup(
                    &mut **state,
                    GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: None,
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    },
                );
                self.multiplayer = multiplayer;
                self.multiplayer.on_startup(
                    &mut **state,
                    GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: None,
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
                        state_heartbeat: &state_heartbeat,
                        subsystems: GameSubsystems {
                            subsystems: &mut self.subsystems,
                        },
                        time: total_time,
                        frame: self.frame,
                    },
                );
            }
            self.multiplayer.maintain(
                &mut **state,
                GameContext {
                    graphics,
                    draw: &mut self.draw,
                    gui: &mut self.gui,
                    input: &mut self.input,
                    state_change: &mut self.state_change,
                    multiplayer_change: &mut self.multiplayer_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                    scripting: Some(&mut self.scripting),
                    jobs: Some(&self.jobs),
                    network: &mut self.network,
                    multiplayer: None,
                    update_queue: &self.next_update_queue,
                    fixed_update_queue: &self.next_fixed_update_queue,
                    draw_queue: &self.next_draw_queue,
                    draw_gui_queue: &self.next_draw_gui_queue,
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

        // The command queue is drained outside `update_phase` on purpose. The
        // editor skips `update_phase` while it is editing, and a game that
        // answers no command in that mode is unreachable for an agent: it could
        // not even resume time or close the window.
        self.commands.process(&mut GameContext {
            graphics,
            draw: &mut self.draw,
            gui: &mut self.gui,
            input: &mut self.input,
            state_change: &mut self.state_change,
            multiplayer_change: &mut self.multiplayer_change,
            assets: &mut self.assets,
            audio: &mut self.audio,
            globals: &mut self.globals,
            scripting: Some(&mut self.scripting),
            jobs: Some(&self.jobs),
            network: &mut self.network,
            multiplayer: Some(&mut *self.multiplayer),
            update_queue: &self.next_update_queue,
            fixed_update_queue: &self.next_fixed_update_queue,
            draw_queue: &self.next_draw_queue,
            draw_gui_queue: &self.next_draw_gui_queue,
            state_heartbeat: &state_heartbeat,
            subsystems: GameSubsystems {
                subsystems: &mut self.subsystems,
            },
            time: total_time,
            frame: self.frame,
        });
        let total_steps = self.globals.time.total_steps();
        self.globals.input.apply(&self.input, total_steps);

        let mut update_phase = || {
            if let Some((state, _, _)) = self.states.last_mut() {
                state.update(
                    GameContext {
                        graphics,
                        draw: &mut self.draw,
                        gui: &mut self.gui,
                        input: &mut self.input,
                        state_change: &mut self.state_change,
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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
                    multiplayer_change: &mut self.multiplayer_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                    scripting: Some(&mut self.scripting),
                    jobs: None,
                    network: &mut self.network,
                    multiplayer: Some(&mut *self.multiplayer),
                    update_queue: &self.next_update_queue,
                    fixed_update_queue: &self.next_fixed_update_queue,
                    draw_queue: &self.next_draw_queue,
                    draw_gui_queue: &self.next_draw_gui_queue,
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
                        (CONTEXT_META.into(), async_context_lazy.into()),
                        (DELTA_TIME_META.into(), delta_time_lazy.into()),
                        (NEXT_FRAME_QUEUE_META.into(), next_frame_queue_lazy.into()),
                    ]
                    .into_iter()
                    .collect(),
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
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: Some(&self.jobs),
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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

            let mut fixed_delta_time = self.fixed_delta_time;
            let max_fixed_steps_per_frame = self.max_fixed_steps_per_frame;
            let steps = self
                .globals
                .time
                .take_steps(fixed_delta_time, max_fixed_steps_per_frame);

            for _ in 0..steps {
                if let Some((state, _, _)) = self.states.last_mut() {
                    state.fixed_update(
                        GameContext {
                            graphics,
                            draw: &mut self.draw,
                            gui: &mut self.gui,
                            input: &mut self.input,
                            state_change: &mut self.state_change,
                            multiplayer_change: &mut self.multiplayer_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            scripting: Some(&mut self.scripting),
                            jobs: Some(&self.jobs),
                            network: &mut self.network,
                            multiplayer: Some(&mut *self.multiplayer),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
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
                            multiplayer_change: &mut self.multiplayer_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            scripting: Some(&mut self.scripting),
                            jobs: Some(&self.jobs),
                            network: &mut self.network,
                            multiplayer: Some(&mut *self.multiplayer),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
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
                        multiplayer_change: &mut self.multiplayer_change,
                        assets: &mut self.assets,
                        audio: &mut self.audio,
                        globals: &mut self.globals,
                        scripting: Some(&mut self.scripting),
                        jobs: None,
                        network: &mut self.network,
                        multiplayer: Some(&mut *self.multiplayer),
                        update_queue: &self.next_update_queue,
                        fixed_update_queue: &self.next_fixed_update_queue,
                        draw_queue: &self.next_draw_queue,
                        draw_gui_queue: &self.next_draw_gui_queue,
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
                            (CONTEXT_META.into(), async_context_lazy.into()),
                            (DELTA_TIME_META.into(), delta_time_lazy.into()),
                            (NEXT_FRAME_QUEUE_META.into(), next_frame_queue_lazy.into()),
                        ]
                        .into_iter()
                        .collect(),
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
                            multiplayer_change: &mut self.multiplayer_change,
                            assets: &mut self.assets,
                            audio: &mut self.audio,
                            globals: &mut self.globals,
                            scripting: Some(&mut self.scripting),
                            jobs: Some(&self.jobs),
                            network: &mut self.network,
                            multiplayer: Some(&mut *self.multiplayer),
                            update_queue: &self.next_update_queue,
                            fixed_update_queue: &self.next_fixed_update_queue,
                            draw_queue: &self.next_draw_queue,
                            draw_gui_queue: &self.next_draw_gui_queue,
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
            }
            steps > 0
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
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: Some(&self.jobs),
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: Some(&self.jobs),
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: None,
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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
                    (CONTEXT_META.into(), async_context_lazy.into()),
                    (DELTA_TIME_META.into(), delta_time_lazy.into()),
                    (NEXT_FRAME_QUEUE_META.into(), next_frame_queue_lazy.into()),
                ]
                .into_iter()
                .collect(),
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
                    multiplayer_change: &mut self.multiplayer_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                    scripting: Some(&mut self.scripting),
                    jobs: Some(&self.jobs),
                    network: &mut self.network,
                    multiplayer: Some(&mut *self.multiplayer),
                    update_queue: &self.next_update_queue,
                    fixed_update_queue: &self.next_fixed_update_queue,
                    draw_queue: &self.next_draw_queue,
                    draw_gui_queue: &self.next_draw_gui_queue,
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
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: Some(&self.jobs),
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: Some(&self.jobs),
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: None,
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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
                    (CONTEXT_META.into(), async_context_lazy.into()),
                    (DELTA_TIME_META.into(), delta_time_lazy.into()),
                    (NEXT_FRAME_QUEUE_META.into(), next_frame_queue_lazy.into()),
                ]
                .into_iter()
                .collect(),
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
                    multiplayer_change: &mut self.multiplayer_change,
                    assets: &mut self.assets,
                    audio: &mut self.audio,
                    globals: &mut self.globals,
                    scripting: Some(&mut self.scripting),
                    jobs: Some(&self.jobs),
                    network: &mut self.network,
                    multiplayer: Some(&mut *self.multiplayer),
                    update_queue: &self.next_update_queue,
                    fixed_update_queue: &self.next_fixed_update_queue,
                    draw_queue: &self.next_draw_queue,
                    draw_gui_queue: &self.next_draw_gui_queue,
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
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: Some(&self.jobs),
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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

        if !self.next_frame_queue.is_empty() {
            self.jobs.jobs.submit_queue(&self.next_frame_queue);
        }
        while self
            .jobs
            .jobs
            .queue()
            .filter_count(|object| object.location() == &JobLocation::Local)
            > 0
        {
            let mut async_context = GameContext {
                graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: None,
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
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
                    (CONTEXT_META.into(), async_context_lazy.into()),
                    (DELTA_TIME_META.into(), delta_time_lazy.into()),
                    (NEXT_FRAME_QUEUE_META.into(), next_frame_queue_lazy.into()),
                ]
                .into_iter()
                .collect(),
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
    fn on_init(&mut self, _graphics: &mut Graphics<Vertex>, control: &mut AppControl) {
        self.globals.app.fullscreen = control.fullscreen();
        #[cfg(feature = "editor")]
        {
            let temp = Gc::new(());
            let state_heartbeat = temp.heartbeat();
            self.editor.initialize(GameContext {
                graphics: _graphics,
                draw: &mut self.draw,
                gui: &mut self.gui,
                input: &mut self.input,
                state_change: &mut self.state_change,
                multiplayer_change: &mut self.multiplayer_change,
                assets: &mut self.assets,
                audio: &mut self.audio,
                globals: &mut self.globals,
                scripting: Some(&mut self.scripting),
                jobs: Some(&self.jobs),
                network: &mut self.network,
                multiplayer: Some(&mut *self.multiplayer),
                update_queue: &self.next_update_queue,
                fixed_update_queue: &self.next_fixed_update_queue,
                draw_queue: &self.next_draw_queue,
                draw_gui_queue: &self.next_draw_gui_queue,
                state_heartbeat: &state_heartbeat,
                subsystems: GameSubsystems {
                    subsystems: &mut self.subsystems,
                },
                time: 0.0,
                frame: self.frame,
            });
        }
    }

    fn on_redraw(&mut self, graphics: &mut Graphics<Vertex>, control: &mut AppControl) {
        self.globals.app.fullscreen = control.fullscreen();
        self.process_frame(graphics);
        if self.globals.app.is_close_requested() {
            control.close_requested = true;
        }
        if let Some(fullscreen) = self.globals.app.fullscreen_request.take() {
            control.set_fullscreen(fullscreen);
        }
    }

    fn on_event(&mut self, event: Event<()>, _: Option<&Window>) -> bool {
        self.process_event(&event)
    }
}

#[cfg(test)]
mod tests {
    use super::GameTimeControl;

    const STEP: f32 = 1.0 / 60.0;

    #[test]
    fn test_time_control_runs_whole_steps_and_carries_the_remainder() {
        let mut time = GameTimeControl::default();

        time.advance(STEP * 2.5, 1.0);
        assert_eq!(time.take_steps(STEP, 8), 2);
        assert!((time.accumulator - STEP * 0.5).abs() < 1.0e-6);

        time.advance(STEP * 0.6, 1.0);
        assert_eq!(time.take_steps(STEP, 8), 1);
        assert!((time.accumulator - STEP * 0.1).abs() < 1.0e-6);
    }

    #[test]
    fn test_time_control_carries_a_partial_step_between_frames() {
        let mut time = GameTimeControl::default();

        time.advance(STEP * 0.5, 1.0);
        assert_eq!(time.take_steps(STEP, 8), 0);

        time.advance(STEP * 0.5, 1.0);
        assert_eq!(time.take_steps(STEP, 8), 1);
    }

    #[test]
    fn test_time_control_pause_stops_accumulation_and_freezes_total_time() {
        let mut time = GameTimeControl::default();

        time.pause();
        assert_eq!(time.advance(STEP * 10.0, 1.0), 0.0);
        assert_eq!(time.take_steps(STEP, 8), 0);
        assert_eq!(time.total_time(), 0.0);

        time.resume();
        time.advance(STEP, 1.0);
        assert_eq!(time.take_steps(STEP, 8), 1);
        assert!((time.total_time() - STEP).abs() < 1.0e-6);
    }

    #[test]
    fn test_time_control_requested_steps_run_while_paused_and_advance_total_time() {
        let mut time = GameTimeControl::default();

        time.pause();
        time.request_steps(3);
        assert_eq!(time.pending_steps(), 3);
        assert_eq!(time.take_steps(STEP, 8), 3);
        assert_eq!(time.pending_steps(), 0);
        assert!((time.total_time() - STEP * 3.0).abs() < 1.0e-6);

        assert_eq!(time.take_steps(STEP, 8), 0);
    }

    #[test]
    fn test_time_control_requested_steps_over_the_cap_carry_to_later_frames() {
        let mut time = GameTimeControl::default();

        time.pause();
        time.request_steps(5);
        assert_eq!(time.take_steps(STEP, 2), 2);
        assert_eq!(time.take_steps(STEP, 2), 2);
        assert_eq!(time.take_steps(STEP, 2), 1);
        assert_eq!(time.take_steps(STEP, 2), 0);
    }

    #[test]
    fn test_time_control_drops_the_backlog_past_the_cap_instead_of_spiralling() {
        let mut time = GameTimeControl::default();

        time.advance(STEP * 100.0, 1.0);
        assert_eq!(time.take_steps(STEP, 8), 8);
        assert_eq!(time.dropped_steps(), 92);
        assert!(time.accumulator < STEP);
        assert_eq!(time.take_steps(STEP, 8), 0);
    }

    #[test]
    fn test_time_control_time_scale_and_unfocused_divisor_scale_accumulation() {
        let mut time = GameTimeControl::default();

        time.set_time_scale(2.0);
        assert_eq!(time.advance(STEP, 1.0), STEP * 2.0);
        assert_eq!(time.take_steps(STEP, 8), 2);

        time.set_time_scale(1.0);
        assert_eq!(time.advance(STEP * 4.0, 4.0), STEP);
        assert_eq!(time.take_steps(STEP, 8), 1);
    }

    #[test]
    fn test_time_control_rejects_a_negative_time_scale() {
        let mut time = GameTimeControl::default();

        time.set_time_scale(-1.0);
        assert_eq!(time.time_scale(), 0.0);
        assert_eq!(time.advance(STEP, 1.0), 0.0);
        assert_eq!(time.take_steps(STEP, 8), 0);
    }
}
