// A game object defined in script and driven by the host.
//
// The script declares `Player`, with its own data and its own `update` and
// `draw`. This game state holds the value `Player::create` returned and calls
// the matching method when the matching event happens. So the host owns the
// lifecycle and the script owns the object.

use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    gc::DynGc,
    scripting::{
        AuriValue, AuriValueTransformer, HOST_MODULE, ScriptObject, Scripting, ValueTransformer,
    },
    third_party::{
        intuicio_core::{
            context::Context,
            function::{Function, FunctionBody, FunctionParameter, FunctionSignature},
            registry::Registry,
        },
        intuicio_derive::intuicio_function,
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
    fn enter(&mut self, mut context: GameContext) {
        if let Some(scripting) = context.scripting.as_mut() {
            scripting.add_setup(install_host_functions);
        }

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

    // The subsystem collects loaded scripts after this state updates, so the
    // registry is built here, once every file is in.
    fn exit(&mut self, mut context: GameContext) {
        if let Some(scripting) = context.scripting.as_mut() {
            scripting.reinstall();
        }
    }
}

#[derive(Default)]
struct State {
    player: Option<ScriptObject>,
    greeting: String,
    exit: InputActionRef,
}

impl GameState for State {
    fn enter(&mut self, mut context: GameContext) {
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

        context.with_scripting(|scripting, context| {
            // The script builds the object and hands the object back. Nothing
            // on this side knows what a `Player` holds.
            self.player = Some(
                ScriptObject::create(scripting, context, "player", "Player", |call| {
                    call.value("ferris.png")
                })
                .unwrap(),
            );

            // A plain function call, to show a script value coming back to
            // Rust. `value` turns the `&str` into the text auri holds, so
            // nothing here builds a handle by hand.
            let greeting = scripting
                .call_function("utils", "greet")
                .and_then(|call| call.value("auri").run(context))
                .and_then(|results| results.value());
            if let Ok(AuriValue::Text(text)) = greeting {
                self.greeting = text;
            }
        });
    }

    fn exit(&mut self, mut context: GameContext) {
        context.input.pop_mapping();

        if let Some(mut player) = self.player.take() {
            context.with_scripting(|scripting, context| {
                player.destroy(scripting, context).unwrap();
            });
        }
    }

    fn fixed_update(&mut self, mut context: GameContext, delta_time: f32) {
        // `value` widens the `f32` into the `f64` auri does arithmetic on, so
        // the script never meets a number its operators do not take.
        if let Some(player) = self.player.as_mut() {
            context.with_scripting(|scripting, context| {
                if let Some(call) = player.call(scripting, "update") {
                    call.value(delta_time).run(context).unwrap();
                }
            });
        }

        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw(&mut self, mut context: GameContext) {
        if let Some(player) = self.player.as_mut() {
            context.with_scripting(|scripting, context| {
                if let Some(call) = player.call(scripting, "draw") {
                    call.run(context).unwrap();
                }
            });
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
fn install_host_functions(registry: &mut Registry) {
    registry.add_function(print::define_function(registry));
    registry.add_function(draw_sprite::define_function(registry));
    registry.add_function(input_down::define_function(registry));

    // `text` takes whatever the script gives it, so it has no Rust type to name and
    // the derive macro cannot describe it. A host function like that is written by
    // hand. Every auri value is one gc managed value, which is what `pop_gc` and
    // `push_gc` move.
    let gc = Scripting::gc_type(registry);
    registry.add_function(Function::new(
        FunctionSignature::new("text")
            .with_module_name(HOST_MODULE)
            .with_input(FunctionParameter::new("value", gc.clone()))
            .with_output(FunctionParameter::new("result", gc)),
        FunctionBody::closure(|context: &mut Context, _: &Registry| {
            let value = Scripting::pop_gc(context)
                .expect("`text` got a stack value that is not gc managed!");
            Scripting::push_gc(context, DynGc::new(Scripting::describe(&value)));
        }),
    ));
}

// `AuriValueTransformer` is what lets the derive macro write the stack work.
// Each argument arrives as a read guard on the script's own value, and the
// result is wrapped on the way out. `-> ()` hands the script the unit, which is
// what a function with nothing to say returns.
#[intuicio_function(module_name = "host", transformer = "AuriValueTransformer")]
#[allow(clippy::unused_unit)]
fn print(value: &String) -> () {
    println!("{value}");
}

// `use_context` adds the script context as a first argument. That is what
// `Scripting::with_game` needs to reach the running game.
#[intuicio_function(
    module_name = "host",
    transformer = "AuriValueTransformer",
    use_context
)]
#[allow(clippy::unused_unit)]
fn draw_sprite(context: &mut Context, texture: &String, x: &f64, y: &f64) -> () {
    Scripting::with_game(context, "draw_sprite", |game| {
        Sprite::single(SpriteTexture {
            sampler: "u_image".into(),
            texture: TextureRef::name(texture.to_owned()),
            filtering: GlowTextureFiltering::Linear,
        })
        .pivot(0.5.into())
        .position(Vec2::new(*x as f32, *y as f32))
        .draw(game.draw, game.graphics);
    });
}

#[intuicio_function(
    module_name = "host",
    transformer = "AuriValueTransformer",
    use_context
)]
fn input_down(context: &mut Context, name: &String) -> bool {
    Scripting::with_game(context, "input_down", |game| {
        game.globals
            .access::<ScriptInputs>()
            .and_then(|inputs| {
                inputs
                    .read()
                    .0
                    .get(name)
                    .map(|action| action.get().is_down())
            })
            .unwrap_or(false)
    })
}

// A script cannot build an input mapping, so the game builds the mapping and
// puts the actions where a host function can find the actions.
#[derive(Default)]
struct ScriptInputs(HashMap<String, InputActionRef>);
