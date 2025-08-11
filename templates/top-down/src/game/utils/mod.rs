pub mod events;
pub mod space;

use quaso::{context::GameContext, third_party::vek::Vec2};

pub fn world_to_screen(position: Vec2<f32>, context: &GameContext) -> Vec2<f32> {
    context
        .graphics
        .state
        .main_camera
        .world_matrix()
        .mul_point(position)
}

pub fn world_to_screen_anchor(position: Vec2<f32>, context: &GameContext) -> Vec2<f32> {
    let position = world_to_screen(position, context);
    Vec2 {
        x: (position.x + 1.0) * 0.5,
        y: (-position.y + 1.0) * 0.5,
    }
}
