use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    game_state_custom_event, inputs_bitstruct,
    multiplayer::{
        GameMultiplayerChange, GameNetwork,
        ggpo::{GgpoMultiplayer, GgpoPlayerCommunication, GgpoPlayerRole},
        tcp::{TcpClientConnection, TcpServerConnection},
        universal::{UniversalMultiplayerAuthority, UniversalMultiplayerGameState},
    },
    third_party::{
        fontdue::layout::{HorizontalAlign, VerticalAlign},
        raui_core::{
            layout::CoordsMappingScaling,
            widget::{
                component::{
                    image_box::ImageBoxProps, interactive::navigation::NavItemActive,
                    text_box::TextBoxProps,
                },
                unit::text::{TextBoxFont, TextBoxHorizontalAlign, TextBoxVerticalAlign},
                utils::Color,
            },
        },
        raui_immediate_widgets::core::{
            containers::{content_box, nav_horizontal_box},
            image_box,
            interactive::{ImmediateButton, button},
            text_box,
        },
        spitfire_draw::{
            context::DrawContext,
            sprite::{Sprite, SpriteTexture},
            text::Text,
            utils::{Drawable, ShaderRef, TextureRef, Vertex},
        },
        spitfire_glow::{
            graphics::{CameraScaling, GraphicsTarget, Shader},
            renderer::GlowTextureFiltering,
        },
        spitfire_input::{
            ArrayInputCombinator, InputActionRef, InputAxisRef, InputConsume, InputMapping,
            MouseButton, VirtualAction, VirtualAxis,
        },
        tehuti::{
            channel::Dispatch,
            hash,
            peer::{Peer, PeerFactory, PeerId, TypedPeerRole},
        },
        tehuti_timeline::time::TimeStamp,
        time::{Duration, Instant},
        tracing::{debug, level_filters::LevelFilter},
        tracing_subscriber::{
            Layer, fmt::layer, layer::SubscriberExt, registry, util::SubscriberInitExt,
        },
        vek::{Rgba, Vec2},
        windowing::event::VirtualKeyCode,
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, error::Error, sync::Arc};
use tehuti::fixed::Fixed;

type Number = Fixed<6>;

const ADDRESS: &str = "127.0.0.1:12345";
const PLAYER_ROLE: u64 = 1;
const HISTORY_CAPACITY: usize = 16;
const SEND_STATE_HASH_INTERVAL: Duration = Duration::from_millis(100);
const SEND_INPUT_WINDOW: u64 = 8;
const SEND_STATE_HASH_WINDOW: u64 = 8;
const INPUT_DELAY_TICKS: u64 = 3;
const MAX_PREDICTION_TICKS: u64 = 6;
const SPEED: f32 = 100.0;
const COLOR_WHITE: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
const COLOR_BLACK: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

type PlayerRole = GgpoPlayerRole<PLAYER_ROLE, HISTORY_CAPACITY, InputSnapshot, StateSnapshot>;
type Multiplayer = GgpoMultiplayer;

fn main() -> Result<(), Box<dyn Error>> {
    registry()
        .with(
            layer()
                .with_writer(std::io::stdout)
                .with_filter(LevelFilter::DEBUG),
        )
        .init();

    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Netcode: GGPO (P2P rollback)")
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
        context.assets.ensure("texture://ferris.png").unwrap();

        *context.network =
            GameNetwork::new(Arc::new(PeerFactory::default().with_typed::<PlayerRole>()));
    }

    fn update(&mut self, context: GameContext, _: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(Lobby::default()));
        }
    }
}

#[derive(Default)]
struct Lobby {
    exit: InputActionRef,
}

impl GameState for Lobby {
    fn enter(&mut self, context: GameContext) {
        let pointer_x = InputAxisRef::default();
        let pointer_y = InputAxisRef::default();
        let pointer_trigger = InputActionRef::default();
        self.exit = InputActionRef::default();

        context.gui.interactions.inputs.pointer_position =
            ArrayInputCombinator::new([pointer_x.clone(), pointer_y.clone()]);
        context.gui.interactions.inputs.pointer_trigger = pointer_trigger.clone();

        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .axis(VirtualAxis::MousePositionX, pointer_x)
                .axis(VirtualAxis::MousePositionY, pointer_y)
                .action(
                    VirtualAction::MouseButton(MouseButton::Left),
                    pointer_trigger,
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Escape),
                    self.exit.clone(),
                ),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn update(&mut self, context: GameContext, _delta_time: f32) {
        if let Some(multiplayer) = context.multiplayer::<Multiplayer>()
            && multiplayer.peers_in_queue().count() == 2
        {
            debug!("All players connected, starting game...");
            *context.state_change = GameStateChange::Swap(Box::new(State::default()));
        }
    }

