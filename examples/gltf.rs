use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    coroutine::{async_game_context, async_wait_for_asset},
    game::{GameInstance, GameState, GameStateChange},
    third_party::spitfire_glow::graphics::{CameraScaling, Shader},
};
use spitfire_draw::{
    sprite::{Sprite, SpriteTexture},
    utils::{Drawable, TextureRef},
};
use std::{error::Error, pin::Pin};

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("GLTF")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

#[derive(Default)]
struct Preloader;

impl GameState for Preloader {
    fn timeline(
        &mut self,
        context: GameContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
        context.graphics.state.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(500.0);

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

        let handle = context
            .assets
            .ensure("gltf://cesium-man.glb?binary")
            .unwrap();

        Box::pin(async move {
            println!("Waiting for GLTF asset to load...");
            async_wait_for_asset(handle).await;
            println!("GLTF asset loaded.");

            {
                let context = async_game_context().await.unwrap();
                for handle in handle.dependencies(context.assets) {
                    println!(
                        "Dependency: {} | Ready: {}",
                        handle.path(context.assets).unwrap().content(),
                        handle.is_ready_to_use(context.assets)
                    );
                }
            }

            {
                let context = async_game_context().await.unwrap();
                *context.state_change = GameStateChange::Swap(Box::new(State::default()));
            }
        })
    }
}

#[derive(Default)]
struct State {}

impl GameState for State {
    fn draw(&mut self, context: GameContext) {
        Sprite::single(SpriteTexture::new(
            "u_image".into(),
            TextureRef::name("cesium-man.glb@0"),
        ))
        .pivot(0.5.into())
        .size(500.0.into())
        .draw(context.draw, context.graphics);
    }
}
