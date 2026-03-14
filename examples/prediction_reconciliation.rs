use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    game_state_custom_event, inputs_bitstruct,
    multiplayer::{
        GameMultiplayerChange, GameNetwork,
        csp_ssr::{CspSsrAuthority, CspSsrGameState, CspSsrMultiplayer, CspSsrPlayerRole},
        tcp::{TcpClientConnection, TcpServerConnection},
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
            peer::{Peer, PeerId, TypedPeerRole},
            replication::primitives::RepF32,
        },
        tehuti_timeline::{history::HistoryEvent, time::TimeStamp},
        time::Duration,
        tracing::{debug, level_filters::LevelFilter},
        tracing_subscriber::{
            Layer, fmt::layer, layer::SubscriberExt, registry, util::SubscriberInitExt,
        },
        vek::{Rgba, Vec2},
        windowing::event::VirtualKeyCode,
    },
};
use serde::{Deserialize, Serialize};
use std::{any::Any, collections::HashMap, error::Error, sync::Arc};

const ADDRESS: &str = "127.0.0.1:12345";
const AUTHORITY_CLOCK_CHANNEL: u64 = 10;
const PLAYER_ROLE: u64 = 1;
const HISTORY_CAPACITY: usize = 16;
const SERVER_SEND_STATE_INTERVAL: Duration = Duration::from_millis(1000 / 30);
const CLIENT_SEND_INPUT_WINDOW: u64 = 4;
const CLIENT_LEAD_TICKS: u64 = 2;
const CLIENT_PING_INTERVAL: Duration = Duration::from_millis(250);
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

