use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    coroutine::{async_delay, async_delta_time},
    game::{GameInstance, GameState, GameStateChange},
    third_party::{
        moirai::coroutine::yield_now,
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, TextureRef},
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::GlowTextureFiltering,
        },
        spitfire_input::{InputActionRef, InputConsume, InputMapping, VirtualAction},
        vek::Vec2,
        windowing::event::VirtualKeyCode,
    },
    value::Val,
};
use rand::{Rng, rng};
use std::error::Error;

const SPEED: f32 = 100.0;

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Coroutines")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

#[derive(Default)]
struct Preloader;

impl GameState for Preloader {
    fn enter(&mut self, context: GameContext) {
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

        context.assets.ensure("texture://ferris.png").unwrap();

        *context.state_change = GameStateChange::Swap(Box::new(State::default()));
    }
}

#[derive(Default)]
struct State {
    ferris: Sprite,
    exit: InputActionRef,
    position: Val<Vec2<f32>>,
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        self.ferris = Sprite::single(SpriteTexture {
            sampler: "u_image".into(),
            texture: TextureRef::name("ferris.png"),
            filtering: GlowTextureFiltering::Linear,
        })
        .pivot(0.5.into());

        context
            .input
            .push_mapping(InputMapping::default().consume(InputConsume::Hit).action(
                VirtualAction::KeyButton(VirtualKeyCode::Escape),
                self.exit.clone(),
            ));

        // Get lazy managed value of the interpolated position.
        // This allows us to read and write the position in a thread-safe manner.
        let position = self.position.pointer();
        // Start a coroutine to handle the interpolated movement.
        context.jobs.unwrap().defer(async move {
            // interpolated movement never ends.
            loop {
                // Generate a random target position within a range.
                let target = Vec2::new(
                    rng().random_range(-100.0..100.0),
                    rng().random_range(-100.0..100.0),
                );
                // Wait 5 in-game seconds before starting the movement.
                async_delay(0.5).await;
                // Move towards the target position smoothly.
                loop {
                    // Calculate the delta time for smooth movement.
                    let dt = async_delta_time().await;
                    // If the position is close enough to the target, break the loop.
                    if (*position.read() - target).magnitude() <= SPEED * dt {
                        break;
                    }
                    // Interpolate towards the target position.
                    let delta = (target - *position.read()).normalized() * SPEED * dt;
                    *position.write() += delta;
                    // Yield control to allow other tasks to run.
                    yield_now().await;
                }
            }
        });
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, context: GameContext, _: f32) {
        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw(&mut self, context: GameContext) {
        self.ferris.transform.position = (*self.position.read()).into();
        self.ferris.draw(context.draw, context.graphics);
    }
}
