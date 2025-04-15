use super::{PlayerState, PlayerWeapon};
use quaso::{character::CharacterMemory, third_party::emergent::condition::Condition};

pub struct PlayerIsMovingCondition;

impl Condition<CharacterMemory<PlayerState>> for PlayerIsMovingCondition {
    fn validate(&self, memory: &CharacterMemory<PlayerState>) -> bool {
        let state = memory.state.read().unwrap();
        let [x, y] = state.input.movement.get();
        x.abs() + y.abs() > 0.1
    }
}

pub struct PlayerIsAttackingCondition;

impl Condition<CharacterMemory<PlayerState>> for PlayerIsAttackingCondition {
    fn validate(&self, memory: &CharacterMemory<PlayerState>) -> bool {
        let state = memory.state.read().unwrap();
        state.input.attack.get().is_down()
    }
}

pub struct PlayerHasActiveWeapon(pub PlayerWeapon);

impl Condition<CharacterMemory<PlayerState>> for PlayerHasActiveWeapon {
    fn validate(&self, memory: &CharacterMemory<PlayerState>) -> bool {
        let state = memory.state.read().unwrap();
        state.weapon == self.0
    }
}
