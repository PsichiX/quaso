#[cfg(feature = "agent")]
use quaso::agent::AgentServerConfig;
#[cfg(not(feature = "agent"))]
use quaso::commands::GameCommandsHandle;
#[cfg(not(feature = "agent"))]
use quaso::third_party::serde_json::{Value, json};
use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    commands::NoArgs,
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    game_command,
    third_party::{
        raui_core::{
            layout::CoordsMappingScaling,
            widget::{
                component::text_box::TextBoxProps,
                unit::text::{TextBoxFont, TextBoxHorizontalAlign, TextBoxVerticalAlign},
                utils::Color,
            },
        },
        raui_immediate_widgets::core::text_box,
        schemars::JsonSchema,
        serde::Serialize,
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
#[cfg(not(feature = "agent"))]
use std::{thread, time::Duration};

fn main() -> Result<(), Box<dyn Error>> {
    let instance = GameInstance::new(Preloader)
        .setup_assets(|assets| {
            *assets = make_directory_database("./resources/").unwrap();
        })
        .with_command(
            game_command!("game.player" => player_report)
                .title("Read the player")
                .description("Report where the player stands, and how many steps it has taken.")
                .read_only(),
        );

    // With the transport built in, the game waits for a client to drive it from
    // outside. Run the game, then talk to it over the port.
    #[cfg(feature = "agent")]
    let instance = instance.with_agent_server(AgentServerConfig::default());

    // Without the transport there is nothing outside to reach the game, so the
    // example drives itself. The handle must be taken before the launcher
    // consumes the instance.
    #[cfg(not(feature = "agent"))]
    {
        let handle = instance.commands().handle();
        thread::spawn(move || drive(handle));
    }

    GameLauncher::new(instance)
        .title("Agent control")
        .config(Config::load_from_file("./resources/GameConfig.toml")?)
        .run();
    Ok(())
}

const SPEED: f32 = 100.0;

#[derive(Default)]
struct Player {
    position: Vec2<f32>,
    steps: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(crate = "quaso::third_party::serde")]
struct PlayerReport {
    x: f32,
    y: f32,
    steps: usize,
}

fn player_report(context: &mut GameContext, _: NoArgs) -> Result<PlayerReport, String> {
    let player = context
        .globals
        .access::<Player>()
        .ok_or_else(|| "The game has not reached the playable state yet".to_owned())?;
    let player = player.read();
    Ok(PlayerReport {
        x: player.position.x,
        y: player.position.y,
        steps: player.steps,
    })
}

// Drives the game the way an MCP adapter will: every answer comes back on the
// frame phase queue, so nothing here touches game state directly.
#[cfg(not(feature = "agent"))]
fn drive(handle: GameCommandsHandle) {
    let call = |name: &str, arguments: Value| -> Result<Value, String> {
        handle
            .call(name, arguments)?
            .take_timeout(Duration::from_secs(10))?
            .result
    };

    // The game needs a few frames to load its assets and to reach the playable
    // state, and a command that lands too early answers with an error.
    thread::sleep(Duration::from_secs(2));

    let report = |step: &str, result: Result<Value, String>| match result {
        Ok(value) => println!("[agent] {step}: {value}"),
        Err(error) => println!("[agent] {step} FAILED: {error}"),
    };

    match call("commands.list", json!({})) {
        Ok(value) => {
            let commands = value["commands"].as_array().cloned().unwrap_or_default();
            let player = commands
                .iter()
                .find(|command| command["name"] == json!("game.player"));
            println!("[agent] commands.list: {} commands", commands.len());
            match player {
                Some(player) => println!("[agent] game.player descriptor: {player}"),
                None => println!("[agent] game.player FAILED: it is not in the list"),
            }
        }
        Err(error) => println!("[agent] commands.list FAILED: {error}"),
    }

    report("time.pause", call("time.pause", json!({})));
    report("game.player before", call("game.player", json!({})));
    report(
        "unknown argument is refused",
        call("game.player", json!({ "nope": 1 })),
    );
    report(
        "input.press",
        call("input.press", json!({ "action": "key:D", "steps": 10 })),
    );
    report("time.step", call("time.step", json!({ "count": 10 })));

    // A step request larger than `max_fixed_steps_per_frame` needs more than one
    // frame to drain. A query sent before the queue empties reads a half done
    // run, so an agent that steps must wait for `pending_steps` to reach zero.
    for round in 0..100 {
        match call("time.status", json!({})) {
            Ok(value) => {
                if value["pending_steps"] == json!(0) {
                    println!("[agent] the step queue drained after {} polls", round + 1);
                    break;
                }
            }
            Err(error) => {
                println!("[agent] time.status FAILED: {error}");
                break;
            }
        }
    }
    report("game.player after", call("game.player", json!({})));

    match call("render.capture", json!({ "max_size": 320 })) {
        Ok(value) => println!(
            "[agent] render.capture: {} bytes of base64 png",
            value["data"].as_str().map(str::len).unwrap_or_default()
        ),
        Err(error) => println!("[agent] render.capture FAILED: {error}"),
    }

    report("app.close", call("app.close", json!({})));
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
        context.assets.ensure("texture://ferris.png").unwrap();
    }

    fn update(&mut self, context: GameContext, _: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(State::default()));
        }
    }
}

#[derive(Default)]
struct State {
    ferris: Sprite,
    movement: CardinalInputCombinator,
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        context.globals.ensure::<Player>();

        self.ferris = Sprite::single(SpriteTexture {
            sampler: "u_image".into(),
            texture: TextureRef::name("ferris.png"),
            filtering: GlowTextureFiltering::Linear,
        })
        .pivot(0.5.into());

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
                .action(VirtualAction::KeyButton(VirtualKeyCode::A), move_left)
                .action(VirtualAction::KeyButton(VirtualKeyCode::D), move_right)
                .action(VirtualAction::KeyButton(VirtualKeyCode::W), move_up)
                .action(VirtualAction::KeyButton(VirtualKeyCode::S), move_down),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {
        let movement = Vec2::<f32>::from(self.movement.get());
        let mut player = context.globals.ensure::<Player>();
        let mut player = player.write();
        player.position += movement * SPEED * delta_time;
        if movement.magnitude_squared() > 0.0 {
            player.steps += 1;
        }
        self.ferris.transform.position = player.position.into();
    }

    fn draw(&mut self, context: GameContext) {
        self.ferris.draw(context.draw, context.graphics);
    }

    fn draw_gui(&mut self, context: GameContext) {
        let player = context.globals.ensure::<Player>();
        let player = player.read();
        text_box(TextBoxProps {
            text: format!(
                "x: {:.1} y: {:.1} steps: {}",
                player.position.x, player.position.y, player.steps
            ),
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
