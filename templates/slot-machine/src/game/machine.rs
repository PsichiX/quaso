use quaso::{
    context::GameContext,
    coroutine::{async_delta_time, async_next_frame, coroutine},
    game::GameObject,
    third_party::{
        moirai::coroutine::with_all,
        rand::{Rng, rng},
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, ShaderRef, TextureRef},
        },
        spitfire_glow::renderer::GlowTextureFiltering,
        vek::{Rect, Transform, Vec2, Vec3},
    },
    value::Ptr,
};
use std::{
    future::Future,
    ops::{Add, AddAssign, Rem, RemAssign},
    pin::Pin,
};

const REEL_SPIN_DURATION: f32 = 2.0;
const LEVER_DOWN_DURATION: f32 = 0.5;
const BASELINE_SYMBOLS_ROLL: usize = 4 * 5;
const OFFSET: Vec2<f32> = Vec2::new(-816.0 * 0.5, -624.0 * 0.5);
const REEL_OFFSETS: [Vec2<f32>; 3] = [
    Vec2::new((337.0 + 229.0) * 0.5, (456.0 + 247.0) * 0.5),
    Vec2::new((467.0 + 359.0) * 0.5, (456.0 + 247.0) * 0.5),
    Vec2::new((597.0 + 489.0) * 0.5, (456.0 + 247.0) * 0.5),
];

#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd)]
pub struct Reel {
    index: usize,
    fraction: f32,
}

impl Reel {
    pub fn new(index: usize, fraction: f32) -> Self {
        Self {
            index,
            fraction: fraction.max(0.0),
        }
    }

    pub fn from_real(real: f32) -> Self {
        let index = real.floor() as usize;
        let fraction = real.fract().max(0.0);
        Self { index, fraction }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn fraction(&self) -> f32 {
        self.fraction
    }

    pub fn real(&self) -> f32 {
        self.index as f32 + self.fraction
    }

    pub fn lerp(&self, other: &Self, factor: f32) -> Self {
        let from = self.real();
        let to = other.real();
        Reel::from_real(from + (to - from) * factor)
    }

    pub fn normalize(&mut self) {
        if self.fraction >= 0.5 {
            self.index = self.index.wrapping_add(1);
        }
        self.fraction = 0.0;
    }
}

impl Add<Self> for Reel {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self::Output {
        self += rhs;
        self
    }
}

impl AddAssign<Self> for Reel {
    fn add_assign(&mut self, rhs: Self) {
        self.fraction += rhs.fraction;
        self.index = rhs.index.wrapping_add(rhs.index);
        while self.fraction >= 1.0 {
            self.index = self.index.wrapping_add(1);
            self.fraction -= 1.0;
        }
    }
}

impl Add<f32> for Reel {
    type Output = Self;

    fn add(mut self, rhs: f32) -> Self::Output {
        self += rhs;
        self
    }
}

impl AddAssign<f32> for Reel {
    fn add_assign(&mut self, rhs: f32) {
        self.fraction += rhs.max(0.0);
        while self.fraction >= 1.0 {
            self.index = self.index.wrapping_add(1);
            self.fraction -= 1.0;
        }
    }
}

impl Add<usize> for Reel {
    type Output = Self;

    fn add(mut self, rhs: usize) -> Self::Output {
        self += rhs;
        self
    }
}

impl AddAssign<usize> for Reel {
    fn add_assign(&mut self, rhs: usize) {
        self.index = self.index.wrapping_add(rhs);
    }
}

impl Rem<usize> for Reel {
    type Output = Self;

