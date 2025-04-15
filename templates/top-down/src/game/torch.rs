use quaso::{
    context::GameContext,
    game::GameObject,
    third_party::{
        rand::{thread_rng, Rng},
        spitfire_draw::{
            particles::{
                ParticleEmitter, ParticleInstance, ParticleSystem, ParticleSystemProcessor,
            },
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, ShaderRef, TextureRef},
        },
        spitfire_glow::renderer::GlowTextureFiltering,
        vek::{Rgba, Transform, Vec2},
    },
};
use std::ops::RangeInclusive;

pub struct Torch {
    pub sprite: Sprite,
    pub fire: ParticleSystem<TorchParticlesProcessor, TorchParticleData, f32>,
    pub emmission_accumulator: f32,
}

impl Torch {
    pub fn new(position: impl Into<Vec2<f32>>) -> Self {
        Self {
            sprite: Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name("item/torch"),
                filtering: GlowTextureFiltering::Linear,
            })
            .position(position.into())
            .pivot([0.5, 1.0].into()),
            fire: ParticleSystem::new(0.0, 100),
            emmission_accumulator: 0.0,
        }
    }
}

impl GameObject for Torch {
    fn process(&mut self, _: &mut GameContext, delta_time: f32) {
        self.fire.config = delta_time;
        self.fire.process();

        self.emmission_accumulator += delta_time * 5.0;
        while self.emmission_accumulator > 0.0 {
            self.emmission_accumulator -= 1.0;
            self.fire.push(TorchParticleData::new(
                self.sprite.transform.position.xy() + Vec2::new(0.0, -22.0),
                60.0f32.to_radians(),
                10.0..=20.0,
                0.85..=0.95,
                2.0..=3.0,
            ))
        }
    }

    fn draw(&mut self, context: &mut GameContext) {
        self.sprite.draw(context.draw, context.graphics);

        ParticleEmitter::single(SpriteTexture {
            sampler: "u_image".into(),
            texture: TextureRef::name("particle/fire"),
            filtering: GlowTextureFiltering::Linear,
        })
        .shader(ShaderRef::name("image"))
        .emit(self.fire.emit())
        .draw(context.draw, context.graphics);
    }
}

pub struct TorchParticleData {
    position: Vec2<f32>,
    velocity: Vec2<f32>,
    stabilization: f32,
    lifetime: f32,
    lifetime_max: f32,
}

impl TorchParticleData {
    pub fn new(
        position: Vec2<f32>,
        angle_range: f32,
        speed: RangeInclusive<f32>,
        stabilization: RangeInclusive<f32>,
        lifetime_max: RangeInclusive<f32>,
    ) -> Self {
        let mut rng = thread_rng();
        let angle = rng.gen_range((-angle_range)..=angle_range);
        let speed = rng.gen_range(speed);
        let stabilization = rng.gen_range(stabilization);
        let lifetime_max = rng.gen_range(lifetime_max);
        let (vx, vy) = angle.sin_cos();
        Self {
            position,
            velocity: Vec2 { x: vx, y: -vy } * speed,
            stabilization,
            lifetime: lifetime_max,
            lifetime_max,
        }
    }
}

pub struct TorchParticlesProcessor {}

impl ParticleSystemProcessor<TorchParticleData, f32> for TorchParticlesProcessor {
    fn process(delta_time: &f32, mut data: TorchParticleData) -> Option<TorchParticleData> {
        data.lifetime -= *delta_time;
        if data.lifetime >= 0.0 {
            data.position += data.velocity * *delta_time;
            data.velocity.x *= data.stabilization;
            Some(data)
        } else {
            None
        }
    }

    fn emit(_: &f32, data: &TorchParticleData) -> Option<ParticleInstance> {
        let alpha = data.lifetime / data.lifetime_max;
        if alpha > 0.0 {
            Some(ParticleInstance {
                tint: Rgba {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: alpha,
                },
                transform: Transform {
                    position: data.position.into(),
                    ..Default::default()
                },
                size: Vec2 { x: 8.0, y: 16.0 },
                pivot: 0.5.into(),
                ..Default::default()
            })
        } else {
            None
        }
    }
}
