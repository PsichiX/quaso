use crate::{context::GameContext, game::GameState, multiplayer::GameMultiplayer};
use std::any::Any;
use tehuti_timeline::time::TimeStamp;

pub struct LocalPrepareFrameEvent {
    pub current_tick: TimeStamp,
}

pub struct LocalHandleInputsEvent {
    pub current_tick: TimeStamp,
}

pub struct LocalTickEvent {
    pub current_tick: TimeStamp,
    pub delta_time: f32,
}

pub struct LocalCommitFrameEvent {
    pub current_tick: TimeStamp,
}

pub trait LocalGameState {
    fn is(payload: &dyn Any) -> bool {
        payload.is::<LocalPrepareFrameEvent>()
            || payload.is::<LocalHandleInputsEvent>()
            || payload.is::<LocalTickEvent>()
            || payload.is::<LocalCommitFrameEvent>()
    }

    fn custom_event(&mut self, context: GameContext, payload: &mut dyn Any) {
        if let Some(payload) = payload.downcast_ref::<LocalPrepareFrameEvent>() {
            self.prepare_frame(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<LocalHandleInputsEvent>() {
            self.handle_inputs(context, payload.current_tick);
        } else if let Some(payload) = payload.downcast_ref::<LocalTickEvent>() {
            self.tick(context, payload.current_tick, payload.delta_time);
        } else if let Some(payload) = payload.downcast_ref::<LocalCommitFrameEvent>() {
            self.commit_frame(context, payload.current_tick);
        }
    }

    #[allow(unused_variables)]
    fn prepare_frame(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn handle_inputs(&mut self, context: GameContext, current_tick: TimeStamp) {}

    #[allow(unused_variables)]
    fn tick(&mut self, context: GameContext, current_tick: TimeStamp, delta_time: f32) {}

    #[allow(unused_variables)]
    fn commit_frame(&mut self, context: GameContext, current_tick: TimeStamp) {}
}

#[derive(Default)]
pub struct LocalMultiplayer {
    current_tick: TimeStamp,
}

impl GameMultiplayer for LocalMultiplayer {
    fn current_tick(&self) -> TimeStamp {
        self.current_tick
    }

    fn maintain(&mut self, state: &mut dyn GameState, mut context: GameContext, delta_time: f32) {
        {
            let current_tick = self.current_tick;
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.custom_event(context, &mut LocalPrepareFrameEvent { current_tick });
        }

        {
            let current_tick = self.current_tick;
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.custom_event(context, &mut LocalHandleInputsEvent { current_tick });
        }

        {
            let current_tick = self.current_tick;
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.custom_event(
                context,
                &mut LocalTickEvent {
                    current_tick,
                    delta_time,
                },
            );
            self.current_tick += 1;
        }

        {
            let current_tick = self.current_tick;
            let mut context = unsafe { context.fork() };
            context.multiplayer = Some(self);
            state.custom_event(context, &mut LocalCommitFrameEvent { current_tick });
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
