use crate::{
    context::GameContext,
    game::GameState,
    multiplayer::{GameMultiplayer, GameNetwork},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    error::Error,
};
use tehuti::{
    channel::{ChannelId, ChannelMode, Dispatch},
    codec::postcard::PostcardCodec,
    event::{Receiver, Sender},
    meeting::{MeetingInterface, MeetingUserEvent},
    peer::{
        Peer, PeerBuilder, PeerDestructurer, PeerId, PeerInfo, PeerKiller, PeerRoleId, TypedPeer,
        TypedPeerRole,
    },
};
use tehuti_timeline::{
    history::{HistoryBuffer, HistoryEvent},
    time::TimeStamp,
};

pub struct GgpoPrepareFrameEvent {
    pub current_tick: TimeStamp,
}

pub struct GgpoTimeTravelEvent {
    pub target_tick: TimeStamp,
}

pub struct GgpoHandleInputsEvent {
    pub current_tick: TimeStamp,
}

pub struct GgpoFindInputDivergenceEvent {
    pub current_tick: TimeStamp,
    pub out_divergence: Option<TimeStamp>,
}

pub struct GgpoMinConfirmedTickEvent {
    pub current_tick: TimeStamp,
    pub out_tick: TimeStamp,
}

pub struct GgpoDetectStateDesyncEvent {
    pub current_tick: TimeStamp,
    pub out_desync: bool,
}

pub struct GgpoTickEvent {
    pub current_tick: TimeStamp,
    pub delta_time: f32,
    pub resimulating: bool,
}

pub struct GgpoCommitFrameEvent {
    pub current_tick: TimeStamp,
}

pub struct GgpoOnStartEvent {
    pub current_tick: TimeStamp,
}

pub struct GgpoOnStopEvent {
    pub current_tick: TimeStamp,
}

pub trait GgpoGameState {
    fn is(payload: &dyn Any) -> bool {
        payload.is::<GgpoPrepareFrameEvent>()
            || payload.is::<GgpoTimeTravelEvent>()
            || payload.is::<GgpoHandleInputsEvent>()
            || payload.is::<GgpoFindInputDivergenceEvent>()
            || payload.is::<GgpoMinConfirmedTickEvent>()
            || payload.is::<GgpoDetectStateDesyncEvent>()
            || payload.is::<GgpoTickEvent>()
            || payload.is::<GgpoCommitFrameEvent>()
            || payload.is::<GgpoOnStartEvent>()
            || payload.is::<GgpoOnStopEvent>()
    }