    fn rem(mut self, rhs: usize) -> Self::Output {
        self %= rhs;
        self
    }
}

impl RemAssign<usize> for Reel {
    fn rem_assign(&mut self, rhs: usize) {
        self.index %= rhs;
    }
}

#[derive(Debug)]
pub struct SlotMachine {
    reels: [Reel; 3],
    lever_down: bool,
    spinning: bool,
}

impl Default for SlotMachine {
    fn default() -> Self {
        Self {
            reels: [Reel::new(0, 0.0), Reel::new(1, 0.0), Reel::new(2, 0.0)],
            lever_down: false,
            spinning: false,
        }
    }
}

impl SlotMachine {
    pub async fn spin(this: Ptr<Self>) -> Option<usize> {
        let (source_reels, target_reels) = {
            if this.read().spinning {
                return None;
            }
            this.write().spinning = true;
            let source_reels = this.read().reels;
            let target_reels = this.read().reels.map(|reel| {
                let index = rng().random_range(0..4) + BASELINE_SYMBOLS_ROLL;
                Reel::new(reel.index() + index, 0.0)
            });
            (source_reels, target_reels)
        };

        coroutine(SlotMachine::hold_lever(this.clone())).await;

        with_all(
            (0..3)
                .map(|index| {
                    Box::pin(SlotMachine::spin_reel(
                        this.clone(),
                        index,
                        (index as f32) * 0.5,
                        source_reels[index],
                        target_reels[index],
                    ))
                        as Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>
                })
                .collect(),
        )
        .await;

        this.write().spinning = false;
        let expected = this.read().reels[0].index() % 4;
        for reel in this.read().reels.iter().skip(1) {
            if reel.index() % 4 != expected {
                return None;
            }
        }
        Some(expected)
    }

    async fn spin_reel(
        this: Ptr<Self>,
        index: usize,
        delay: f32,
        source_reel: Reel,
        target_reel: Reel,
    ) {
        let mut timer = -delay;
        loop {
            timer += async_delta_time().await;
            if timer > REEL_SPIN_DURATION {
                break;
            }

            let factor = (timer / REEL_SPIN_DURATION).clamp(0.0, 1.0);
            let factor = factor * factor * (3.0 - 2.0 * factor);
            this.write().reels[index] = source_reel.lerp(&target_reel, factor);

            async_next_frame().await;
        }

        this.write().reels[index].normalize();
    }

    async fn hold_lever(this: Ptr<Self>) {
        this.write().lever_down = true;

        let mut timer = 0.0;
        while timer < LEVER_DOWN_DURATION.min(REEL_SPIN_DURATION) {
            timer += async_delta_time().await;
            async_next_frame().await;
        }

        this.write().lever_down = false;
    }
}

impl GameObject for SlotMachine {
    fn draw(&mut self, context: &mut GameContext) {
        context.draw.push_transform_relative(
            Transform {
                position: OFFSET.into(),
                orientation: Default::default(),
                scale: Vec3::one(),
            }
            .into(),
        );

        Sprite::single(SpriteTexture {
            sampler: "u_image".into(),
            texture: TextureRef::name("slots.png"),
            filtering: GlowTextureFiltering::Nearest,
        })
        .draw(context.draw, context.graphics);

        for (index, reel) in self.reels.iter().enumerate() {
            Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name("reel.png"),
                filtering: GlowTextureFiltering::Nearest,
            })
            .shader(ShaderRef::name("image-wrapped"))
            .region_page(Rect::new(0.0, reel.real() * 0.25, 1.0, 0.75), 0.0)
            .position(REEL_OFFSETS[index])
            .pivot(0.5.into())
            .size(Vec2::new(96.0, 288.0))
            .draw(context.draw, context.graphics);
        }

        Sprite::single(SpriteTexture {
            sampler: "u_image".into(),
            texture: TextureRef::name("machine.png"),
            filtering: GlowTextureFiltering::Nearest,
        })
        .draw(context.draw, context.graphics);

        Sprite::single(SpriteTexture {
            sampler: "u_image".into(),
            texture: if self.lever_down {
                TextureRef::name("lever-down.png")
            } else {
                TextureRef::name("lever-up.png")
            },
            filtering: GlowTextureFiltering::Nearest,
        })
        .draw(context.draw, context.graphics);

        context.draw.pop_transform();
    }
}
