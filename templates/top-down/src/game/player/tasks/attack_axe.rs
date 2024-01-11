use crate::game::player::PlayerState;
use micro_games_kit::{
    animation::{FrameAnimation, NamedAnimation},
    character::CharacterMemory,
    third_party::emergent::task::Task,
};

#[derive(Debug, Clone)]
pub struct PlayerAttackAxeTask {
    animation: NamedAnimation,
}

impl Default for PlayerAttackAxeTask {
    fn default() -> Self {
        Self {
            animation: NamedAnimation {
                animation: FrameAnimation::new(1..9),
                id: "player/axe".to_owned(),
            },
        }
    }
}

impl Task<CharacterMemory<PlayerState>> for PlayerAttackAxeTask {
    fn is_locked(&self, _: &CharacterMemory<PlayerState>) -> bool {
        self.animation.animation.is_playing
    }

    fn on_enter(&mut self, _: &mut CharacterMemory<PlayerState>) {
        self.animation.animation.play();
    }

    fn on_exit(&mut self, _: &mut CharacterMemory<PlayerState>) {
        self.animation.animation.stop();
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<PlayerState>) {
        self.animation.animation.update(memory.delta_time);

        memory
            .state
            .write()
            .unwrap()
            .apply_animation(&self.animation);
    }
}
