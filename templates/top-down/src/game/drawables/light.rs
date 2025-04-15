use quaso::third_party::{
    spitfire_draw::{
        context::DrawContext,
        sprite::Sprite,
        utils::{Drawable, ShaderRef, Vertex},
    },
    spitfire_glow::{
        graphics::Graphics,
        renderer::{GlowBlending, GlowUniformValue},
    },
    vek::Vec2,
};
use std::ops::RangeInclusive;

pub fn draw_sphere_light(
    position: Vec2<f32>,
    radius: f32,
    intensity: RangeInclusive<f32>,
    attenuation: f32,
    draw: &mut DrawContext,
    graphics: &mut Graphics<Vertex>,
) {
    Sprite::default()
        .shader(ShaderRef::name("sphere-light"))
        .position(position)
        .pivot(0.5.into())
        .size((radius * 2.0).into())
        .uniform(
            "u_intensity".into(),
            GlowUniformValue::F2([*intensity.start(), *intensity.end()]),
        )
        .uniform("u_attenuation".into(), GlowUniformValue::F1(attenuation))
        .blending(GlowBlending::Alpha)
        .draw(draw, graphics);
}
