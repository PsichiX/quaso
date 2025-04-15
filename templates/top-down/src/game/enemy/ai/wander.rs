use crate::game::enemy::EnemyState;
use quaso::{
    character::CharacterMemory,
    third_party::{
        emergent::task::Task,
        rand::{thread_rng, Rng},
        vek::Vec2,
    },
};
use std::{f32::consts::TAU, ops::Range};

pub struct EnemyAiWanderTask {
    pub target_point_radius_range: Range<f32>,
    pub target_distance_threshold: f32,
    pub cooldown_seconds: f32,
    target_position: Option<Vec2<f32>>,
}

impl Default for EnemyAiWanderTask {
    fn default() -> Self {
        Self {
            target_point_radius_range: 50.0..150.0,
            target_distance_threshold: 10.0,
            cooldown_seconds: 1.0,
            target_position: None,
        }
    }
}

impl Task<CharacterMemory<EnemyState>> for EnemyAiWanderTask {
    fn on_exit(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        self.target_position = None;

        let mut state = memory.state.write().unwrap();
        state.ai.direction = 0.0.into();
        state.ai.attack = false;
        state.ai.cooldown_seconds = 0.0;
    }

    fn on_update(&mut self, memory: &mut CharacterMemory<EnemyState>) {
        let mut state = memory.state.write().unwrap();

        if let Some(position) = self.target_position {
            state.ai.direction = position - state.sprite.transform.position.xy();
            if state.ai.direction.magnitude() <= self.target_distance_threshold {
                self.target_position = None;
                state.ai.cooldown_seconds = self.cooldown_seconds;
            }
        } else {
            let radius = thread_rng().gen_range(self.target_point_radius_range.clone());
            let angle = thread_rng().gen_range(0.0..TAU);
            let (y, x) = angle.sin_cos();
            self.target_position = Some(Vec2 { x, y } * radius);
            state.ai.direction = 0.0.into();
        }
    }
}
