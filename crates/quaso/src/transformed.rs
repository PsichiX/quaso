use crate::{context::GameContext, game::GameObject};
use spitfire_draw::utils::transform_to_matrix;
use vek::{Mat4, Quaternion, Transform, Vec3};

pub struct Transformed<T: GameObject> {
    pub inner: T,
    pub transform: Transform<f32, f32, f32>,
}

impl<T: GameObject> Transformed<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            transform: Default::default(),
        }
    }

    pub fn with_transform(mut self, transform: Transform<f32, f32, f32>) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_position(mut self, position: impl Into<Vec3<f32>>) -> Self {
        self.transform.position = position.into();
        self
    }

    pub fn with_rotation(mut self, rotation: impl Into<Quaternion<f32>>) -> Self {
        self.transform.orientation = rotation.into();
        self
    }

    pub fn with_scale(mut self, scale: impl Into<Vec3<f32>>) -> Self {
        self.transform.scale = scale.into();
        self
    }

    pub fn world_matrix(&self) -> Mat4<f32> {
        transform_to_matrix(self.transform)
    }

    pub fn world_inverse_matrix(&self) -> Mat4<f32> {
        self.world_matrix().inverted()
    }
}

impl<T: GameObject> GameObject for Transformed<T> {
    fn activate(&mut self, context: &mut GameContext) {
        self.inner.activate(context);
    }

    fn deactivate(&mut self, context: &mut GameContext) {
        self.inner.deactivate(context);
    }

    fn process(&mut self, context: &mut GameContext, delta_time: f32) {
        self.inner.process(context, delta_time);
    }

    fn draw(&mut self, context: &mut GameContext) {
        context
            .draw
            .push_transform_relative(transform_to_matrix(self.transform));
        self.inner.draw(context);
        context.draw.pop_transform();
    }
}