    fn custom_event(&mut self, context: GameContext, payload: &mut dyn Any) {
        if let Some(event) = payload.downcast_ref::<GgpoPrepareFrameEvent>() {
            self.prepare_frame(context, event.current_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoTimeTravelEvent>() {
            self.time_travel(context, event.target_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoHandleInputsEvent>() {
            self.handle_inputs(context, event.current_tick);
        } else if let Some(event) = payload.downcast_mut::<GgpoFindInputDivergenceEvent>() {
            event.out_divergence = self.find_input_divergence(context, event.current_tick);
        } else if let Some(event) = payload.downcast_mut::<GgpoMinConfirmedTickEvent>() {
            event.out_tick = self.min_confirmed_tick(context, event.current_tick);
        } else if let Some(event) = payload.downcast_mut::<GgpoDetectStateDesyncEvent>() {
            event.out_desync = self.detect_state_desync(context, event.current_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoTickEvent>() {
            self.tick(
                context,
                event.current_tick,
                event.delta_time,
                event.resimulating,
            );
        } else if let Some(event) = payload.downcast_ref::<GgpoCommitFrameEvent>() {
            self.commit_frame(context, event.current_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoOnStartEvent>() {
            self.on_start(context, event.current_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoOnStopEvent>() {
            self.on_stop(context, event.current_tick);
        }
    }

    #[allow(unused_variables)]
    fn prepare_frame(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn time_travel(&mut self, context: GameContext, target_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn handle_inputs(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn find_input_divergence(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
    ) -> Option<TimeStamp> {
        None
    }

    #[allow(unused_variables)]
    fn min_confirmed_tick(&mut self, context: GameContext, current_tick: TimeStamp) -> TimeStamp {
        current_tick
    }

    #[allow(unused_variables)]
    fn detect_state_desync(&mut self, context: GameContext, current_tick: TimeStamp) -> bool {
        false
    }

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

    #[allow(unused_variables)]
    fn on_start(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn on_stop(&mut self, context: GameContext, current_tick: TimeStamp) {}
}

pub struct GgpoMultiplayer {
    pub meeting: MeetingInterface,
    current_tick: TimeStamp,
    peers_in_queue: HashMap<PeerId, Peer>,
    peers_in_play: HashSet<PeerId>,
    progressing: bool,
    request_progressing: Option<bool>,
    pub tick_delta_time: f32,
    pub max_prediction_ticks: u64,
}

impl GgpoMultiplayer {
    pub fn new(network: &GameNetwork) -> Self {
        Self {
            meeting: network.interface.clone(),
            current_tick: TimeStamp::default(),
            peers_in_queue: Default::default(),
            peers_in_play: Default::default(),
            progressing: false,
            request_progressing: None,
            // default to 20 FPS
            tick_delta_time: 0.05,
            max_prediction_ticks: u64::MAX,
        }
    }

    pub fn with_tick_delta_time(mut self, value: f32) -> Self {
        self.tick_delta_time = value;
        self
    }

    pub fn with_ticks_per_second(mut self, value: usize) -> Self {
        self.tick_delta_time = 1.0 / value.max(1) as f32;
        self
    }

    pub fn with_max_prediction_ticks(mut self, value: u64) -> Self {
        self.max_prediction_ticks = value;
        self
    }

    pub fn is_progressing(&self) -> bool {
        self.progressing
    }

    pub fn request_progressing(&mut self, value: bool) {
        self.request_progressing = Some(value);
    }

    pub fn peers_in_queue(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers_in_queue.keys().copied()
    }

    pub fn peers_in_play(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers_in_play.iter().copied()
    }

    pub fn create_peer(&self, peer_id: PeerId, role_id: PeerRoleId) {
        if let Err(error) = self
            .meeting
            .sender
            .send(MeetingUserEvent::PeerCreate(peer_id, role_id))
        {
            tracing::event!(
                target: "quaso::multiplayer::ggpo",
                tracing::Level::ERROR,
                "Failed to create peer {:?} with role {:?}: {:?}",
                peer_id,
                role_id,
                error
            );
        }
    }

    pub fn destroy_peer(&self, peer_id: PeerId) {
        if let Err(error) = self
            .meeting
            .sender
            .send(MeetingUserEvent::PeerDestroy(peer_id))
        {
            tracing::event!(
                target: "quaso::multiplayer::ggpo",
                tracing::Level::ERROR,
                "Failed to destroy peer {:?}: {:?}",
                peer_id,
                error
            );
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
                target: "quaso::multiplayer::ggpo",
                tracing::Level::DEBUG,
                "Resimulating from {:?} to {:?} ({} ticks)",
                start_tick,
                end_tick,
                end_tick - start_tick
            );

            while self.current_tick() < end_tick {
                self.simulate(state, context, true);
                self.current_tick += 1;
            }
        }
    }

    fn prepare_current_tick(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoPrepareFrameEvent { current_tick };
        state.custom_event(context, &mut payload);
    }

    fn time_travel(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
        target_tick: TimeStamp,
    ) {
        self.current_tick = target_tick;
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoTimeTravelEvent {
            target_tick: target_tick - 1,
        };
        state.custom_event(context, &mut payload);
    }

    fn handle_inputs(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoHandleInputsEvent { current_tick };
        state.custom_event(context, &mut payload);
    }

    fn find_input_divergence(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
    ) -> Option<TimeStamp> {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoFindInputDivergenceEvent {
            current_tick,
            out_divergence: None,
        };
        state.custom_event(context, &mut payload);
        payload.out_divergence
    }

    fn min_confirmed_tick(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
    ) -> TimeStamp {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoMinConfirmedTickEvent {
            current_tick,
            out_tick: current_tick,
        };
        state.custom_event(context, &mut payload);
        payload.out_tick
    }

    fn detect_state_desync(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
    ) -> bool {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoDetectStateDesyncEvent {
            current_tick,
            out_desync: false,
        };
        state.custom_event(context, &mut payload);
        payload.out_desync
    }

    fn simulate(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
        resimulating: bool,
    ) {
        let current_tick = self.current_tick();
        let tick_delta_time = self.tick_delta_time;
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoTickEvent {
            current_tick,
            delta_time: tick_delta_time,
            resimulating,
        };
        state.custom_event(context, &mut payload);
    }

    fn commit(&mut self, state: &mut dyn GameState, context: &mut GameContext) {
        let current_tick = self.current_tick();
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoCommitFrameEvent { current_tick };
        state.custom_event(context, &mut payload);
    }

    fn on_start(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
        current_tick: TimeStamp,
    ) {
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoOnStartEvent { current_tick };
        state.custom_event(context, &mut payload);
    }

    fn on_stop(
        &mut self,
        state: &mut dyn GameState,
        context: &mut GameContext,
        current_tick: TimeStamp,
    ) {
        let mut context = unsafe { context.fork() };
        context.multiplayer = Some(self);
        let mut payload = GgpoOnStopEvent { current_tick };
        state.custom_event(context, &mut payload);
    }
}

impl GameMultiplayer for GgpoMultiplayer {
    fn current_tick(&self) -> TimeStamp {
        self.current_tick
    }

    fn on_startup(&mut self, _state: &mut dyn GameState, _context: GameContext) {
        tracing::event!(
            target: "quaso::multiplayer::ggpo",
            tracing::Level::DEBUG,
            "GGPO Multiplayer starting up with peers in queue: {:?}",
            self.peers_in_queue.keys().copied().collect::<Vec<_>>()
        );
    }

    fn on_cleanup(&mut self, _state: &mut dyn GameState, _context: GameContext) {
        tracing::event!(
            target: "quaso::multiplayer::ggpo",
            tracing::Level::DEBUG,
            "GGPO Multiplayer cleaning up"
        );
    }

    fn maintain(&mut self, state: &mut dyn GameState, mut context: GameContext, _delta_time: f32) {
        for event in self.meeting.receiver.clone().iter() {
            match event {
                MeetingUserEvent::PeerAdded(peer) => {
                    self.peers_in_queue.insert(peer.info().peer_id, peer);
                }
                MeetingUserEvent::PeerRemoved(peer_id) => {
                    self.peers_in_queue.remove(&peer_id);
                    if self.peers_in_play.remove(&peer_id) {
                        let mut context = unsafe { context.fork() };
                        context.multiplayer = Some(self);
                        state.multiplayer_peer_removed(context, peer_id);
                        self.request_progressing = Some(false);
                    }
                }
                _ => {}
            }
        }

        if let Some(request_progressing) = self.request_progressing {
            if request_progressing != self.progressing {
                self.progressing = request_progressing;
                if self.progressing {
                    self.current_tick = TimeStamp::default();
                    for (_, peer) in std::mem::take(&mut self.peers_in_queue) {
                        self.peers_in_play.insert(peer.info().peer_id);
                        let mut context = unsafe { context.fork() };
                        context.multiplayer = Some(self);
                        state.multiplayer_peer_added(context, peer);
                    }
                    self.on_start(state, &mut context, self.current_tick);
                } else {
                    for peer_id in std::mem::take(&mut self.peers_in_play) {
                        let mut context = unsafe { context.fork() };
                        context.multiplayer = Some(self);
                        state.multiplayer_peer_removed(context, peer_id);
                    }
                    self.on_stop(state, &mut context, self.current_tick);
                    self.current_tick = TimeStamp::default();
                }
            }
            self.request_progressing = None;
        }

        if !self.progressing {
            return;
        }

        self.prepare_current_tick(state, &mut context);

        self.handle_inputs(state, &mut context);

        let divergence = self.find_input_divergence(state, &mut context);
        if let Some(divergence) = divergence {
            self.resimulate(state, &mut context, divergence);
        }

        let min_confirmed_tick = self.min_confirmed_tick(state, &mut context);
        let target_tick =
            (min_confirmed_tick + self.max_prediction_ticks).min(self.current_tick + 1);
        if self.current_tick < target_tick {
            while self.current_tick < target_tick {
                self.simulate(state, &mut context, false);
                self.current_tick += 1;
            }
        } else if self.current_tick > target_tick {
            tracing::event!(
                target: "quaso::multiplayer::ggpo",
                tracing::Level::DEBUG,
                "Current tick {:?} is ahead of target tick {:?}, skipping simulation",
                self.current_tick,
                target_tick
            );
        }

        if self.detect_state_desync(state, &mut context) {
            self.progressing = false;
            self.on_stop(state, &mut context, self.current_tick);
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

pub enum GgpoPlayerCommunication<Input: Send + Clone + Serialize + DeserializeOwned + 'static> {
    Local {
        input_sender: Sender<Dispatch<HistoryEvent<Input>>>,
        state_hash_sender: Sender<Dispatch<HistoryEvent<u64>>>,
    },
    Remote {
        input_receiver: Receiver<Dispatch<HistoryEvent<Input>>>,
        state_hash_receiver: Receiver<Dispatch<HistoryEvent<u64>>>,
    },
}

pub struct GgpoPlayerRole<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + 'static,
> {
    pub info: PeerInfo,
    pub communication: GgpoPlayerCommunication<Input>,
    pub input_history: HistoryBuffer<Input>,
    pub state_history: HistoryBuffer<State>,
    pub state_hash_history: HistoryBuffer<u64>,
    _peer_killer: PeerKiller,
}

impl<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + 'static,
> GgpoPlayerRole<PEER_ROLE_ID, HISTORY_CAPACITY, Input, State>
{
    const PLAYER_INPUT_CHANNEL: ChannelId = ChannelId::new(0);
    const PLAYER_STATE_HASH_CHANNEL: ChannelId = ChannelId::new(1);
}

impl<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + 'static,
> TypedPeerRole for GgpoPlayerRole<PEER_ROLE_ID, HISTORY_CAPACITY, Input, State>
{
    const ROLE_ID: PeerRoleId = PeerRoleId::new(PEER_ROLE_ID);
}

impl<
    const PEER_ROLE_ID: u64,
    const HISTORY_CAPACITY: usize,
    Input: Send + Clone + Serialize + DeserializeOwned + 'static,
    State: Send + Clone + 'static,
> TypedPeer for GgpoPlayerRole<PEER_ROLE_ID, HISTORY_CAPACITY, Input, State>
{
    fn builder(builder: PeerBuilder) -> Result<PeerBuilder, Box<dyn Error>> {
        if builder.info().remote {
            Ok(builder
                .bind_read::<PostcardCodec<HistoryEvent<Input>>, HistoryEvent<Input>>(
                    Self::PLAYER_INPUT_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                )
                .bind_read::<PostcardCodec<HistoryEvent<u64>>, HistoryEvent<u64>>(
                    Self::PLAYER_STATE_HASH_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                ))
        } else {
            Ok(builder
                .bind_write::<PostcardCodec<HistoryEvent<Input>>, HistoryEvent<Input>>(
                    Self::PLAYER_INPUT_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                )
                .bind_write::<PostcardCodec<HistoryEvent<u64>>, HistoryEvent<u64>>(
                    Self::PLAYER_STATE_HASH_CHANNEL,
                    ChannelMode::Unreliable,
                    None,
                ))
        }
    }

    fn into_typed(mut peer: PeerDestructurer) -> Result<Self, Box<dyn Error>> {
        if peer.info().remote {
            Ok(Self {
                info: *peer.info(),
                communication: GgpoPlayerCommunication::Remote {
                    input_receiver: peer.read::<HistoryEvent<Input>>(Self::PLAYER_INPUT_CHANNEL)?,
                    state_hash_receiver: peer
                        .read::<HistoryEvent<u64>>(Self::PLAYER_STATE_HASH_CHANNEL)?,
                },
                input_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                state_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                state_hash_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                _peer_killer: peer.take_killer(),
            })
        } else {
            Ok(Self {
                info: *peer.info(),
                communication: GgpoPlayerCommunication::Local {
                    input_sender: peer.write::<HistoryEvent<Input>>(Self::PLAYER_INPUT_CHANNEL)?,
                    state_hash_sender: peer
                        .write::<HistoryEvent<u64>>(Self::PLAYER_STATE_HASH_CHANNEL)?,
                },
                input_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                state_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                state_hash_history: HistoryBuffer::with_capacity(HISTORY_CAPACITY),
                _peer_killer: peer.take_killer(),
            })
        }
    }
}
