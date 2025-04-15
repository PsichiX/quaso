use crate::game::enemy::EnemyState;
use quaso::{character::CharacterMemory, third_party::emergent::task::Task};

pub struct EnemyAiPursueTask {
    pub target_distance_threshold: f32,
}

impl Default for EnemyAiPursueTask {
    fn default() -> Self {
        Self {
            target_distance_threshold: 10.0,
        }
    }
}

impl Task<CharacterMemory<EnemyState>> for EnemyAiPursueTask {
    fn on_exit(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        let mut state = memory.state.write().unwrap();
        state.ai.direction = 0.0.into();
        state.ai.attack = false;
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        let mut state = memory.state.write().unwrap();

        if let Some(mut position) = state.ai.target_in_range_position {
            let side = position.x - state.sprite.transform.position.x;
            if side >= 0.0 {
                position.x -= state.ai.target_offset_x;
            } else {
                position.x += state.ai.target_offset_x;
            }

            state.ai.direction = position - state.sprite.transform.position.xy();
            if state.ai.direction.magnitude() <= self.target_distance_threshold {
                state.ai.direction = 0.0.into();
                state.ai.attack = true;
            } else {
                state.ai.attack = false;
            }
        } else {
            state.ai.direction = 0.0.into();
        }
    }
}
