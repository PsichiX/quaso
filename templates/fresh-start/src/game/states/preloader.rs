use crate::game::states::gameplay::Gameplay;
use quaso::{
    assets::shader::ShaderAsset,
    context::GameContext,
    game::{GameState, GameStateChange},
    third_party::spitfire_glow::graphics::Shader,
};

pub struct Preloader;

impl GameState for Preloader {
    fn enter(&mut self, context: GameContext) {
        context
            .assets
            .spawn(
                "shader://color",
                (ShaderAsset::new(
                    Shader::COLORED_VERTEX_2D,
                    Shader::PASS_FRAGMENT,
                ),),
            )
            .unwrap();
        context
            .assets
            .spawn(
                "shader://image",
                (ShaderAsset::new(
                    Shader::TEXTURED_VERTEX_2D,
                    Shader::TEXTURED_FRAGMENT,
                ),),
            )
            .unwrap();
        context
            .assets
            .spawn(
                "shader://text",
                (ShaderAsset::new(Shader::TEXT_VERTEX, Shader::TEXT_FRAGMENT),),
            )
            .unwrap();

        context.assets.ensure("group://index").unwrap();
    }

    fn update(&mut self, context: GameContext, _delta_time: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(Gameplay::default()));
        }
    }
}
