use tehuti_timeline::time::TimeStamp;

use crate::{
    context::GameContext,
    multiplayer::{
        client_server::{
            ClientServerCommitFrameEvent, ClientServerHandleInputsEvent,
            ClientServerPrepareFrameEvent, ClientServerTickEvent,
        },
        csp_ssr::{
            CspSsrClientFindStateDivergenceEvent, CspSsrCommitFrameEvent, CspSsrHandleInputsEvent,
            CspSsrPrepareFrameEvent, CspSsrServerFindInputDivergenceEvent,
            CspSsrServerSendStateEvent, CspSsrTickEvent, CspSsrTimeTravelEvent,
        },
        ggpo::{
            GgpoCommitFrameEvent, GgpoDetectStateDesyncEvent, GgpoFindInputDivergenceEvent,
            GgpoHandleInputsEvent, GgpoMinConfirmedTickEvent, GgpoOnStartEvent, GgpoOnStopEvent,
            GgpoPrepareFrameEvent, GgpoTickEvent, GgpoTimeTravelEvent,
        },
        local::{
            LocalCommitFrameEvent, LocalHandleInputsEvent, LocalPrepareFrameEvent, LocalTickEvent,
        },
        rollback::{
            RollbackClientFindStateDivergenceEvent, RollbackCommitFrameEvent,
            RollbackFindInputDivergenceEvent, RollbackHandleInputsEvent, RollbackPrepareFrameEvent,
            RollbackServerSendStateEvent, RollbackTickEvent, RollbackTimeTravelEvent,
        },
    },
};
use std::any::Any;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniversalMultiplayerAuthority {
    Unspecified,
    Client,
    Server,
}

impl UniversalMultiplayerAuthority {
    pub fn is_client(&self) -> bool {
        *self == UniversalMultiplayerAuthority::Client
    }

    pub fn is_server(&self) -> bool {
        *self == UniversalMultiplayerAuthority::Server
    }
}

