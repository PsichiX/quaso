use crate::game::enemy::EnemyState;
use quaso::{
    animation::frame::{FrameAnimation, NamedFrameAnimation},
    character::CharacterMemory,
    third_party::emergent::task::Task,
};

#[derive(Debug, Clone)]
pub struct EnemyIdleTask {
    animation: NamedFrameAnimation,
}

impl Default for EnemyIdleTask {
    fn default() -> Self {
        Self {
            animation: NamedFrameAnimation {
                animation: FrameAnimation::new(1..6).speed(10.0).looping(),
                id: "enemy/idle".to_owned(),
            },
        }
    }
}

impl Task<CharacterMemory<EnemyState>> for EnemyIdleTask {
    fn on_enter(&mut self, _: &mut CharacterMemory<EnemyState>) {
        self.animation.animation.play();
    }

    fn on_exit(&mut self, _: &mut CharacterMemory<EnemyState>) {
        self.animation.animation.stop();
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        self.animation.animation.update(memory.delta_time);

        memory.state.write().apply_animation(&self.animation);
    }
}
