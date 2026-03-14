use crate::{
    context::GameContext,
    game::GameState,
    multiplayer::{GameMultiplayer, GameNetwork},
};
use std::any::Any;
use tehuti::{
    event::{Receiver, unbounded},
    peer::{Peer, PeerId},
};
use tehuti_client_server::authority::{AuthorityUserData, PureAuthority};
use tehuti_timeline::time::TimeStamp;

pub struct ClientServerHandleInputsEvent {
    pub current_tick: TimeStamp,
}

pub struct ClientServerTickEvent {
    pub current_tick: TimeStamp,
    pub delta_time: f32,
}

pub trait ClientServerGameState {
    fn is(payload: &dyn Any) -> bool {
        payload.is::<ClientServerHandleInputsEvent>() || payload.is::<ClientServerTickEvent>()
    }

    fn custom_event(&mut self, context: GameContext, payload: &mut dyn Any) {
        if let Some(payload) = payload.downcast_ref::<ClientServerHandleInputsEvent>() {
            self.handle_inputs(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<ClientServerTickEvent>() {
            self.tick(context, payload.current_tick, payload.delta_time);
        }
    }

    #[allow(unused_variables)]
    fn handle_inputs(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn tick(&mut self, context: GameContext, current_tick: TimeStamp, delta_time: f32) {}
}

pub struct ClientServerMultiplayer {
    pub authority: PureAuthority,
    added_peers_receiver: Receiver<Peer>,
    removed_peers_receiver: Receiver<PeerId>,
    current_tick: TimeStamp,
    time_accumulator: f32,
    pub process_lifecycle_events: bool,
    pub tick_delta_time: f32,
    pub ticks_limit_per_frame: usize,
}

impl ClientServerMultiplayer {
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
        let authority = PureAuthority::new(
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
            current_tick: TimeStamp::default(),
            time_accumulator: 0.0,
            process_lifecycle_events: false,
            // default to 20 FPS
            tick_delta_time: 0.05,
            ticks_limit_per_frame: usize::MAX,
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

    pub fn is_initialized(&self) -> bool {
        self.authority.is_initialized()
    }

    pub fn is_server(&self) -> bool {
        self.authority.is_server()
    }

    pub fn is_client(&self) -> bool {
        self.authority.is_client()
    }
}

impl GameMultiplayer for ClientServerMultiplayer {
    fn current_tick(&self) -> TimeStamp {
        self.current_tick
    }

    fn maintain(&mut self, state: &mut dyn GameState, mut context: GameContext, delta_time: f32) {
        if let Err(error) = self.authority.maintain() {
            tracing::event!(
                target: "quaso::multiplayer::client_server",
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

        {
            let current_tick = self.current_tick;
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.custom_event(context, &mut ClientServerHandleInputsEvent { current_tick });
        }

        self.time_accumulator += delta_time;
        let mut ticks_this_frame = 0;
        while self.time_accumulator >= self.tick_delta_time
            && ticks_this_frame < self.ticks_limit_per_frame
        {
            self.time_accumulator -= self.tick_delta_time;
            let current_tick = self.current_tick;
            let tick_delta_time = self.tick_delta_time;
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.custom_event(
                context,
                &mut ClientServerTickEvent {
                    current_tick,
                    delta_time: tick_delta_time,
                },
            );
            ticks_this_frame += 1;
            self.current_tick += 1;
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
