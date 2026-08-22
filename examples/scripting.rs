// A game object defined in script and driven by the host.
//
// The script declares `Player`, with its own data and its own `update` and
// `draw`. This game state holds the value `Player::create` returned and calls
// the matching method when the matching event happens. So the host owns the
// lifecycle and the script owns the object.

use quaso::{
    GameLauncher,
    assets::{auri::AuriAsset, make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    scripting::{ScriptObject, Scripting},
    third_party::{
        intuicio_core::registry::Registry,
        intuicio_data::managed::gc::DynamicManagedGc,
        raui_core::widget::{
            component::text_box::TextBoxProps,
            unit::text::{TextBoxFont, TextBoxHorizontalAlign, TextBoxVerticalAlign},
            utils::Color,
        },
        raui_immediate_widgets::core::text_box,
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
};
use std::{collections::HashMap, error::Error};

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Scripting")
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
        context.assets.ensure("texture://ferris.png").unwrap();
        context.assets.ensure("auri://game.auri").unwrap();
    }

    fn update(&mut self, context: GameContext, _: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(State::default()));
        }
    }
}

#[derive(Default)]
struct State {
    scripting: Option<Scripting>,
    player: Option<ScriptObject>,
    greeting: String,
    exit: InputActionRef,
}

impl GameState for State {
    fn enter(&mut self, mut context: GameContext) {
        let file = context
            .assets
            .ensure("auri://game.auri")
            .unwrap()
            .access::<&AuriAsset>(context.assets)
            .file
            .clone();
        let mut scripting = Scripting::default();
        install_host_functions(scripting.registry_mut());
        scripting.install([("game.auri".to_owned(), file)]);

        // The script asks for actions by name, so the mapping is built here and
        // the actions are put where the host functions can find the actions.
        let left = InputActionRef::default();
        let right = InputActionRef::default();
        let up = InputActionRef::default();
        let down = InputActionRef::default();
        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .action(VirtualAction::KeyButton(VirtualKeyCode::A), left.clone())
                .action(VirtualAction::KeyButton(VirtualKeyCode::D), right.clone())
                .action(VirtualAction::KeyButton(VirtualKeyCode::W), up.clone())
                .action(VirtualAction::KeyButton(VirtualKeyCode::S), down.clone())
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Escape),
                    self.exit.clone(),
                ),
        );
        context.globals.set(ScriptInputs(HashMap::from([
            ("left".to_owned(), left),
            ("right".to_owned(), right),
            ("up".to_owned(), up),
            ("down".to_owned(), down),
        ])));

        // The script builds the object and hands the object back. Nothing on
        // this side knows what a `Player` holds.
        self.player = ScriptObject::create(&mut scripting, &mut context, "game", "Player", []);

        // A plain function call, to show a script value coming back to Rust.
        if let Some(function) = scripting.find("game", "greet") {
            let result = scripting.call(
                &mut context,
                &function,
                [DynamicManagedGc::new("auri".to_owned())],
            );
            if let Some(result) = result
                && result.is::<String>()
            {
                self.greeting = result.read::<true, String>().to_owned();
            }
        }

        self.scripting = Some(scripting);
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, mut context: GameContext, delta_time: f32) {
        if let (Some(scripting), Some(player)) = (self.scripting.as_mut(), self.player.as_mut()) {
            player.call(
                scripting,
                &mut context,
                "update",
                [DynamicManagedGc::new(delta_time as f64)],
            );
        }

        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw(&mut self, mut context: GameContext) {
        if let (Some(scripting), Some(player)) = (self.scripting.as_mut(), self.player.as_mut()) {
            player.call(scripting, &mut context, "draw", []);
        }
    }

    fn draw_gui(&mut self, _: GameContext) {
        text_box(TextBoxProps {
            text: self.greeting.to_owned(),
            horizontal_align: TextBoxHorizontalAlign::Center,
            vertical_align: TextBoxVerticalAlign::Bottom,
            font: TextBoxFont {
                name: "roboto.ttf".to_owned(),
                size: 40.0,
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

// Quaso registers no host functions of its own, so what a script may do is
// entirely this game's choice. These four are what `resources/game.auri` needs.
//
// `add_host_function` takes how many arguments the function has, because every
// auri value is a `DynamicManagedGc` and a signature has nothing else to say.
// The body pops the arguments left to right and returns the one result.
fn install_host_functions(registry: &mut Registry) {
    Scripting::add_host_function(registry, "print", 1, |context, _| {
        let value = Scripting::pop_argument(context, "print");
        println!("{}", Scripting::describe(&value));
        DynamicManagedGc::new(())
    });

    Scripting::add_host_function(registry, "text", 1, |context, _| {
        let value = Scripting::pop_argument(context, "text");
        DynamicManagedGc::new(Scripting::describe(&value))
    });

    // `with_game` is what reaches the running game from inside a script call.
    Scripting::add_host_function(registry, "draw_sprite", 3, |context, _| {
        let texture = Scripting::text_of(
            &Scripting::pop_argument(context, "draw_sprite"),
            "draw_sprite",
        );
        let x = Scripting::number(
            &Scripting::pop_argument(context, "draw_sprite"),
            "draw_sprite",
        ) as f32;
        let y = Scripting::number(
            &Scripting::pop_argument(context, "draw_sprite"),
            "draw_sprite",
        ) as f32;
        Scripting::with_game(context, "draw_sprite", |game| {
            Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name(texture),
                filtering: GlowTextureFiltering::Linear,
            })
            .pivot(0.5.into())
            .position(Vec2::new(x, y))
            .draw(game.draw, game.graphics);
        });
        DynamicManagedGc::new(())
    });

    Scripting::add_host_function(registry, "input_down", 1, |context, _| {
        let name = Scripting::text_of(
            &Scripting::pop_argument(context, "input_down"),
            "input_down",
        );
        let down = Scripting::with_game(context, "input_down", |game| {
            game.globals
                .access::<ScriptInputs>()
                .and_then(|inputs| {
                    inputs
                        .read()
                        .0
                        .get(&name)
                        .map(|action| action.get().is_down())
                })
                .unwrap_or(false)
        });
        DynamicManagedGc::new(down)
    });
}

// A script cannot build an input mapping, so the game builds the mapping and
// puts the actions where a host function can find the actions.
#[derive(Default)]
struct ScriptInputs(HashMap<String, InputActionRef>);
