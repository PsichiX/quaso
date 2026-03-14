use crate::{
    audio::Audio,
    game::{GameGlobals, GameJobs, GameStateChange, GameSubsystem},
    gc::Heartbeat,
    multiplayer::{GameConnection, GameMultiplayer, GameMultiplayerChange, GameNetwork},
};
use anput::universe::Universe;
use keket::database::AssetDatabase;
use moirai::queue::JobQueue;
use nodio::graph::Graph;
use spitfire_draw::{context::DrawContext, utils::Vertex};
use spitfire_glow::graphics::Graphics;
use spitfire_gui::context::GuiContext;
use spitfire_input::InputContext;
use tehuti::engine::EngineId;

pub struct GameContext<'a> {
    pub graphics: &'a mut Graphics<Vertex>,
    pub draw: &'a mut DrawContext,
    pub gui: &'a mut GuiContext,
    pub input: &'a mut InputContext,
    pub state_change: &'a mut GameStateChange,
    pub multiplayer_change: &'a mut GameMultiplayerChange,
    pub assets: &'a mut AssetDatabase,
    pub audio: &'a mut Audio,
    pub globals: &'a mut GameGlobals,
    pub jobs: Option<&'a GameJobs>,
    pub network: &'a mut GameNetwork,
    pub multiplayer: Option<&'a mut dyn GameMultiplayer>,
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
    pub fn network_connection<T: GameConnection + 'static>(
        &self,
        engine_id: EngineId,
    ) -> Option<&T> {
        self.network
            .connection(engine_id)?
            .as_any()
            .downcast_ref::<T>()
    }

    pub fn network_connection_mut<T: GameConnection + 'static>(
        &mut self,
        engine_id: EngineId,
    ) -> Option<&mut T> {
        self.network
            .connection_mut(engine_id)?
            .as_any_mut()
            .downcast_mut::<T>()
    }

    pub fn multiplayer<T: GameMultiplayer + 'static>(&self) -> Option<&T> {
        self.multiplayer.as_ref()?.as_any().downcast_ref::<T>()
    }

    pub fn multiplayer_mut<T: GameMultiplayer + 'static>(&mut self) -> Option<&mut T> {
        self.multiplayer.as_mut()?.as_any_mut().downcast_mut::<T>()
    }

    /// Forks the context, returning a new GameContext with the caller's lifetime.
    ///
    /// # Safety
    /// The caller must ensure that the returned GameContext does not outlive the original context
    /// and that no aliasing mutable references are created. This transmutes the lifetime to the caller's.
    pub unsafe fn fork<'b>(&'b mut self) -> GameContext<'b> {
        unsafe {
            std::mem::transmute::<&mut GameContext<'a>, &mut GameContext<'b>>(self).clone_inner()
        }
    }

    fn clone_inner(&mut self) -> GameContext<'_> {
        GameContext {
            graphics: self.graphics,
            draw: self.draw,
            gui: self.gui,
            input: self.input,
            state_change: self.state_change,
            multiplayer_change: self.multiplayer_change,
            assets: self.assets,
            audio: self.audio,
            globals: self.globals,
            jobs: self.jobs,
            network: self.network,
            multiplayer: match &mut self.multiplayer {
                Some(multiplayer) => Some(&mut **multiplayer),
                None => None,
            },
            update_queue: self.update_queue,
            fixed_update_queue: self.fixed_update_queue,
            draw_queue: self.draw_queue,
            draw_gui_queue: self.draw_gui_queue,
            universe: self.universe,
            graph: self.graph,
            state_heartbeat: self.state_heartbeat,
            subsystems: GameSubsystems {
                subsystems: self.subsystems.subsystems,
            },
            time: self.time,
            frame: self.frame,
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