pub trait UniversalMultiplayerGameState {
    fn is(payload: &dyn Any) -> bool {
        payload.is::<LocalPrepareFrameEvent>()
            || payload.is::<LocalHandleInputsEvent>()
            || payload.is::<LocalTickEvent>()
            || payload.is::<LocalCommitFrameEvent>()
            || payload.is::<ClientServerPrepareFrameEvent>()
            || payload.is::<ClientServerHandleInputsEvent>()
            || payload.is::<ClientServerTickEvent>()
            || payload.is::<ClientServerCommitFrameEvent>()
            || payload.is::<CspSsrPrepareFrameEvent>()
            || payload.is::<CspSsrTimeTravelEvent>()
            || payload.is::<CspSsrHandleInputsEvent>()
            || payload.is::<CspSsrServerFindInputDivergenceEvent>()
            || payload.is::<CspSsrClientFindStateDivergenceEvent>()
            || payload.is::<CspSsrServerSendStateEvent>()
            || payload.is::<CspSsrTickEvent>()
            || payload.is::<CspSsrCommitFrameEvent>()
            || payload.is::<RollbackPrepareFrameEvent>()
            || payload.is::<RollbackTimeTravelEvent>()
            || payload.is::<RollbackHandleInputsEvent>()
            || payload.is::<RollbackFindInputDivergenceEvent>()
            || payload.is::<RollbackClientFindStateDivergenceEvent>()
            || payload.is::<RollbackServerSendStateEvent>()
            || payload.is::<RollbackTickEvent>()
            || payload.is::<RollbackCommitFrameEvent>()
            || payload.is::<GgpoPrepareFrameEvent>()
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
        if let Some(payload) = payload.downcast_ref::<LocalPrepareFrameEvent>() {
            self.prepare_frame(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<LocalHandleInputsEvent>() {
            self.handle_inputs(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<LocalTickEvent>() {
            self.tick(context, payload.current_tick, payload.delta_time, false);
        } else if let Some(payload) = payload.downcast_ref::<LocalCommitFrameEvent>() {
            self.commit_frame(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<ClientServerPrepareFrameEvent>() {
            self.prepare_frame(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<ClientServerHandleInputsEvent>() {
            self.handle_inputs(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<ClientServerTickEvent>() {
            self.tick(context, payload.current_tick, payload.delta_time, false);
        } else if let Some(payload) = payload.downcast_ref::<ClientServerCommitFrameEvent>() {
            self.commit_frame(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<CspSsrPrepareFrameEvent>() {
            self.prepare_frame(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<CspSsrTimeTravelEvent>() {
            self.time_travel(context, payload.target_tick);
        } else if let Some(payload) = payload.downcast_ref::<CspSsrHandleInputsEvent>() {
            self.handle_inputs(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_mut::<CspSsrServerFindInputDivergenceEvent>()
        {
            payload.out_divergence = self.find_input_divergence(
                context,
                payload.current_tick,
                UniversalMultiplayerAuthority::Server,
            );
        } else if let Some(payload) = payload.downcast_mut::<CspSsrClientFindStateDivergenceEvent>()
        {
            payload.out_divergence = self.find_state_divergence(
                context,
                payload.current_tick,
                UniversalMultiplayerAuthority::Client,
            );
        } else if let Some(payload) = payload.downcast_ref::<CspSsrServerSendStateEvent>() {
            self.send_state(
                context,
                payload.current_tick,
                UniversalMultiplayerAuthority::Server,
            );
        } else if let Some(payload) = payload.downcast_ref::<CspSsrTickEvent>() {
            self.tick(
                context,
                payload.current_tick,
                payload.delta_time,
                payload.resimulating,
            );
        } else if let Some(payload) = payload.downcast_ref::<CspSsrCommitFrameEvent>() {
            self.commit_frame(context, payload.current_tick)
        } else if let Some(event) = payload.downcast_ref::<RollbackPrepareFrameEvent>() {
            self.prepare_frame(context, event.current_tick);
        } else if let Some(event) = payload.downcast_ref::<RollbackTimeTravelEvent>() {
            self.time_travel(context, event.target_tick);
        } else if let Some(event) = payload.downcast_ref::<RollbackHandleInputsEvent>() {
            self.handle_inputs(context, event.current_tick);
        } else if let Some(event) = payload.downcast_mut::<RollbackFindInputDivergenceEvent>() {
            event.out_divergence = self.find_input_divergence(
                context,
                event.current_tick,
                UniversalMultiplayerAuthority::Unspecified,
            );
        } else if let Some(event) = payload.downcast_mut::<RollbackClientFindStateDivergenceEvent>()
        {
            event.out_divergence = self.find_state_divergence(
                context,
                event.current_tick,
                UniversalMultiplayerAuthority::Client,
            );
        } else if let Some(event) = payload.downcast_ref::<RollbackServerSendStateEvent>() {
            self.send_state(
                context,
                event.current_tick,
                UniversalMultiplayerAuthority::Server,
            );
        } else if let Some(event) = payload.downcast_ref::<RollbackTickEvent>() {
            self.tick(
                context,
                event.current_tick,
                event.delta_time,
                event.resimulating,
            );
        } else if let Some(event) = payload.downcast_ref::<RollbackCommitFrameEvent>() {
            self.commit_frame(context, event.current_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoPrepareFrameEvent>() {
            self.prepare_frame(context, event.current_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoTimeTravelEvent>() {
            self.time_travel(context, event.target_tick);
        } else if let Some(event) = payload.downcast_ref::<GgpoHandleInputsEvent>() {
            self.handle_inputs(context, event.current_tick);
        } else if let Some(event) = payload.downcast_mut::<GgpoFindInputDivergenceEvent>() {
            event.out_divergence = self.find_input_divergence(
                context,
                event.current_tick,
                UniversalMultiplayerAuthority::Unspecified,
            );
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

    /// Issued by:
    /// - Local
    /// - CSP+SSR
    /// - Rollback
    /// - GGPO
    #[allow(unused_variables)]
    fn prepare_frame(&mut self, context: GameContext, current_tick: TimeStamp) {}

    /// Issued by:
    /// - CSP+SSR
    /// - Rollback
    /// - GGPO
    #[allow(unused_variables)]
    fn time_travel(&mut self, context: GameContext, target_tick: TimeStamp) {}

    /// Issued by:
    /// - Local
    /// - Client-server
    /// - CSP+SSR
    /// - Rollback
    /// - GGPO
    #[allow(unused_variables)]
    fn handle_inputs(&mut self, context: GameContext, current_tick: TimeStamp) {}

    /// Issued by:
    /// - CSP+SSR
    /// - Rollback
    /// - GGPO
    #[allow(unused_variables)]
    fn find_input_divergence(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
        authority: UniversalMultiplayerAuthority,
    ) -> Option<TimeStamp> {
        None
    }

    /// Issued by:
    /// - CSP+SSR
    /// - Rollback
    #[allow(unused_variables)]
    fn find_state_divergence(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
        authority: UniversalMultiplayerAuthority,
    ) -> Option<TimeStamp> {
        None
    }

    /// Issued by:
    /// - CSP+SSR
    /// - Rollback
    #[allow(unused_variables)]
    fn send_state(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
        authority: UniversalMultiplayerAuthority,
    ) {
    }

    /// Issued by:
    /// - GGPO
    #[allow(unused_variables)]
    fn min_confirmed_tick(&mut self, context: GameContext, current_tick: TimeStamp) -> TimeStamp {
        current_tick
    }

    /// Issued by:
    /// - GGPO
    #[allow(unused_variables)]
    fn detect_state_desync(&mut self, context: GameContext, current_tick: TimeStamp) -> bool {
        false
    }

    /// Issued by:
    /// - Local
    /// - Client-server
    /// - CSP+SSR
    /// - Rollback
    /// - GGPO
    #[allow(unused_variables)]
    fn tick(
        &mut self,
        context: GameContext,
        current_tick: TimeStamp,
        delta_time: f32,
        resimulating: bool,
    ) {
    }

    /// Issued by:
    /// - Local
    /// - CSP+SSR
    /// - Rollback
    /// - GGPO
    #[allow(unused_variables)]
    fn commit_frame(&mut self, context: GameContext, current_tick: TimeStamp) {}

    /// Issued by:
    /// - GGPO
    #[allow(unused_variables)]
    fn on_start(&mut self, context: GameContext, current_tick: TimeStamp) {}

    /// Issued by:
    /// - GGPO
    #[allow(unused_variables)]
    fn on_stop(&mut self, context: GameContext, current_tick: TimeStamp) {}
}
