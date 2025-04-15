use quaso::{
    animation::frame::{FrameAnimation, NamedAnimation},
    character::CharacterMemory,
    third_party::{
        emergent::task::Task,
        vek::{Vec2, Vec3},
    },
};

use crate::game::enemy::EnemyState;

#[derive(Debug, Clone)]
pub struct EnemyRunTask {
    animation: NamedAnimation,
    speed: f32,
}

impl Default for EnemyRunTask {
    fn default() -> Self {
        Self {
            animation: NamedAnimation {
                animation: FrameAnimation::new(1..9).fps(10.0).looping(),
                id: "enemy/run".to_owned(),
            },
            speed: 60.0,
        }
    }
}

impl Task<CharacterMemory<EnemyState>> for EnemyRunTask {
    fn on_enter(&mut self, _: &mut CharacterMemory<EnemyState>) {
        self.animation.animation.play();
    }

    fn on_exit(&mut self, _: &mut CharacterMemory<EnemyState>) {
        self.animation.animation.stop();
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        let mut state = memory.state.write().unwrap();
        let Vec2 { x, y } = state.ai.direction;
        let direction = Vec3::new(x, y, 0.0).normalized();

        self.animation.animation.update(memory.delta_time);
        state.apply_animation(&self.animation);

        state.sprite.transform.position += direction * self.speed * memory.delta_time;
        if x > 0.1 {
            state.sprite.transform.scale.x = 1.0;
        } else if x < -0.1 {
            state.sprite.transform.scale.x = -1.0;
        }
    }
}
