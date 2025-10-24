use crate::game::player::PlayerState;
use quaso::{
    animation::frame::{FrameAnimation, NamedFrameAnimation},
    character::CharacterMemory,
    third_party::emergent::task::Task,
};

#[derive(Debug, Clone)]
pub struct PlayerIdleTask {
    animation: NamedFrameAnimation,
}

impl Default for PlayerIdleTask {
    fn default() -> Self {
        Self {
            animation: NamedFrameAnimation {
                animation: FrameAnimation::new(1..2).looping(),
                id: "player/idle".to_owned(),
            },
        }
    }
}

impl Task<CharacterMemory<PlayerState>> for PlayerIdleTask {
    fn on_enter(&mut self, _: &mut CharacterMemory<PlayerState>) {
        self.animation.animation.play();
    }

    fn on_exit(&mut self, _: &mut CharacterMemory<PlayerState>) {
        self.animation.animation.stop();
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<PlayerState>) {
        self.animation.animation.update(memory.delta_time);

        memory.state.write().apply_animation(&self.animation);
    }
}
