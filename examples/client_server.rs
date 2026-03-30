use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    game_state_custom_event,
    multiplayer::{
        GameMultiplayerChange, GameNetwork,
        client_server::ClientServerMultiplayer,
        tcp::{TcpClientConnection, TcpServerConnection},
        universal::UniversalMultiplayerGameState,
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
            ArrayInputCombinator, CardinalInputCombinator, InputActionRef, InputAxisRef,
            InputConsume, InputMapping, MouseButton, VirtualAction, VirtualAxis,
        },
        tehuti::{
            channel::{ChannelId, ChannelMode},
            codec::postcard::PostcardCodec,
            event::{Receiver, unbounded},
            peer::{Peer, PeerBuilder, PeerId, PeerInfo, PeerRoleId, TypedPeer, TypedPeerRole},
            replica::{
                Replica, ReplicaApplyChanges, ReplicaCollectChanges, ReplicaId, ReplicationBuffer,
            },
            replication::{HashReplicated, primitives::RepF32},
        },
        tehuti_client_server::{
            authority::PureAuthority,
            controller::{Controller, ControllerEvent},
            puppet::{Puppet, Puppetable},
        },
        tehuti_timeline::time::TimeStamp,
        tracing::{debug, level_filters::LevelFilter},
        tracing_subscriber::{
            Layer, fmt::layer, layer::SubscriberExt, registry, util::SubscriberInitExt,
        },
        vek::{Rgba, Vec2},
        windowing::event::VirtualKeyCode,
    },
};
use std::{any::Any, collections::HashMap, error::Error, sync::Arc};

const ADDRESS: &str = "127.0.0.1:12345";
const PLAYER_ROLE: PeerRoleId = PeerRoleId::new(1);
const PLAYER_EVENT_CHANNEL: ChannelId = ChannelId::new(0);
const PLAYER_CHANGE_CHANNEL: ChannelId = ChannelId::new(1);
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
    .title("Netcode: Replicated Client-Server")
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
        if let Some(multiplayer) = context.multiplayer::<ClientServerMultiplayer>()
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
            if context.multiplayer::<ClientServerMultiplayer>().is_some() {
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
                        PureAuthority::peer_factory(is_server)
                            .unwrap()
                            .with_typed::<PlayerController>(),
                    ));
                    if is_server {
                        network.add_connection(TcpServerConnection::listen(ADDRESS).unwrap());
                    } else {
                        network.add_connection(TcpClientConnection::connect(ADDRESS).unwrap());
                    }
                    if let Some(multiplayer) = ClientServerMultiplayer::new(&network) {
                        debug!("{} started", if is_server { "Server" } else { "Client" });

                        *context.network = network;
                        *context.multiplayer_change = GameMultiplayerChange::Set(Box::new(
                            multiplayer.with_ticks_per_second(30),
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
    players: HashMap<PeerId, Player>,
    movement: CardinalInputCombinator,
    exit: InputActionRef,
}

impl GameState for State {
    fn enter(&mut self, mut context: GameContext) {
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

        if let Some(multiplayer) = context.multiplayer_mut::<ClientServerMultiplayer>() {
            multiplayer.process_lifecycle_events = true;
            let _ = multiplayer.authority.create_peer(PlayerController::ROLE_ID);
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
            if let Some(character) = &player.character {
                character.draw(context.draw, context.graphics);
            }
        }
    }

    fn multiplayer_peer_added(&mut self, mut context: GameContext, peer: Peer) {
        let (replica_added_sender, replica_added_receiver) = unbounded();
        let (replica_removed_sender, replica_removed_receiver) = unbounded();

        let mut controller = Controller::new(
            peer,
            PLAYER_EVENT_CHANNEL,
            Some(PLAYER_CHANGE_CHANNEL),
            None,
            replica_added_sender,
            replica_removed_sender,
        )
        .unwrap();

        if let Some(multiplayer) = context.multiplayer_mut::<ClientServerMultiplayer>() {
            let _ = controller.create_replica(&mut multiplayer.authority);
        }

        for player in self.players.values_mut() {
            if let Some(character) = &mut player.character {
                character.request_full_snapshot();
            }
        }

        self.players.insert(
            controller.info().peer_id,
            Player {
                controller,
                character: None,
                replica_added_receiver,
                replica_removed_receiver,
            },
        );
    }

    fn multiplayer_peer_removed(&mut self, _context: GameContext, peer_id: PeerId) {
        self.players.remove(&peer_id);
    }

    game_state_custom_event! {
        trait(UniversalMultiplayerGameState)
    }
}

impl UniversalMultiplayerGameState for State {
    fn handle_inputs(&mut self, _context: GameContext, _current_tick: TimeStamp) {
        let [x, y] = self.movement.get();
        let movement = Vec2::new(x, y).try_normalized().unwrap_or_default() * SPEED;

        for player in self.players.values_mut() {
            if !player.controller.info().remote
                && let Some(character) = &mut player.character
            {
                character.velocity_x.0 = movement.x;
                character.velocity_y.0 = movement.y;
            }
        }
    }

    fn tick(
        &mut self,
        _context: GameContext,
        _current_tick: TimeStamp,
        delta_time: f32,
        _resimulating: bool,
    ) {
        for player in self.players.values_mut() {
            player.tick(delta_time);
        }
    }
}

struct Character {
    peer_info: PeerInfo,
    sprite: Sprite,
    position_x: HashReplicated<RepF32>,
    position_y: HashReplicated<RepF32>,
    velocity_x: HashReplicated<RepF32>,
    velocity_y: HashReplicated<RepF32>,
}

impl Character {
    fn new(peer_info: PeerInfo, position: Vec2<f32>) -> Self {
        Self {
            peer_info,
            sprite: Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name("ferris.png"),
                filtering: GlowTextureFiltering::Linear,
            })
            .pivot(0.5.into())
            .position(position)
            .scale(0.25.into()),
            position_x: HashReplicated::new(RepF32(position.x)),
            position_y: HashReplicated::new(RepF32(position.y)),
            velocity_x: Default::default(),
            velocity_y: Default::default(),
        }
    }

    fn tick_before(&mut self, delta_time: f32) {
        self.position_x.0 += self.velocity_x.0 * delta_time;
        self.position_y.0 += self.velocity_y.0 * delta_time;
    }

    fn tick_after(&mut self) {
        self.sprite.transform.position.x = self.position_x.0;
        self.sprite.transform.position.y = self.position_y.0;
    }
}

