use quaso::{
    GameLauncher,
    assets::{
        ldtk::{FilteredLdtkEntityExtractor, LdtkAsset},
        make_directory_database,
        shader::ShaderAsset,
    },
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    map::{LdtkMapBuilder, Map, ldtk::EntityInstance},
    third_party::{
        spitfire_draw::utils::{Drawable, ShaderRef},
        spitfire_glow::graphics::{CameraScaling, Shader},
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        windowing::event::VirtualKeyCode,
    },
};
use rand::{Rng, thread_rng};
use spitfire_draw::{
    sprite::{Sprite, SpriteTexture},
    utils::TextureRef,
};
use spitfire_glow::renderer::GlowTextureFiltering;
use std::error::Error;
use vek::{Rect, Vec2};

const SPEED: f32 = 200.0;

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("LDTK")
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
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(200.0);

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

        context.assets.ensure("ldtk://world.zip").unwrap();

        *context.state_change = GameStateChange::Swap(Box::new(State::default()));
    }
}

#[derive(Default)]
struct State {
    movement: CardinalInputCombinator,
    map: Option<Map>,
    animals: Vec<Sprite>,
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        // Load LDTK world.
        let asset = context
            .assets
            .find("ldtk://world.zip")
            .unwrap()
            .access::<&LdtkAsset>(context.assets);

        // Create world map from LDTK world.
        self.map = Some(
            asset.build_map(
                LdtkMapBuilder::default()
                    .image_shader(ShaderRef::name("image"))
                    .int_grid_colliders(&[
                        ("Buildings", 1 << 0),
                        ("Forest", 1 << 0),
                        ("Walls", 1 << 0),
                        ("Water", 1 << 1),
                        ("Mountains", 1 << 0),
                    ]),
            ),
        );

        let extractor = FilteredLdtkEntityExtractor::default().by_identifier(
            "Animal",
            |entity: &EntityInstance| {
                let index = thread_rng().gen_range(0..=6);
                Some(
                    Sprite::single(
                        SpriteTexture::new(
                            "u_image".into(),
                            TextureRef::name("world.zip/characters.png"),
                        )
                        .filtering(GlowTextureFiltering::Nearest),
                    )
                    .shader(ShaderRef::name("image"))
                    .region_page(
                        Rect {
                            x: index as f32 * 8.0 / 48.0,
                            y: 32.0 / 40.0,
                            w: 8.0 / 48.0,
                            h: 8.0 / 40.0,
                        },
                        0.0,
                    )
                    .size(8.0.into())
                    .position(Vec2::new(
                        entity.world_x.unwrap_or_default() as f32,
                        entity.world_y.unwrap_or_default() as f32,
                    )),
                )
            },
        );
        self.animals = asset.extract_entities(&extractor).collect();

        // Setup inputs for moving the map.
        let move_left = InputActionRef::default();
        let move_right = InputActionRef::default();
        let move_up = InputActionRef::default();
        let move_down = InputActionRef::default();
        self.movement = CardinalInputCombinator::new(
            move_left.clone(),
            move_right.clone(),
            move_up.clone(),
            move_down.clone(),
        );
        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::A),
                    move_left.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::D),
                    move_right.clone(),
                )
                .action(VirtualAction::KeyButton(VirtualKeyCode::W), move_up.clone())
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::S),
                    move_down.clone(),
                )
                .action(VirtualAction::KeyButton(VirtualKeyCode::Left), move_left)
                .action(VirtualAction::KeyButton(VirtualKeyCode::Right), move_right)
                .action(VirtualAction::KeyButton(VirtualKeyCode::Up), move_up)
                .action(VirtualAction::KeyButton(VirtualKeyCode::Down), move_down),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {
        context.graphics.state.main_camera.transform.position +=
            Vec2::from(self.movement.get()) * SPEED * delta_time;
    }

    fn draw(&mut self, context: GameContext) {
        let Some(map) = &self.map else { return };

        map.draw()
            // .show_colliders(
            //     ShaderRef::name("color"),
            //     Rgba::new(1.0, 0.0, 1.0, 0.5),
            //     GlowBlending::Alpha,
            // )
            .draw(context.draw, context.graphics);

        for sprite in &self.animals {
            sprite.draw(context.draw, context.graphics);
        }
    }
}
