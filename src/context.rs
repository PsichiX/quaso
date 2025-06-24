use crate::{
    audio::Audio,
    coroutine::AsyncNextFrame,
    game::{GameGlobals, GameJobs, GameStateChange},
};
use keket::database::AssetDatabase;
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
    pub jobs: &'a mut GameJobs,
    pub async_next_frame: &'a AsyncNextFrame,
}