impl Drawable for Character {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
        self.sprite.draw(context, graphics);

        Text::new(ShaderRef::name("text"))
            .text(self.peer_info.peer_id.id().to_string())
            .font("roboto.ttf")
            .size(20.0)
            .position(Vec2::from(self.sprite.transform.position) - Vec2::new(0.0, 60.0))
            .tint(if self.peer_info.remote {
                Rgba::red()
            } else {
                Rgba::white()
            })
            .horizontal_align(HorizontalAlign::Center)
            .vertical_align(VerticalAlign::Bottom)
            .draw(context, graphics);
    }
}

impl Puppetable for Character {
    fn collect_changes(
        &mut self,
        mut collector: ReplicaCollectChanges,
        full_snapshot: bool,
    ) -> Result<(), Box<dyn Error>> {
        if full_snapshot {
            HashReplicated::mark_changed(&mut self.position_x);
            HashReplicated::mark_changed(&mut self.position_y);
        }

        collector
            .scope()
            .maybe_collect_replicated::<0, _, _>(&self.position_x)?;
        collector
            .scope()
            .maybe_collect_replicated::<1, _, _>(&self.position_y)?;
        collector
            .scope()
            .maybe_collect_replicated::<2, _, _>(&self.velocity_x)?;
        collector
            .scope()
            .maybe_collect_replicated::<3, _, _>(&self.velocity_y)?;

        Ok(())
    }

    fn apply_changes(&mut self, mut applicator: ReplicaApplyChanges) -> Result<(), Box<dyn Error>> {
        applicator
            .scope()
            .maybe_apply_replicated::<0, _, _>(&mut self.position_x)?;
        applicator
            .scope()
            .maybe_apply_replicated::<1, _, _>(&mut self.position_y)?;
        applicator
            .scope()
            .maybe_apply_replicated::<2, _, _>(&mut self.velocity_x)?;
        applicator
            .scope()
            .maybe_apply_replicated::<3, _, _>(&mut self.velocity_y)?;

        self.sprite.transform.position.x = self.position_x.0;
        self.sprite.transform.position.y = self.position_y.0;

        Ok(())
    }
}

struct Player {
    controller: Controller,
    character: Option<Puppet<Character>>,
    replica_added_receiver: Receiver<Replica>,
    replica_removed_receiver: Receiver<ReplicaId>,
}

impl Player {
    fn tick(&mut self, delta_time: f32) {
        let _ = self.controller.maintain();

        for replica in self.replica_added_receiver.iter() {
            self.character = Some(Puppet::new(
                replica,
                Character::new(*self.controller.info(), 0.0.into()),
            ));
        }

        for _ in self.replica_removed_receiver.iter() {
            self.character = None;
        }

        if let Some(character) = &mut self.character {
            character.tick_before(delta_time);
        }

        if let Some(puppet) = &mut self.character {
            let _ = puppet.replicate();
        }

        if let Some(character) = &mut self.character {
            character.tick_after();
        }
    }
}

struct PlayerController;

impl TypedPeerRole for PlayerController {
    const ROLE_ID: PeerRoleId = PLAYER_ROLE;
}

impl TypedPeer for PlayerController {
    fn builder(builder: PeerBuilder) -> Result<PeerBuilder, Box<dyn Error>> {
        if builder.info().remote {
            Ok(builder
                .bind_read_write::<PostcardCodec<ControllerEvent>, ControllerEvent>(
                    PLAYER_EVENT_CHANNEL,
                    ChannelMode::ReliableOrdered,
                    None,
                )
                .bind_read::<ReplicationBuffer, ReplicationBuffer>(
                    PLAYER_CHANGE_CHANNEL,
                    ChannelMode::ReliableOrdered,
                    None,
                ))
        } else {
            Ok(builder
                .bind_read_write::<PostcardCodec<ControllerEvent>, ControllerEvent>(
                    PLAYER_EVENT_CHANNEL,
                    ChannelMode::ReliableOrdered,
                    None,
                )
                .bind_write::<ReplicationBuffer, ReplicationBuffer>(
                    PLAYER_CHANGE_CHANNEL,
                    ChannelMode::ReliableOrdered,
                    None,
                ))
        }
    }
}
