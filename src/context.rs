use crate::{
    audio::Audio,
    game::{GameGlobals, GameJobs, GameStateChange, GameSubsystem},
    gc::Heartbeat,
};
use anput::universe::Universe;
use keket::database::AssetDatabase;
use moirai::queue::JobQueue;
use nodio::graph::Graph;
use spitfire_draw::{context::DrawContext, utils::Vertex};
use spitfire_glow::graphics::Graphics;
use spitfire_gui::context::GuiContext;
use spitfire_input::InputContext;

pub struct GameContext<'a> {
    pub graphics: &'a mut Graphics<Vertex>,
    pub draw: &'a mut DrawContext,
    pub gui: &'a mut GuiContext,
    pub input: &'a mut InputContext,
    pub state_change: &'a mut GameStateChange,
    pub assets: &'a mut AssetDatabase,
    pub audio: &'a mut Audio,
    pub globals: &'a mut GameGlobals,
    pub jobs: Option<&'a GameJobs>,
    pub update_queue: &'a JobQueue,
    pub fixed_update_queue: &'a JobQueue,
    pub draw_queue: &'a JobQueue,
    pub draw_gui_queue: &'a JobQueue,
    pub universe: &'a mut Universe,
    pub graph: &'a mut Graph,
    pub state_heartbeat: &'a Heartbeat,
    pub subsystems: GameSubsystems<'a>,
    pub time: f32,
    pub frame: usize,
}

impl<'a> GameContext<'a> {
    pub(crate) fn fork(other: &'a mut Self) -> Self {
        Self {
            graphics: other.graphics,
            draw: other.draw,
            gui: other.gui,
            input: other.input,
            state_change: other.state_change,
            assets: other.assets,
            audio: other.audio,
            globals: other.globals,
            jobs: None,
            update_queue: other.update_queue,
            fixed_update_queue: other.fixed_update_queue,
            draw_queue: other.draw_queue,
            draw_gui_queue: other.draw_gui_queue,
            universe: other.universe,
            graph: other.graph,
            state_heartbeat: other.state_heartbeat,
            subsystems: GameSubsystems {
                subsystems: other.subsystems.subsystems,
            },
            time: other.time,
            frame: other.frame,
        }
    }
}

pub struct GameSubsystems<'a> {
    pub subsystems: &'a mut [Box<dyn GameSubsystem>],
}

impl<'a> GameSubsystems<'a> {
    pub fn get<T: GameSubsystem + 'static>(&self) -> Option<&T> {
        for subsystem in self.subsystems.iter() {
            if let Some(specific) = subsystem.as_any().downcast_ref::<T>() {
                return Some(specific);
            }
        }
        None
    }

    pub fn get_mut<T: GameSubsystem + 'static>(&mut self) -> Option<&mut T> {
        for subsystem in self.subsystems.iter_mut() {
            if let Some(specific) = subsystem.as_any_mut().downcast_mut::<T>() {
                return Some(specific);
            }
        }
        None
    }
}