type PlayerRole = CspSsrPlayerRole<PLAYER_ROLE, HISTORY_CAPACITY, InputSnapshot, StateSnapshot>;
type Authority = CspSsrAuthority<AUTHORITY_CLOCK_CHANNEL>;
type Multiplayer = CspSsrMultiplayer<AUTHORITY_CLOCK_CHANNEL>;

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
    .title("Netcode: Client-side prediction and server-side reconciliation")
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
            && multiplayer.is_initialized()
        {
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
            if context.multiplayer::<Multiplayer>().is_some() {
                text_box(TextBoxProps {
                    text: "ESTABLISHING CONNECTION...".to_owned(),
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

                let is_server = if host.trigger_start() {
                    Some(true)
                } else if join.trigger_start() {
                    Some(false)
                } else {
                    None
                };

                if let Some(is_server) = is_server {
                    debug!(
                        "Starting {}...",
                        if is_server { "server" } else { "client" }
                    );

                    context.network.clear_connections();
                    let mut network = GameNetwork::new(Arc::new(
                        Authority::peer_factory(is_server)
                            .unwrap()
                            .with_typed::<PlayerRole>(),
                    ));
                    if is_server {
                        network.add_connection(TcpServerConnection::listen(ADDRESS).unwrap());
                    } else {
                        network.add_connection(TcpClientConnection::connect(ADDRESS).unwrap());
                    }
                    if let Some(multiplayer) = Multiplayer::new(&network) {
                        debug!("{} started", if is_server { "Server" } else { "Client" });

                        *context.network = network;
                        *context.multiplayer_change = GameMultiplayerChange::Set(Box::new(
                            multiplayer
                                .with_ticks_per_second(30)
                                .with_server_send_state_interval(SERVER_SEND_STATE_INTERVAL)
                                .with_client_lead_ticks(CLIENT_LEAD_TICKS)
                                .with_client_ping_interval(CLIENT_PING_INTERVAL),
                        ));
                    }
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

#[derive(Default)]
struct State {
    players: HashMap<PeerId, PlayerCharacter>,
    move_up: InputActionRef,
    move_down: InputActionRef,
    move_left: InputActionRef,
    move_right: InputActionRef,
    exit: InputActionRef,
}

impl State {
    fn local_player_mut(&mut self) -> Option<&mut PlayerCharacter> {
        self.players
            .values_mut()
            .find(|player| !player.role.info().remote)
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
            multiplayer.process_lifecycle_events = true;
            let _ = multiplayer.authority.create_peer(PlayerRole::ROLE_ID);
        } else {
            *context.state_change = GameStateChange::Swap(Box::new(Lobby::default()));
        }
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();

        *context.network = GameNetwork::default();
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
            .insert(player.info().peer_id, PlayerCharacter::new(player));
    }

    fn multiplayer_peer_removed(&mut self, _context: GameContext, peer_id: PeerId) {
        self.players.remove(&peer_id);
    }

    game_state_custom_event! {
        trait(CspSsrGameState)
    }
}

impl CspSsrGameState for State {
    fn prepare_frame(&mut self, _context: GameContext, current_tick: TimeStamp) {
        for player in self.players.values_mut() {
            if let Some(inputs) = player.role.inputs_mut() {
                inputs.ensure_timestamp(current_tick, Default::default);
            }
            player
                .role
                .state_mut()
                .ensure_timestamp(current_tick, Default::default);
        }
    }

    fn time_travel(&mut self, _context: GameContext, target_tick: TimeStamp) {
        for player in self.players.values_mut() {
            if let Some(inputs) = player.role.inputs_mut() {
                inputs.time_travel_to(target_tick);
            }
            player.role.state_mut().time_travel_to(target_tick);
        }
    }

    fn handle_inputs(&mut self, _context: GameContext, current_tick: TimeStamp) {
        let input = InputSnapshot::default()
            .with_left(self.move_left.get().is_down())
            .with_right(self.move_right.get().is_down())
            .with_up(self.move_up.get().is_down())
            .with_down(self.move_down.get().is_down());

        if let Some(player) = self.local_player_mut() {
            match &mut player.role {
                PlayerRole::ServerLocal { input_history, .. } => {
                    input_history.set(current_tick, input);
                }
                PlayerRole::ClientLocal {
                    input_sender,
                    input_history,
                    ..
                } => {
                    input_history.set(current_tick, input);
                    let since = current_tick - CLIENT_SEND_INPUT_WINDOW;
                    if let Some(event) =
                        HistoryEvent::collect_history(input_history, since..=current_tick)
                    {
                        input_sender.send(event.into()).ok();
                    }
                }
                _ => {}
            }
        }
    }

    fn server_find_input_divergence(
        &mut self,
        _context: GameContext,
        _current_tick: TimeStamp,
    ) -> Option<TimeStamp> {
        let mut divergence = None;

        for player in self.players.values_mut() {
            if let PlayerRole::ServerRemote {
                input_receiver,
                input_history,
                ..
            } = &mut player.role
                && let Some(Dispatch { message, .. }) = input_receiver.last()
            {
                match message.apply_history_divergence(input_history) {
                    Ok(div) => {
                        divergence = TimeStamp::possibly_oldest(divergence, div);
                    }
                    Err(error) => debug!(
                        "Failed to apply input history divergence for player {}: {}",
                        player.role.info().peer_id,
                        error
                    ),
                }
            }
        }

        divergence
    }

    fn client_find_state_divergence(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
    ) -> Option<TimeStamp> {
        let delta_time = context
            .multiplayer::<Multiplayer>()
            .unwrap()
            .tick_delta_time;

        let mut divergence = None;

        for player in self.players.values_mut() {
            match &mut player.role {
                PlayerRole::ClientLocal {
                    state_receiver,
                    state_history,
                    ..
                } => {
                    if let Some(Dispatch { message, .. }) = state_receiver.last() {
                        match message.apply_history_divergence(state_history) {
                            Ok(div) => {
                                divergence = TimeStamp::possibly_oldest(divergence, div);
                            }
                            Err(error) => debug!(
                                "Failed to apply state history divergence for player {}: {}",
                                player.role.info().peer_id,
                                error
                            ),
                        }
                    }
                }
                PlayerRole::ClientRemote {
                    state_receiver,
                    state_history,
                    ..
                } => {
                    if let Some(Dispatch { message, .. }) = state_receiver.last() {
                        match message.apply_history_divergence(state_history) {
                            Ok(Some(divergence)) => {
                                if let Err(error) =
                                    state_history.evolve(divergence, current_tick, |prev| {
                                        let mut state = *prev;
                                        state.position_x.0 += state.velocity_x.0 * delta_time;
                                        state.position_y.0 += state.velocity_y.0 * delta_time;
                                        Ok(state)
                                    })
                                {
                                    debug!(
                                        "Failed to evolve state history for player {}: {}",
                                        player.role.info().peer_id,
                                        error
                                    );
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                debug!(
                                    "Failed to apply state history for player {}: {}",
                                    player.role.info().peer_id,
                                    error
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        divergence
    }

    fn server_send_state(&mut self, _context: GameContext, current_tick: TimeStamp) {
        for player in self.players.values() {
            let (sender, history) = match &player.role {
                PlayerRole::ServerLocal {
                    state_sender,
                    state_history,
                    ..
                } => (state_sender, state_history),
                PlayerRole::ServerRemote {
                    state_sender,
                    state_history,
                    ..
                } => (state_sender, state_history),
                _ => {
                    continue;
                }
            };

            if let Some(event) = HistoryEvent::collect_snapshot(history, current_tick)
                && let Err(error) = sender.send(event.into())
            {
                debug!(
                    "Failed to send state snapshot for player {}: {}",
                    player.role.info().peer_id,
                    error
                );
            }
        }
    }

    fn tick(
        &mut self,
        _context: GameContext,
        current_tick: TimeStamp,
        delta_time: f32,
        _resimulating: bool,
    ) {
        for player in self.players.values_mut() {
            let input = player.role.inputs().map(|history| {
                history
                    .get_extrapolated(current_tick)
                    .copied()
                    .unwrap_or_default()
            });

            let states = player.role.state_mut();

            let mut state = states
                .get_extrapolated(current_tick)
                .copied()
                .unwrap_or_default();

            if let Some(input) = input {
                state.velocity_x.0 = match (input.left(), input.right()) {
                    (true, false) => -SPEED,
                    (false, true) => SPEED,
                    _ => 0.0,
                };
                state.velocity_y.0 = match (input.up(), input.down()) {
                    (true, false) => -SPEED,
                    (false, true) => SPEED,
                    _ => 0.0,
                };
            }

            state.position_x.0 += state.velocity_x.0 * delta_time;
            state.position_y.0 += state.velocity_y.0 * delta_time;

            states.set(current_tick, state);
        }
    }

    fn commit_frame(&mut self, _context: GameContext, current_tick: TimeStamp) {
        for player in self.players.values_mut() {
            let state = player
                .role
                .state_mut()
                .get_extrapolated(current_tick)
                .copied()
                .unwrap_or_default();
            player.sprite.transform.position.x = state.position_x.0;
            player.sprite.transform.position.y = state.position_y.0;
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct StateSnapshot {
    position_x: RepF32,
    position_y: RepF32,
    velocity_x: RepF32,
    velocity_y: RepF32,
}

struct PlayerCharacter {
    role: PlayerRole,
    sprite: Sprite,
}

impl PlayerCharacter {
    fn new(role: PlayerRole) -> Self {
        Self {
            role,
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
            .text(self.role.info().peer_id.id().to_string())
            .font("roboto.ttf")
            .size(20.0)
            .position(Vec2::from(self.sprite.transform.position) - Vec2::new(0.0, 60.0))
            .tint(if self.role.info().remote {
                Rgba::red()
            } else {
                Rgba::white()
            })
            .horizontal_align(HorizontalAlign::Center)
            .vertical_align(VerticalAlign::Bottom)
            .draw(context, graphics);
    }
}
