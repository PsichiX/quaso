use crate::game::{
    enemy::EnemyState,
    utils::events::{Event, Events, Instigator},
};
use quaso::{
    animation::frame::{FrameAnimation, NamedFrameAnimation},
    character::CharacterMemory,
    third_party::emergent::task::Task,
};

#[derive(Debug, Clone)]
pub struct EnemyAttackTask {
    animation: NamedFrameAnimation,
}

impl Default for EnemyAttackTask {
    fn default() -> Self {
        Self {
            animation: NamedFrameAnimation {
                animation: FrameAnimation::new(1..9),
                id: "enemy/attack".to_owned(),
            },
        }
    }
}

impl Task<CharacterMemory<EnemyState>> for EnemyAttackTask {
    fn is_locked(&self, _: &CharacterMemory<EnemyState>) -> bool {
        self.animation.animation.is_playing
    }

    fn on_enter(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        let state = memory.state.read();

        self.animation.animation.play();

        Events::write(Event::Attack {
            position: state.sprite.transform.position.xy(),
            range: state.attack_range,
            value: state.attack,
            instigator: Instigator::Enemy,
        });
    }

    fn on_exit(&mut self, _: &mut CharacterMemory<EnemyState>) {
        self.animation.animation.stop();
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        let mut state = memory.state.write();

        self.animation.animation.update(memory.delta_time);

        if !self.animation.animation.is_playing {
            state.ai.cooldown_seconds = 0.25;
        }

        state.apply_animation(&self.animation);

        if let Some(position) = state.ai.target_in_range_position {
            let dx = position.x - state.sprite.transform.position.x;
            if dx > 0.1 {
                state.sprite.transform.scale.x = 1.0;
            } else if dx < -0.1 {
                state.sprite.transform.scale.x = -1.0;
            }
        }
    }
}
