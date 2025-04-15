use crate::game::enemy::EnemyState;
use quaso::{character::CharacterMemory, third_party::emergent::task::Task};

pub struct EnemyAiWaitTask;

impl Task<CharacterMemory<EnemyState>> for EnemyAiWaitTask {
    fn on_enter(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        let mut state = memory.state.write().unwrap();
        state.ai.attack = false;
        state.ai.direction = 0.0.into();
    }
}