    fn fixed_update(&mut self, context: GameContext, _delta_time: f32) {
        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw_gui(&mut self, context: GameContext) {
        nav_horizontal_box(NavItemActive, || {
            if let Some(multiplayer) = context.multiplayer::<Multiplayer>() {
                text_box(TextBoxProps {
                    text: format!(
                        "WAITING FOR PLAYERS... ({}/2)",
                        multiplayer.peers_in_queue().count()
                    ),
                    font: TextBoxFont {
                        name: "roboto.ttf".to_owned(),
                        size: 24.0,
                    },
                    color: COLOR_WHITE,
                    horizontal_align: TextBoxHorizontalAlign::Center,
                    vertical_align: TextBoxVerticalAlign::Middle,
                    ..Default::default()
                });
            } else {
                let host = lobby_button("HOST");
                let join = lobby_button("JOIN");

                let is_host = if host.trigger_start() {
                    Some(true)
                } else if join.trigger_start() {
                    Some(false)
                } else {
                    None
                };

                if let Some(is_host) = is_host {
                    debug!("Starting {}...", if is_host { "server" } else { "client" });

                    context.network.clear_connections();
                    let id = if is_host {
                        context
                            .network
                            .add_connection(TcpServerConnection::listen(ADDRESS).unwrap());
                        PeerId::new(0)
                    } else {
                        context
                            .network
                            .add_connection(TcpClientConnection::connect(ADDRESS).unwrap());
                        PeerId::new(1)
                    };

                    let multiplayer = Multiplayer::new(context.network);
                    multiplayer.create_peer(id, PlayerRole::ROLE_ID);
                    *context.multiplayer_change = GameMultiplayerChange::Set(Box::new(
                        multiplayer
                            .with_ticks_per_second(30)
                            .with_max_prediction_ticks(MAX_PREDICTION_TICKS),
                    ));
                }
            }
        });
    }
}

fn lobby_button(label: &str) -> ImmediateButton {
    button(NavItemActive, |state| {
        let (bg_color, text_color) = if state.state.selected {
            (COLOR_WHITE, COLOR_BLACK)
        } else {
            (COLOR_BLACK, COLOR_WHITE)
        };

        content_box((), || {
            image_box(ImageBoxProps::colored(bg_color));

            text_box(TextBoxProps {
                text: label.to_owned(),
                font: TextBoxFont {
                    name: "roboto.ttf".to_owned(),
                    size: 24.0,
                },
                color: text_color,
                horizontal_align: TextBoxHorizontalAlign::Center,
                vertical_align: TextBoxVerticalAlign::Middle,
                ..Default::default()
            });
        });
    })
}

struct State {
    players: BTreeMap<PeerId, PlayerCharacter>,
    move_up: InputActionRef,
    move_down: InputActionRef,
    move_left: InputActionRef,
    move_right: InputActionRef,
    exit: InputActionRef,
    send_state_timer: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self {
            players: Default::default(),
            move_up: Default::default(),
            move_down: Default::default(),
            move_left: Default::default(),
            move_right: Default::default(),
            exit: Default::default(),
            send_state_timer: Instant::now(),
        }
    }
}

impl State {
    fn local_player_mut(&mut self) -> Option<&mut PlayerCharacter> {
        self.players
            .values_mut()
            .find(|player| !player.role.info.remote)
    }
}

