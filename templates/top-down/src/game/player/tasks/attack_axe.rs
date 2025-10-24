use crate::game::{
    player::PlayerState,
    utils::events::{Event, Events, Instigator},
};
use quaso::{
    animation::frame::{FrameAnimation, NamedFrameAnimation},
    character::CharacterMemory,
    third_party::emergent::task::Task,
};

#[derive(Debug, Clone)]
pub struct PlayerAttackAxeTask {
    animation: NamedFrameAnimation,
}

impl Default for PlayerAttackAxeTask {
    fn default() -> Self {
        Self {
            animation: NamedFrameAnimation {
                animation: FrameAnimation::new(1..9).event(4, "hit"),
                id: "player/axe".to_owned(),
            },
        }
    }
}

impl Task<CharacterMemory<PlayerState>> for PlayerAttackAxeTask {
    fn is_locked(&self, _: &CharacterMemory<PlayerState>) -> bool {
        self.animation.animation.is_playing
    }

    fn on_enter(&mut self, memory: &mut CharacterMemory<PlayerState>) {
        let state = memory.state.read();

        self.animation.animation.play();

        Events::write(Event::Attack {
            position: state.sprite.transform.position.xy(),
            range: state.weapon.range(),
            value: state.total_attack(),
            instigator: Instigator::Player,
        });
    }

    fn on_exit(&mut self, _: &mut CharacterMemory<PlayerState>) {
        self.animation.animation.stop();
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<PlayerState>) {
        let events = self.animation.animation.update(memory.delta_time);
        {
            for event in events {
                if event == "hit" {
                    Events::write(Event::PlaySound("axe".into()));
                }
            }
        }

        memory.state.write().apply_animation(&self.animation);
    }
}
