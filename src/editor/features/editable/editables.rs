use crate::{
    context::GameContext,
    editor::features::editable::{
        EditableSubsystem, EditableType, EditableWidgetLocation,
        ui::{edit_textual_property, edit_textual_property_convert},
    },
};
use spitfire_draw::{
    primitives::PrimitivesEmitter,
    utils::{Drawable, Vertex},
};
use spitfire_glow::renderer::GlowUniformValue;
use std::str::FromStr;
use vek::Vec2;

const GIZMO_SCREEN_SIZE: f32 = 50.0;
const GIZMO_SCREEN_THICKNESS: f32 = 8.0;

pub struct EditableAsText<T: ToString + FromStr + Send + Sync + Clone + 'static>(pub T);

impl<T: ToString + FromStr + Send + Sync + Clone + 'static> EditableType for EditableAsText<T> {
    type Value = T;
    const WIDGET_LOCATION: EditableWidgetLocation = EditableWidgetLocation::EDITING_PANEL;

    fn unpack(&self) -> Self::Value {
        self.0.clone()
    }

    fn widget(&mut self, _: &mut GameContext) {
        edit_textual_property("", &mut self.0, ());
    }
}

#[derive(Clone, Copy)]
pub struct EditablePosition(pub Vec2<f32>);

impl EditableType for EditablePosition {
    type Value = Vec2<f32>;
    const WIDGET_LOCATION: EditableWidgetLocation =
        EditableWidgetLocation::WORLD_SPACE.with(EditableWidgetLocation::EDITING_PANEL);

    fn unpack(&self) -> Self::Value {
        self.0
    }

    fn draw(&mut self, context: &mut GameContext) {
        let screen_position = context
            .graphics
            .state
            .main_camera
            .world_to_screen_point(self.0);

        const TIP_SIZE: f32 = 10.0;
        const HALF_SIZE: f32 = GIZMO_SCREEN_SIZE / 2.0;
        const HALF_THICKNESS: f32 = GIZMO_SCREEN_THICKNESS / 2.0;

        fn make_vertex(pos: Vec2<f32>) -> Vertex {
            Vertex {
                position: [pos.x, pos.y],
                color: [1.0, 1.0, 1.0, 1.0],
                uv: [0.0, 0.0, 0.0],
            }
        }

        let shader = EditableSubsystem::shader(context);
        PrimitivesEmitter::default()
            .shader(shader)
            .screen_space(true)
            .uniform("u_time".into(), GlowUniformValue::F1(context.time))
            .emit_triangles([
                // Horizontal line
                [
                    make_vertex(screen_position + Vec2::new(-HALF_SIZE, -HALF_THICKNESS)),
                    make_vertex(screen_position + Vec2::new(-HALF_SIZE, HALF_THICKNESS)),
                    make_vertex(screen_position + Vec2::new(HALF_SIZE, HALF_THICKNESS)),
                ],
                [
                    make_vertex(screen_position + Vec2::new(HALF_SIZE, HALF_THICKNESS)),
                    make_vertex(screen_position + Vec2::new(HALF_SIZE, -HALF_THICKNESS)),
                    make_vertex(screen_position + Vec2::new(-HALF_SIZE, -HALF_THICKNESS)),
                ],
                // Vertical line
                [
                    make_vertex(screen_position + Vec2::new(-HALF_THICKNESS, -HALF_SIZE)),
                    make_vertex(screen_position + Vec2::new(HALF_THICKNESS, -HALF_SIZE)),
                    make_vertex(screen_position + Vec2::new(HALF_THICKNESS, HALF_SIZE)),
                ],
                [
                    make_vertex(screen_position + Vec2::new(HALF_THICKNESS, HALF_SIZE)),
                    make_vertex(screen_position + Vec2::new(-HALF_THICKNESS, HALF_SIZE)),
                    make_vertex(screen_position + Vec2::new(-HALF_THICKNESS, -HALF_SIZE)),
                ],
                // Left arrow
                [
                    make_vertex(screen_position + Vec2::new(-HALF_SIZE - TIP_SIZE, 0.0)),
                    make_vertex(screen_position + Vec2::new(-HALF_SIZE, GIZMO_SCREEN_THICKNESS)),
                    make_vertex(screen_position + Vec2::new(-HALF_SIZE, -GIZMO_SCREEN_THICKNESS)),
                ],
                // Right arrow
                [
                    make_vertex(screen_position + Vec2::new(HALF_SIZE + TIP_SIZE, 0.0)),
                    make_vertex(screen_position + Vec2::new(HALF_SIZE, -GIZMO_SCREEN_THICKNESS)),
                    make_vertex(screen_position + Vec2::new(HALF_SIZE, GIZMO_SCREEN_THICKNESS)),
                ],
                // Top arrow
                [
                    make_vertex(screen_position + Vec2::new(0.0, -HALF_SIZE - TIP_SIZE)),
                    make_vertex(screen_position + Vec2::new(GIZMO_SCREEN_THICKNESS, -HALF_SIZE)),
                    make_vertex(screen_position + Vec2::new(-GIZMO_SCREEN_THICKNESS, -HALF_SIZE)),
                ],
                // Bottom arrow
                [
                    make_vertex(screen_position + Vec2::new(0.0, HALF_SIZE + TIP_SIZE)),
                    make_vertex(screen_position + Vec2::new(-GIZMO_SCREEN_THICKNESS, HALF_SIZE)),
                    make_vertex(screen_position + Vec2::new(GIZMO_SCREEN_THICKNESS, HALF_SIZE)),
                ],
            ])
            .draw(context.draw, context.graphics);
    }

    fn widget(&mut self, _context: &mut GameContext) {
        edit_textual_property("x", &mut self.0.x, ());
        edit_textual_property("y", &mut self.0.y, ());
    }
}

#[derive(Clone, Copy)]
pub struct EditableRotation(pub f32);

impl EditableType for EditableRotation {
    type Value = f32;
    const WIDGET_LOCATION: EditableWidgetLocation =
        EditableWidgetLocation::WORLD_SPACE.with(EditableWidgetLocation::EDITING_PANEL);

    fn unpack(&self) -> Self::Value {
        self.0
    }

    fn widget(&mut self, _context: &mut GameContext) {
        edit_textual_property_convert("", &mut self.0, (), |v| v.to_degrees(), |v| v.to_radians());
    }
}

#[derive(Clone, Copy)]
pub struct EditableScale(pub Vec2<f32>);

impl EditableType for EditableScale {
    type Value = Vec2<f32>;
    const WIDGET_LOCATION: EditableWidgetLocation =
        EditableWidgetLocation::WORLD_SPACE.with(EditableWidgetLocation::EDITING_PANEL);

    fn unpack(&self) -> Self::Value {
        self.0
    }

    fn widget(&mut self, _context: &mut GameContext) {
        edit_textual_property("x", &mut self.0.x, ());
        edit_textual_property("y", &mut self.0.y, ());
    }
}
