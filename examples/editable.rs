use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    editable, editable_renderables,
    editor::{
        Editor,
        features::editable::editables::{
            EditableAsText, EditablePosition, EditableRotation, EditableScale,
        },
    },
    game::{GameInstance, GameState, GameStateChange},
    third_party::{
        raui_core::layout::CoordsMappingScaling,
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, TextureRef},
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::GlowTextureFiltering,
        },
        vek::Vec2,
    },
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(
        GameInstance::new(Preloader)
            .setup_assets(|assets| {
                *assets = make_directory_database("./resources/").unwrap();
            })
            .setup(|instance| {
                #[cfg(feature = "editor")]
                {
                    use quaso::editor::ui::editor_game_edit;
                    instance.with_editor(
                        Editor::default()
                            .with_gui_drawer(editor_game_edit)
                            .show_editor_while_running(false),
                    )
                }
                #[cfg(not(feature = "editor"))]
                instance
            }),
    )
    .title("Editable")
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
        context.gui.coords_map_scaling = CoordsMappingScaling::FitVertical(500.0);

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

        context.assets.ensure("font://roboto.ttf").unwrap();

        context
            .assets
            .ensure("animtexture://ferris-bongo.gif")
            .unwrap();
    }

    fn update(&mut self, context: GameContext, _: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(State));
        }
    }
}

struct State;

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        context.assets.ensure("texture://ferris.png").unwrap();
    }

    fn draw(&mut self, context: GameContext) {
        let number = editable!(@edit "number of sprites" => EditableAsText(1_usize));

        for _ in 0..number {
            editable_renderables!(context, {
                let name = editable!("asset name" => EditableAsText("ferris.png".to_string()));
                let position = editable!("position" => EditablePosition(Vec2::new(0.0, 0.0)));
                let rotation = editable!("rotation" => EditableRotation(0.0));
                let scale = editable!("scale" => EditableScale(Vec2::new(1.0, 1.0)));

                Sprite::single(SpriteTexture {
                    sampler: "u_image".into(),
                    texture: TextureRef::name(name),
                    filtering: GlowTextureFiltering::Linear,
                })
                .position(position)
                .rotation(rotation)
                .scale(scale)
                .pivot(0.5.into())
                .draw(context.draw, context.graphics);
            });
        }
    }
}