impl GameState for State {
    fn enter(&mut self, mut context: GameContext) {
        self.move_left = InputActionRef::default();
        self.move_right = InputActionRef::default();
        self.move_up = InputActionRef::default();
        self.move_down = InputActionRef::default();
        self.exit = InputActionRef::default();
        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::A),
                    self.move_left.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::D),
                    self.move_right.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::W),
                    self.move_up.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::S),
                    self.move_down.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Left),
                    self.move_left.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Right),
                    self.move_right.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Up),
                    self.move_up.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Down),
                    self.move_down.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Escape),
                    self.exit.clone(),
                ),
        );

        if let Some(multiplayer) = context.multiplayer_mut::<Multiplayer>() {
            multiplayer.request_progressing(true);
        } else {
            *context.state_change = GameStateChange::Swap(Box::new(Lobby::default()));
        }
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();

        context.network.clear_connections();
        *context.multiplayer_change = GameMultiplayerChange::Reset;
    }

    fn fixed_update(&mut self, context: GameContext, _delta_time: f32) {
        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Swap(Box::new(Lobby::default()));
        }
    }

    fn draw(&mut self, context: GameContext) {
        for player in self.players.values() {
            player.draw(context.draw, context.graphics);
        }
    }

    fn multiplayer_peer_added(&mut self, _context: GameContext, peer: Peer) {
        let player = peer.into_typed::<PlayerRole>().unwrap();
        self.players
            .insert(player.info.peer_id, PlayerCharacter::new(player));
    }

    fn multiplayer_peer_removed(&mut self, _context: GameContext, peer_id: PeerId) {
        self.players.remove(&peer_id);
    }

    game_state_custom_event! {
        trait(UniversalMultiplayerGameState)
    }
}

impl UniversalMultiplayerGameState for State {
    fn on_stop(&mut self, context: GameContext, _current_tick: TimeStamp) {
        *context.state_change = GameStateChange::Swap(Box::new(Lobby::default()));
    }

    fn prepare_frame(&mut self, _context: GameContext, current_tick: TimeStamp) {
        for player in self.players.values_mut() {
            player
                .role
                .input_history
                .ensure_timestamp(current_tick, Default::default);
            player
                .role
                .state_history
                .ensure_timestamp(current_tick, Default::default);
            player
                .role
                .state_hash_history
                .ensure_timestamp(current_tick, Default::default);
        }
    }

    fn time_travel(&mut self, _context: GameContext, target_tick: TimeStamp) {
        for player in self.players.values_mut() {
            player.role.input_history.time_travel_to(target_tick + 1);
            player.role.state_history.time_travel_to(target_tick);
            player.role.state_hash_history.time_travel_to(target_tick);
        }
    }

    fn handle_inputs(&mut self, _context: GameContext, current_tick: TimeStamp) {
        let input_tick = current_tick + INPUT_DELAY_TICKS;
        let input = InputSnapshot::default()
            .with_left(self.move_left.get().is_down())
            .with_right(self.move_right.get().is_down())
            .with_up(self.move_up.get().is_down())
            .with_down(self.move_down.get().is_down());

        if let Some(player) = self.local_player_mut() {
            let GgpoPlayerCommunication::Local { input_sender, .. } =
                &mut player.role.communication
            else {
                return;
            };

            player.role.input_history.set(input_tick, input);
            let since = current_tick - SEND_INPUT_WINDOW;
            if let Some(event) = player
                .role
                .input_history
                .collect_history(since..=input_tick)
            {
                input_sender.send(event.into()).ok();
            }
        }
    }

    fn find_input_divergence(
        &mut self,
        _context: GameContext,
        _current_tick: TimeStamp,
        _authority: UniversalMultiplayerAuthority,
    ) -> Option<TimeStamp> {
        let mut divergence = None;

        for player in self.players.values_mut() {
            if let GgpoPlayerCommunication::Remote { input_receiver, .. } =
                &player.role.communication
            {
                for Dispatch { message, .. } in input_receiver.iter() {
                    player.confirmed_tick = player.confirmed_tick.max(message.now());
                    let div = player
                        .role
                        .input_history
                        .apply_history_divergence(&message)
                        .unwrap();
                    divergence = TimeStamp::possibly_oldest(divergence, div);
                }
            }
        }

        divergence
    }

    fn min_confirmed_tick(&mut self, _context: GameContext, current_tick: TimeStamp) -> TimeStamp {
        self.players
            .values()
            .filter(|player| player.role.info.remote)
            .map(|player| player.confirmed_tick)
            .min()
            .unwrap_or(current_tick)
    }

