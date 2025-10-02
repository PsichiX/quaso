use crate::game::enemy::EnemyState;
use quaso::{
    character::CharacterMemory,
    third_party::{emergent::condition::Condition, vek::Vec2},
};

pub struct EnemyIsMovingCondition;

impl Condition<CharacterMemory<EnemyState>> for EnemyIsMovingCondition {
    fn validate(&self, memory: &CharacterMemory<EnemyState>) -> bool {
        let state = memory.state.read();
        let Vec2 { x, y } = state.ai.direction;
        x.abs() + y.abs() > 0.1
    }
}

pub struct EnemyIsAttackingCondition;

impl Condition<CharacterMemory<EnemyState>> for EnemyIsAttackingCondition {
    fn validate(&self, memory: &CharacterMemory<EnemyState>) -> bool {
        let state = memory.state.read();
        state.ai.attack
    }
}

pub struct EnemyIsNotInCooldownCondition;

impl Condition<CharacterMemory<EnemyState>> for EnemyIsNotInCooldownCondition {
    fn validate(&self, memory: &CharacterMemory<EnemyState>) -> bool {
        let state = memory.state.read();
        state.ai.cooldown_seconds <= 0.0
    }
}

pub struct EnemyHasPlayerInRangeCondition;

impl Condition<CharacterMemory<EnemyState>> for EnemyHasPlayerInRangeCondition {
    fn validate(&self, memory: &CharacterMemory<EnemyState>) -> bool {
        let state = memory.state.read();
        state.ai.target_in_range_position.is_some()
    }
}
