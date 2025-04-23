use intuicio_frontend_simpleton::Real;
use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    script_contents,
    scripting::{call_object, create_host, get, new_init, new_typed, set},
    third_party::{
        intuicio_frontend_simpleton::Reference,
        raui_core::layout::CoordsMappingScaling,
        raui_immediate_widgets::core::{
            Color, TextBoxFont, TextBoxHorizontalAlign, TextBoxProps, TextBoxVerticalAlign,
            text_box,
        },
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, TextureRef},
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::GlowTextureFiltering,
        },
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        vek::Vec2,
        windowing::event::VirtualKeyCode,
    },
};
use std::error::Error;

// We define scripts by including them into application in constant,
// to make it work also for web builds.
script_contents!(SCRIPTS => "../resources/player.simp");

const SPEED: f32 = 100.0;

fn main() -> Result<(), Box<dyn Error>> {
    // Create global scriptable host from script contents.
    create_host(Default::default(), SCRIPTS, []);

    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Scripting")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

#[derive(Default)]
struct GameObject {
    pub sprite: Sprite,
    // Here we store object that holds scriptable state for this game object.
    pub script_object: Reference,
}

impl Drop for GameObject {
    fn drop(&mut self) {
        self.sync_state_to_script();
        call_object(self.script_object.clone(), "on_destroy", &[]);
        self.sync_script_to_state();
    }
}

impl GameObject {
    fn new(sprite: Sprite, speed: f32, type_name: &str, module_name: &str) -> Self {
        let mut result = Self {
            sprite,
            // Create object by type name and module name for struct from script side.
            script_object: new_init(
                type_name,
                module_name,
                &[("speed", new_typed(speed as Real))],
            ),
        };
        result.sync_state_to_script();
        // Having scriptable state object, we can call function on it
        // based on struct module name.
        call_object(result.script_object.clone(), "on_create", &[]);
        result.sync_script_to_state();
        result
    }

    fn sync_state_to_script(&self) {
        // We should synchronize state between game and script side before we
        // do changes in scripts - otherwise game can't see script changes.
        if !self.script_object.is_null() {
            set(
                self.script_object.clone(),
                "x",
                new_typed(self.sprite.transform.position.x as Real),
            );
            set(
                self.script_object.clone(),
                "y",
                new_typed(self.sprite.transform.position.y as Real),
            );
        }
    }

    fn sync_script_to_state(&mut self) {
        if !self.script_object.is_null() {
            self.sprite.transform.position.x =
                *get(self.script_object.clone(), "x").read::<Real>().unwrap() as f32;
            self.sprite.transform.position.y =
                *get(self.script_object.clone(), "y").read::<Real>().unwrap() as f32;
        }
    }

    fn on_update(&mut self, delta_time: f32, movement: &CardinalInputCombinator) {
        self.sync_state_to_script();
        let movement = Vec2::<f32>::from(movement.get());
        call_object(
            self.script_object.clone(),
            "on_update",
            &[
                new_typed(delta_time as Real),
                new_typed(movement.x as Real),
                new_typed(movement.y as Real),
            ],
        );
        self.sync_script_to_state();
    }
}

#[derive(Default)]
struct Preloader;

impl GameState for Preloader {
    fn enter(&mut self, context: GameContext) {
        context.graphics.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.main_camera.screen_alignment = 0.5.into();
        context.graphics.main_camera.scaling = CameraScaling::FitVertical(500.0);
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

        context.assets.ensure("texture://ferris.png").unwrap();

        context.assets.ensure("font://roboto.ttf").unwrap();

        *context.state_change = GameStateChange::Swap(Box::new(State::default()));
    }
}

#[derive(Default)]
struct State {
    ferris: GameObject,
    movement: CardinalInputCombinator,
    exit: InputActionRef,
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        self.ferris = GameObject::new(
            Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name("ferris.png"),
                filtering: GlowTextureFiltering::Linear,
            })
            .pivot(0.5.into()),
            SPEED,
            "Player",
            "player",
        );

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
                .action(VirtualAction::KeyButton(VirtualKeyCode::Down), move_down)
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Escape),
                    self.exit.clone(),
                ),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {
        self.ferris.on_update(delta_time, &self.movement);

        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw(&mut self, context: GameContext) {
        self.ferris.sprite.draw(context.draw, context.graphics);
    }

    fn draw_gui(&mut self, _: GameContext) {
        text_box(TextBoxProps {
            text: "Simpleton scripting".to_owned(),
            horizontal_align: TextBoxHorizontalAlign::Center,
            vertical_align: TextBoxVerticalAlign::Bottom,
            font: TextBoxFont {
                name: "roboto.ttf".to_owned(),
                size: 50.0,
            },
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            ..Default::default()
        });
    }
}