    fn detect_state_desync(&mut self, _context: GameContext, current_tick: TimeStamp) -> bool {
        let mut desync = false;
        let since = current_tick - SEND_STATE_HASH_WINDOW;
        for player in self.players.values_mut() {
            if self.send_state_timer.elapsed() >= SEND_STATE_HASH_INTERVAL
                && let GgpoPlayerCommunication::Local {
                    state_hash_sender, ..
                } = &player.role.communication
                && let Some(event) = player
                    .role
                    .state_hash_history
                    .collect_history(since..=current_tick)
            {
                self.send_state_timer = Instant::now();
                state_hash_sender.send(event.into()).ok();
            }

            if let GgpoPlayerCommunication::Remote {
                state_hash_receiver,
                ..
            } = &player.role.communication
            {
                for Dispatch { message, .. } in state_hash_receiver.iter() {
                    for (tick, hash) in message.iter() {
                        if let Some(local_hash) = player.role.state_hash_history.get(tick).copied()
                            && *hash != local_hash
                        {
                            debug!(
                                "State hash mismatch at tick {:?}: local = {}, remote = {}",
                                tick, local_hash, hash
                            );
                            desync = true;
                            break;
                        }
                    }
                }
            }
        }

        desync
    }

    fn tick(
        &mut self,
        _context: GameContext,
        current_tick: TimeStamp,
        delta_time: f32,
        _resimulating: bool,
    ) {
        let delta_time = Number::from_f32(delta_time);
        let prev_tick = current_tick - 1;

        for player in self.players.values_mut() {
            let input = player
                .role
                .input_history
                .get_extrapolated(current_tick)
                .copied()
                .unwrap_or_default();
            let mut state = player
                .role
                .state_history
                .get_extrapolated(prev_tick)
                .copied()
                .unwrap_or_default();

            state.velocity_x = match (input.left(), input.right()) {
                (true, false) => Number::from_f32(-SPEED),
                (false, true) => Number::from_f32(SPEED),
                _ => Number::from_f32(0.0),
            };
            state.velocity_y = match (input.up(), input.down()) {
                (true, false) => Number::from_f32(-SPEED),
                (false, true) => Number::from_f32(SPEED),
                _ => Number::from_f32(0.0),
            };

            state.position_x += state.velocity_x * delta_time;
            state.position_y += state.velocity_y * delta_time;

            player.role.input_history.set(current_tick, input);
            player.role.state_history.set(current_tick, state);
            player
                .role
                .state_hash_history
                .set(current_tick, hash(&state));
        }
    }

    fn commit_frame(&mut self, _context: GameContext, current_tick: TimeStamp) {
        for player in self.players.values_mut() {
            let state = player
                .role
                .state_history
                .get_extrapolated(current_tick)
                .copied()
                .unwrap_or_default();
            player.sprite.transform.position.x = state.position_x.into_f32();
            player.sprite.transform.position.y = state.position_y.into_f32();
        }
    }
}

// Inputs snapshot should only contain input down states, from which simulation
// will deduce detailed changes between consecutive ticks.
inputs_bitstruct! {
    #[repr(transparent)]
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    struct InputSnapshot(u8) {
        left: 0,
        right: 1,
        up: 2,
        down: 3,
    }
}

// State snapshots should store information required for simulation evolution,
// so not only positions, but also velocities for example.
// If we will only send positions, players would not be able to predict
// their movement!
#[derive(Debug, Default, Clone, Copy, PartialEq, Hash)]
struct StateSnapshot {
    position_x: Number,
    position_y: Number,
    velocity_x: Number,
    velocity_y: Number,
}

struct PlayerCharacter {
    role: PlayerRole,
    confirmed_tick: TimeStamp,
    sprite: Sprite,
}

impl PlayerCharacter {
    fn new(role: PlayerRole) -> Self {
        Self {
            role,
            confirmed_tick: TimeStamp::default(),
            sprite: Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name("ferris.png"),
                filtering: GlowTextureFiltering::Linear,
            })
            .pivot(0.5.into())
            .scale(0.25.into()),
        }
    }
}

impl Drawable for PlayerCharacter {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
        self.sprite.draw(context, graphics);

        Text::new(ShaderRef::name("text"))
            .text(self.role.info.peer_id.id().to_string())
            .font("roboto.ttf")
            .size(20.0)
            .position(Vec2::from(self.sprite.transform.position) - Vec2::new(0.0, 60.0))
            .tint(if self.role.info.remote {
                Rgba::red()
            } else {
                Rgba::white()
            })
            .horizontal_align(HorizontalAlign::Center)
            .vertical_align(VerticalAlign::Bottom)
            .draw(context, graphics);
    }
}
