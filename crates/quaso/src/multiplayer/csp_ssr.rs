use crate::{
    context::GameContext,
    game::GameState,
    multiplayer::{GameMultiplayer, GameNetwork, clock::ClockExtension},
    third_party::time::{Duration, Instant},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{any::Any, error::Error};
use tehuti::{
    channel::{ChannelId, ChannelMode, Dispatch},
    codec::postcard::PostcardCodec,
    event::{Receiver, Sender, unbounded},
    peer::{
        Peer, PeerBuilder, PeerDestructurer, PeerId, PeerInfo, PeerKiller, PeerRoleId, TypedPeer,
        TypedPeerRole,
    },
};
use tehuti_client_server::authority::{Authority, AuthorityUserData};
use tehuti_timeline::{
    clock::Clock,
    history::{HistoryBuffer, HistoryEvent},
    time::TimeStamp,
};

pub type CspSsrAuthority<const AUTHORITY_CLOCK_CHANNEL: u64> =
    Authority<ClockExtension<AUTHORITY_CLOCK_CHANNEL>>;

pub struct CspSsrPrepareFrameEvent {
    pub current_tick: TimeStamp,
}

pub struct CspSsrTimeTravelEvent {
    pub target_tick: TimeStamp,
}

pub struct CspSsrHandleInputsEvent {
    pub current_tick: TimeStamp,
}

pub struct CspSsrServerFindInputDivergenceEvent {
    pub current_tick: TimeStamp,
    pub out_divergence: Option<TimeStamp>,
}

pub struct CspSsrClientFindStateDivergenceEvent {
    pub current_tick: TimeStamp,
    pub out_divergence: Option<TimeStamp>,
}

pub struct CspSsrServerSendStateEvent {
    pub current_tick: TimeStamp,
}

pub struct CspSsrTickEvent {
    pub current_tick: TimeStamp,
    pub delta_time: f32,
    pub resimulating: bool,
}

pub struct CspSsrCommitFrameEvent {
    pub current_tick: TimeStamp,
}

pub trait CspSsrGameState {
    fn is(payload: &dyn Any) -> bool {
        payload.is::<CspSsrPrepareFrameEvent>()
            || payload.is::<CspSsrTimeTravelEvent>()
            || payload.is::<CspSsrHandleInputsEvent>()
            || payload.is::<CspSsrServerFindInputDivergenceEvent>()
            || payload.is::<CspSsrClientFindStateDivergenceEvent>()
            || payload.is::<CspSsrServerSendStateEvent>()
            || payload.is::<CspSsrTickEvent>()
            || payload.is::<CspSsrCommitFrameEvent>()
    }

    fn custom_event(&mut self, context: GameContext, payload: &mut dyn Any) {
        if let Some(payload) = payload.downcast_ref::<CspSsrPrepareFrameEvent>() {
            self.prepare_frame(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<CspSsrTimeTravelEvent>() {
            self.time_travel(context, payload.target_tick);
        } else if let Some(payload) = payload.downcast_ref::<CspSsrHandleInputsEvent>() {
            self.handle_inputs(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_mut::<CspSsrServerFindInputDivergenceEvent>()
        {
            payload.out_divergence =
                self.server_find_input_divergence(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_mut::<CspSsrClientFindStateDivergenceEvent>()
        {
            payload.out_divergence =
                self.client_find_state_divergence(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<CspSsrServerSendStateEvent>() {
            self.server_send_state(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<CspSsrTickEvent>() {
            self.tick(
                context,
                payload.current_tick,
                payload.delta_time,
                payload.resimulating,
            );
        } else if let Some(payload) = payload.downcast_ref::<CspSsrCommitFrameEvent>() {
            self.commit_frame(context, payload.current_tick)
        }
    }

    #[allow(unused_variables)]
    fn prepare_frame(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn time_travel(&mut self, context: GameContext, target_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn handle_inputs(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn server_find_input_divergence(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
    ) -> Option<TimeStamp> {
        None
    }

    #[allow(unused_variables)]
    fn client_find_state_divergence(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
    ) -> Option<TimeStamp> {
        None
    }

    #[allow(unused_variables)]
    fn server_send_state(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn tick(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
        delta_time: f32,
        resimulating: bool,
    ) {
    }

    #[allow(unused_variables)]
    fn commit_frame(&mut self, context: GameContext, current_tick: TimeStamp) {}
}

pub struct CspSsrMultiplayer<const AUTHORITY_CLOCK_CHANNEL: u64> {
    pub authority: CspSsrAuthority<AUTHORITY_CLOCK_CHANNEL>,
    added_peers_receiver: Receiver<Peer>,
    removed_peers_receiver: Receiver<PeerId>,
    time_accumulator: f32,
    state_send_timer: Instant,
    clock_timer: Instant,
    pub process_lifecycle_events: bool,
    pub tick_delta_time: f32,
    pub ticks_limit_per_frame: usize,
    pub server_send_state_interval: Duration,
    pub client_lead_ticks: u64,
    pub client_ping_interval: Duration,
}

impl<const AUTHORITY_CLOCK_CHANNEL: u64> CspSsrMultiplayer<AUTHORITY_CLOCK_CHANNEL> {
    pub fn new(network: &GameNetwork) -> Option<Self> {
        let is_server = network
            .meeting
            .meeting_factory()
            .user_data
            .access::<AuthorityUserData>()
            .ok()?
            .is_server;
        let (added_peers_sender, added_peers_receiver) = unbounded();
        let (removed_peers_sender, removed_peers_receiver) = unbounded();
        let authority = CspSsrAuthority::new(
            is_server,
            network.interface.clone(),
            added_peers_sender,
            removed_peers_sender,
        )
        .ok()?;
        Some(Self {
            authority,
            added_peers_receiver,
            removed_peers_receiver,
            time_accumulator: 0.0,
            state_send_timer: Instant::now(),
            clock_timer: Instant::now(),
            process_lifecycle_events: false,
            // default to 20 FPS
            tick_delta_time: 0.05,
            ticks_limit_per_frame: usize::MAX,
            server_send_state_interval: Duration::from_millis(1000 / 20),
            client_lead_ticks: 2,
            client_ping_interval: Duration::from_millis(250),
        })
    }

    pub fn with_process_lifecycle_events(mut self, value: bool) -> Self {
        self.process_lifecycle_events = value;
        self
    }

    pub fn with_tick_delta_time(mut self, value: f32) -> Self {
        self.tick_delta_time = value;
        self
    }

    pub fn with_ticks_per_second(mut self, value: usize) -> Self {
        self.tick_delta_time = 1.0 / value.max(1) as f32;
        self
    }

    pub fn with_ticks_limit_per_frame(mut self, value: usize) -> Self {
        self.ticks_limit_per_frame = value;
        self
    }

    pub fn with_server_send_state_interval(mut self, value: Duration) -> Self {
        self.server_send_state_interval = value;
        self
    }

    pub fn with_client_lead_ticks(mut self, value: u64) -> Self {
        self.client_lead_ticks = value;
        self
    }

    pub fn with_client_ping_interval(mut self, value: Duration) -> Self {
        self.client_ping_interval = value;
        self
    }

    pub fn is_initialized(&self) -> bool {
        self.authority.is_initialized()
    }

    pub fn is_server(&self) -> bool {
        self.authority.is_server()
    }

    pub fn is_client(&self) -> bool {
        self.authority.is_client()
    }

    pub fn clock(&self) -> Option<&Clock> {
        self.authority.extension().map(|extension| &extension.clock)
    }

    fn maintain_server(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let divergence = self.server_find_input_divergence(state, context);

        if let Some(divergence) = divergence {
            tracing::event!(
                target: "quaso::multiplayer::csp_ssr",
                tracing::Level::DEBUG,
                "Input divergence detected at tick {:?}",
                divergence
            );
            self.resimulate(state, context, divergence);
        }

        if self.state_send_timer.elapsed() >= self.server_send_state_interval {
            self.state_send_timer = Instant::now();
            self.server_send_state(state, context);
        }

        let extension = self.authority.extension().unwrap();

        if let Clock::Authority(clock) = &extension.clock
            && let Some(Dispatch { message, .. }) = extension.events.receiver.last()
        {
            let event = clock.pong(message);
            if let Err(error) = extension.events.sender.send(event.into()) {
                tracing::event!(
                    target: "quaso::multiplayer::csp_ssr",
                    tracing::Level::ERROR,
                    "Failed to send clock event: {}",
                    error
                );
            }
        }
    }

    fn maintain_client(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let divergence = self.client_find_state_divergence(state, context);

        if let Some(divergence) = divergence {
            tracing::event!(
                target: "quaso::multiplayer::csp_ssr",
                tracing::Level::DEBUG,
                "State divergence detected at tick {:?}",
                divergence
            );
            self.resimulate(state, context, divergence);
        }

        let extension = self.authority.extension_mut().unwrap();

        if let Clock::Simulation(clock) = &mut extension.clock {
            if self.clock_timer.elapsed() >= self.client_ping_interval {
                let event = clock.ping();
                if let Err(error) = extension.events.sender.send(event.into()) {
                    tracing::event!(
                        target: "quaso::multiplayer::csp_ssr",
                        tracing::Level::ERROR,
                        "Failed to send clock event: {}",
                        error
                    );
                }
                self.clock_timer = Instant::now();
            }

            for Dispatch { message, .. } in extension.events.receiver.iter() {
                clock.roundtrip(message);
            }
        }
    }

    fn resimulate(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
        divergence: TimeStamp,
    ) {
        let start_tick = divergence;
        let end_tick = self.current_tick();

        if start_tick < end_tick {
            self.time_travel(state, context, start_tick);

            tracing::event!(
                target: "quaso::multiplayer::csp_ssr",
                tracing::Level::DEBUG,
                "Resimulating from {:?} to {:?} ({} ticks)",
                start_tick,
                end_tick,
                end_tick - start_tick
            );

            while self.current_tick() < end_tick {
                self.simulate(state, context, true);
                self.authority.extension_mut().unwrap().clock.advance(1);
            }

            self.server_send_state(state, context);
        }
    }

    fn prepare_current_tick(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let current_tick = self.current_tick();
        self.authority
            .extension_mut()
            .unwrap()
            .clock
            .set_tick(current_tick);
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = CspSsrPrepareFrameEvent { current_tick };
        state.custom_event(context, &mut payload);
    }

    fn time_travel(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
        target_tick: TimeStamp,
    ) {
        self.authority
            .extension_mut()
            .unwrap()
            .clock
            .set_tick(target_tick);
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = CspSsrTimeTravelEvent { target_tick };
        state.custom_event(context, &mut payload);
    }

    fn handle_inputs(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = CspSsrHandleInputsEvent { current_tick };
        state.custom_event(context, &mut payload);
    }

    fn server_find_input_divergence(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
    ) -> Option<TimeStamp> {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = CspSsrServerFindInputDivergenceEvent {
            current_tick,
            out_divergence: None,
        };
        state.custom_event(context, &mut payload);
        payload.out_divergence
    }

    fn client_find_state_divergence(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
    ) -> Option<TimeStamp> {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = CspSsrClientFindStateDivergenceEvent {
            current_tick,
            out_divergence: None,
        };
        state.custom_event(context, &mut payload);
        payload.out_divergence
    }

    fn server_send_state(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = CspSsrServerSendStateEvent { current_tick };
        state.custom_event(context, &mut payload);
    }

    fn simulate(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
        resimulating: bool,
    ) {
        let current_tick = self.current_tick();
        self.authority
            .extension_mut()
            .unwrap()
            .clock
            .set_tick(current_tick);
        let tick_delta_time = self.tick_delta_time;
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        state.custom_event(
            context,
            &mut CspSsrTickEvent {
                current_tick,
                delta_time: tick_delta_time,
                resimulating,
            },
        );
    }

    fn commit(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = CspSsrCommitFrameEvent { current_tick };
        state.custom_event(context, &mut payload);
    }
}

impl<const AUTHORITY_CLOCK_CHANNEL: u64> GameMultiplayer
    for CspSsrMultiplayer<AUTHORITY_CLOCK_CHANNEL>
{
    fn current_tick(&self) -> TimeStamp {
        self.authority
            .extension()
            .map(|extension| extension.clock.tick())
            .unwrap_or_default()
    }

    fn maintain(&mut self, state: &mut dyn GameState, mut context: GameContext, delta_time: f32) {
        if let Err(error) = self.authority.maintain() {
            tracing::event!(
                target: "quaso::multiplayer::csp_ssr",
                tracing::Level::ERROR,
                "Failed to maintain authority: {}",
                error
            );
        }
        if !self.process_lifecycle_events || !self.authority.is_initialized() {
            return;
        }

        let added_peers_receiver = self.added_peers_receiver.clone();
        for peer in added_peers_receiver.iter() {
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.multiplayer_peer_added(context, peer);
        }

        let removed_peers_receiver = self.removed_peers_receiver.clone();
        for peer_id in removed_peers_receiver.iter() {
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.multiplayer_peer_removed(context, peer_id);
        }

        self.prepare_current_tick(state, &mut context);

        if self.authority.is_server() {
            self.maintain_server(state, &mut context);
        } else if self.authority.is_client() {
            self.maintain_client(state, &mut context);
        }

        self.handle_inputs(state, &mut context);

        if self.authority.is_server() {
            self.time_accumulator += delta_time;
            let mut ticks_this_frame = 0;
            while self.time_accumulator >= self.tick_delta_time
                && ticks_this_frame < self.ticks_limit_per_frame
            {
                self.time_accumulator -= self.tick_delta_time;
                ticks_this_frame += 1;
                self.simulate(state, &mut context, false);
                self.authority.extension_mut().unwrap().clock.advance(1);
            }
        } else if self.authority.is_client()
            && let Clock::Simulation(clock) = &self.authority.extension().unwrap().clock
        {
            let target = clock.estimate_target_tick(self.client_lead_ticks);
            while self.current_tick() < target {
                self.simulate(state, &mut context, false);
                self.authority.extension_mut().unwrap().clock.advance(1);
            }
        }

        self.commit(state, &mut context);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub enum CspSsrPlayerRole<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + Serialize + DeserializeOwned + 'static,
> {
    ServerLocal {
        info: PeerInfo,
        state_sender: Sender<Dispatch<HistoryEvent<State>>>,
        input_history: HistoryBuffer<Input>,
        state_history: HistoryBuffer<State>,
        _peer_killer: PeerKiller,
    },
    ServerRemote {
        info: PeerInfo,
        input_receiver: Receiver<Dispatch<HistoryEvent<Input>>>,
        state_sender: Sender<Dispatch<HistoryEvent<State>>>,
        input_history: HistoryBuffer<Input>,
        state_history: HistoryBuffer<State>,
        _peer_killer: PeerKiller,
    },
    ClientLocal {
        info: PeerInfo,
        input_sender: Sender<Dispatch<HistoryEvent<Input>>>,
        state_receiver: Receiver<Dispatch<HistoryEvent<State>>>,
        input_history: HistoryBuffer<Input>,
        state_history: HistoryBuffer<State>,
        _peer_killer: PeerKiller,
    },
    ClientRemote {
        info: PeerInfo,
        state_receiver: Receiver<Dispatch<HistoryEvent<State>>>,
        state_history: HistoryBuffer<State>,
        _peer_killer: PeerKiller,
    },
}

impl<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + Serialize + DeserializeOwned + 'static,
> CspSsrPlayerRole<PEER_ROLE_ID, HISTORY_CAPACITY, Input, State>
{
    const PLAYER_INPUT_CHANNEL: ChannelId = ChannelId::new(0);
    const PLAYER_STATE_CHANNEL: ChannelId = ChannelId::new(1);

    pub fn info(&self) -> &PeerInfo {
        match self {
            Self::ServerLocal { info, .. } => info,
            Self::ServerRemote { info, .. } => info,
            Self::ClientLocal { info, .. } => info,
            Self::ClientRemote { info, .. } => info,
        }
    }

    pub fn inputs(&self) -> Option<&HistoryBuffer<Input>> {
        match self {
            Self::ServerLocal { input_history, .. } => Some(input_history),
            Self::ClientLocal { input_history, .. } => Some(input_history),
            Self::ServerRemote { input_history, .. } => Some(input_history),
            _ => None,
        }
    }

    pub fn inputs_mut(&mut self) -> Option<&mut HistoryBuffer<Input>> {
        match self {
            Self::ServerLocal { input_history, .. } => Some(input_history),
            Self::ClientLocal { input_history, .. } => Some(input_history),
            Self::ServerRemote { input_history, .. } => Some(input_history),
            _ => None,
        }
    }

    pub fn state(&self) -> &HistoryBuffer<State> {
        match self {
            Self::ServerLocal { state_history, .. } => state_history,
            Self::ServerRemote { state_history, .. } => state_history,
            Self::ClientLocal { state_history, .. } => state_history,
            Self::ClientRemote { state_history, .. } => state_history,
        }
    }

    pub fn state_mut(&mut self) -> &mut HistoryBuffer<State> {
        match self {
            Self::ServerLocal { state_history, .. } => state_history,
            Self::ServerRemote { state_history, .. } => state_history,
            Self::ClientLocal { state_history, .. } => state_history,
            Self::ClientRemote { state_history, .. } => state_history,
        }
    }
}

impl<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + Serialize + DeserializeOwned + 'static,
> TypedPeerRole for CspSsrPlayerRole<PEER_ROLE_ID, HISTORY_CAPACITY, Input, State>
{
    const ROLE_ID: PeerRoleId = PeerRoleId::new(PEER_ROLE_ID);
}

impl<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + Serialize + DeserializeOwned + 'static,
> TypedPeer for CspSsrPlayerRole<PEER_ROLE_ID, HISTORY_CAPACITY, Input, State>
{
    fn builder(builder: PeerBuilder) -> Result<PeerBuilder, Box<dyn Error>> {
        let is_server = builder.user_data().access::<AuthorityUserData>()?.is_server;
        let is_local = !builder.info().remote;

        Ok(match (is_server, is_local) {
            (true, true) => builder
                .bind_write::<PostcardCodec<HistoryEvent<State>>, HistoryEvent<State>>(
                    Self::PLAYER_STATE_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                ),
            (true, false) => builder
                .bind_read::<PostcardCodec<HistoryEvent<Input>>, HistoryEvent<Input>>(
                    Self::PLAYER_INPUT_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                )
                .bind_write::<PostcardCodec<HistoryEvent<State>>, HistoryEvent<State>>(
                    Self::PLAYER_STATE_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                ),
            (false, true) => builder
                .bind_write::<PostcardCodec<HistoryEvent<Input>>, HistoryEvent<Input>>(
                    Self::PLAYER_INPUT_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                )
                .bind_read::<PostcardCodec<HistoryEvent<State>>, HistoryEvent<State>>(
                    Self::PLAYER_STATE_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                ),
            (false, false) => builder
                .bind_read::<PostcardCodec<HistoryEvent<State>>, HistoryEvent<State>>(
                    Self::PLAYER_STATE_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                ),
        })
    }

    fn into_typed(mut peer: PeerDestructurer) -> Result<Self, Box<dyn Error>> {
        let is_server = peer.user_data().access::<AuthorityUserData>()?.is_server;
        let is_local = !peer.info().remote;

        match (is_server, is_local) {
            (true, true) => Ok(Self::ServerLocal {
                info: *peer.info(),
                state_sender: peer.write::<HistoryEvent<State>>(Self::PLAYER_STATE_CHANNEL)?,
                input_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                state_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                _peer_killer: peer.take_killer(),
            }),
            (true, false) => Ok(Self::ServerRemote {
                info: *peer.info(),
                input_receiver: peer.read::<HistoryEvent<Input>>(Self::PLAYER_INPUT_CHANNEL)?,
                state_sender: peer.write::<HistoryEvent<State>>(Self::PLAYER_STATE_CHANNEL)?,
                input_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                state_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                _peer_killer: peer.take_killer(),
            }),
            (false, true) => Ok(Self::ClientLocal {
                info: *peer.info(),
                input_sender: peer.write::<HistoryEvent<Input>>(Self::PLAYER_INPUT_CHANNEL)?,
                state_receiver: peer.read::<HistoryEvent<State>>(Self::PLAYER_STATE_CHANNEL)?,
                input_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                state_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                _peer_killer: peer.take_killer(),
            }),
            (false, false) => Ok(Self::ClientRemote {
                info: *peer.info(),
                state_receiver: peer.read::<HistoryEvent<State>>(Self::PLAYER_STATE_CHANNEL)?,
                state_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                _peer_killer: peer.take_killer(),
            }),
        }
    }
}
