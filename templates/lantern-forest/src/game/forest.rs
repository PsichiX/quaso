use quaso::{
    context::GameContext,
    lighting::material::{LightMaterial, Lit, Shadow, ShadowMaterial},
    third_party::{
        noise::{NoiseFn, Perlin},
        spitfire_draw::{
            particles::{ParticleEmitter, ParticleInstance},
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, TextureRef},
        },
        spitfire_glow::renderer::GlowTextureFiltering,
        vek::{Rgb, Rgba, Vec2},
    },
};
use std::f32::consts::TAU;

pub const WORLD_RADIUS: f32 = 1000.0;
pub const LANTERN_COUNT: usize = 6;
/// The player lights a stone lantern from this distance. Contact distance is about 46.
pub const LANTERN_REACH: f32 = 90.0;
const FLOOR_TILE: f32 = 320.0;
const VIEW_RADIUS: f32 = 750.0;
const FLAME_COLOR: Rgb<f32> = Rgb::new(1.0, 0.5, 0.15);
/// An unlit stone lantern still glows a little, so the player can find it in the dark.
const EMBER: f32 = 0.6;
const MUSHROOM_COLOR: Rgb<f32> = Rgb::new(0.3, 1.0, 0.85);
const FIREFLY_COLOR: Rgb<f32> = Rgb::new(0.75, 1.0, 0.3);
const PAPER_COLOR: Rgb<f32> = Rgb::new(1.0, 0.35, 0.15);
const CLEARING_RADIUS: f32 = 220.0;
/// Spirits do not enter this far into the light of a lit stone lantern.
const LANTERN_SAFE_RADIUS: f32 = 170.0;
const SHRINE_SAFE_RADIUS: f32 = 210.0;
const SHRINE: Vec2<f32> = Vec2::new(0.0, -50.0);
const TORII_Y: f32 = 110.0;
const TORII_HALF_WIDTH: f32 = 50.0;
const CHOCHINS: [Vec2<f32>; 2] = [Vec2::new(-45.0, 45.0), Vec2::new(45.0, 45.0)];
pub const PLAYER_START: Vec2<f32> = Vec2::new(0.0, 170.0);
/// The cave is a corridor of boulders that bends around `CAVE`, with its entrance toward the shrine.
/// Light does not bend around the corner, so the far end is completely dark.
/// The cave tests eye adaptation between that darkness and a bright outside.
const CAVE: Vec2<f32> = Vec2::new(-540.0, -320.0);
const CAVE_PATH_RADIUS: f32 = 170.0;
const CAVE_HALF_WIDTH: f32 = 70.0;
const CAVE_SWEEP: f32 = 4.0;
const CAVE_ROCK_SIZE: f32 = 80.0;
const CAVE_ROCK_SPACING: f32 = 34.0;
/// The cave roof is a layer of rocks over the corridor. It fades near the player like the canopies.
/// The roof top is above the light plane, so it takes the sky light, not the dark light map below it.
/// The roof draws black with its own albedo as the emission map, so the sky light is its only light.
const CAVE_ROOF_SIZE: f32 = 150.0;
const CAVE_ROOF_SPACING: f32 = 45.0;
/// The share of the sky light on the roof top. It matches the roof to the sunlit cave walls.
const ROOF_SKY_LIGHT: f32 = 0.5;

const CANOPIES: [&str; 2] = ["textures/canopy-a", "textures/canopy-b"];
const CANOPY_HEIGHT: f32 = 140.0;
const ROCKS: [(&str, &str); 2] = [
    ("textures/rock-a", "textures/rock-a-normal"),
    ("textures/rock-b", "textures/rock-b-normal"),
];

pub struct Placement {
    pub position: Vec2<f32>,
    pub rotation: f32,
    pub size: f32,
    pub variant: usize,
}

pub struct StoneLantern {
    pub position: Vec2<f32>,
    pub rotation: f32,
    pub lit: f32,
}

pub struct Obstacle {
    pub position: Vec2<f32>,
    pub radius: f32,
}

pub struct Forest {
    pub trees: Vec<Placement>,
    pub rocks: Vec<Placement>,
    pub ferns: Vec<Placement>,
    pub mushrooms: Vec<Placement>,
    pub fireflies: Vec<Placement>,
    pub lanterns: Vec<StoneLantern>,
    pub obstacles: Vec<Obstacle>,
    pub cave_roof: Vec<Placement>,
    noise: Perlin,
}

impl Forest {
    pub fn generate(seed: u64) -> Self {
        let mut random = Random(seed);
        let mut taken = vec![
            Obstacle {
                position: Vec2::zero(),
                radius: CLEARING_RADIUS,
            },
            Obstacle {
                position: CAVE,
                radius: cave_radius(),
            },
        ];

        let lanterns = (0..LANTERN_COUNT)
            .map(|index| {
                let angle = (index as f32 + random.range(-0.25, 0.25)) / LANTERN_COUNT as f32 * TAU;
                let distance = random.range(380.0, WORLD_RADIUS - 150.0);
                let position = Vec2::new(angle.cos(), angle.sin()) * distance;
                taken.push(Obstacle {
                    position,
                    radius: 90.0,
                });
                StoneLantern {
                    position,
                    rotation: random.range(-0.3, 0.3),
                    lit: 0.0,
                }
            })
            .collect::<Vec<_>>();

        let trees = random.scatter(&mut taken, 55, 80.0, |random| Placement {
            position: Vec2::zero(),
            rotation: random.range(0.0, TAU),
            size: random.range(200.0, 270.0),
            variant: (random.next() * CANOPIES.len() as f32) as usize % CANOPIES.len(),
        });
        let mut rocks = random.scatter(&mut taken, 18, 50.0, |random| Placement {
            position: Vec2::zero(),
            rotation: random.range(0.0, TAU),
            size: random.range(60.0, 110.0),
            variant: (random.next() * ROCKS.len() as f32) as usize % ROCKS.len(),
        });
        rocks.extend(cave_rocks(&mut random));
        let cave_roof = cave_roof(&mut random);

        let obstacles = trees
            .iter()
            .map(|tree| Obstacle {
                position: tree.position,
                radius: 20.0,
            })
            .chain(rocks.iter().map(|rock| Obstacle {
                position: rock.position,
                radius: rock.size * 0.38,
            }))
            .chain(lanterns.iter().map(|lantern| Obstacle {
                position: lantern.position,
                radius: 30.0,
            }))
            .chain([
                Obstacle {
                    position: SHRINE,
                    radius: 62.0,
                },
                Obstacle {
                    position: Vec2::new(-TORII_HALF_WIDTH, TORII_Y),
                    radius: 9.0,
                },
                Obstacle {
                    position: Vec2::new(TORII_HALF_WIDTH, TORII_Y),
                    radius: 9.0,
                },
            ])
            .collect();

        let mushrooms: Vec<_> = (0..18)
            .map(|_| {
                let tree = &trees[(random.next() * trees.len() as f32) as usize % trees.len()];
                let angle = random.range(0.0, TAU);
                Placement {
                    position: tree.position + Vec2::new(angle.cos(), angle.sin()) * 45.0,
                    rotation: random.range(0.0, TAU),
                    size: random.range(36.0, 52.0),
                    variant: 0,
                }
            })
            .collect();
        let outside_cave =
            |placement: &Placement| placement.position.distance(CAVE) > cave_radius();
        let mushrooms = mushrooms.into_iter().filter(outside_cave).collect();
        let ferns = (0..90)
            .map(|_| {
                let angle = random.range(0.0, TAU);
                let distance =
                    CLEARING_RADIUS + random.next().sqrt() * (WORLD_RADIUS - CLEARING_RADIUS);
                Placement {
                    position: Vec2::new(angle.cos(), angle.sin()) * distance,
                    rotation: random.range(0.0, TAU),
                    size: random.range(60.0, 100.0),
                    variant: 0,
                }
            })
            .filter(outside_cave)
            .collect();
        let fireflies = (0..60)
            .map(|index| {
                let angle = random.range(0.0, TAU);
                let distance = random.next().sqrt() * WORLD_RADIUS;
                Placement {
                    position: Vec2::new(angle.cos(), angle.sin()) * distance,
                    rotation: random.range(0.0, TAU),
                    size: random.range(2.0, 3.5),
                    variant: index,
                }
            })
            .filter(outside_cave)
            .collect();

        Self {
            trees,
            rocks,
            ferns,
            mushrooms,
            fireflies,
            lanterns,
            obstacles,
            cave_roof,
            noise: Perlin::new(seed as u32),
        }
    }

    pub fn lit_count(&self) -> usize {
        self.lanterns
            .iter()
            .filter(|lantern| lantern.lit > 0.0)
            .count()
    }

    pub fn update(&mut self, player: Vec2<f32>, delta_time: f32) {
        for lantern in &mut self.lanterns {
            if lantern.lit > 0.0 || lantern.position.distance(player) < LANTERN_REACH {
                lantern.lit = (lantern.lit + delta_time).min(1.0);
            }
        }
    }

    pub fn is_safe(&self, position: Vec2<f32>) -> bool {
        position.distance(Vec2::zero()) < SHRINE_SAFE_RADIUS
            || self.lanterns.iter().any(|lantern| {
                lantern.lit > 0.5 && lantern.position.distance(position) < LANTERN_SAFE_RADIUS
            })
    }

    pub fn repel_spirit(&self, position: Vec2<f32>) -> Vec2<f32> {
        let mut result = position;
        let lights = self
            .lanterns
            .iter()
            .filter(|lantern| lantern.lit > 0.5)
            .map(|lantern| (lantern.position, LANTERN_SAFE_RADIUS * 0.8))
            .chain([(Vec2::zero(), SHRINE_SAFE_RADIUS * 0.8)]);
        for (center, radius) in lights {
            let offset = result - center;
            let distance = offset.magnitude();
            if distance < radius {
                result = center + offset.try_normalized().unwrap_or(Vec2::unit_y()) * radius;
            }
        }
        let distance = result.magnitude();
        if distance > WORLD_RADIUS {
            result *= WORLD_RADIUS / distance;
        }
        result
    }

    pub fn collide(&self, position: Vec2<f32>, radius: f32) -> Vec2<f32> {
        let mut result = position;
        for obstacle in &self.obstacles {
            let offset = result - obstacle.position;
            let distance = offset.magnitude();
            let limit = obstacle.radius + radius;
            if distance < limit && distance > 1.0e-3 {
                result = obstacle.position + offset / distance * limit;
            }
        }
        let distance = result.magnitude();
        if distance > WORLD_RADIUS {
            result *= WORLD_RADIUS / distance;
        }
        result
    }

    pub fn flicker(&self, time: f32, phase: f32) -> f32 {
        self.noise.get([time as f64 * 5.0, phase as f64 * 7.3]) as f32
    }

    pub fn draw_ground(&self, context: &mut GameContext, camera: Vec2<f32>, time: f32) {
        let first = (camera - VIEW_RADIUS) / FLOOR_TILE;
        let last = (camera + VIEW_RADIUS) / FLOOR_TILE;
        for y in first.y.floor() as i32..=last.y.ceil() as i32 {
            for x in first.x.floor() as i32..=last.x.ceil() as i32 {
                Lit::new(
                    sprite(
                        "textures/forest-floor",
                        Vec2::new(x as f32, y as f32) * FLOOR_TILE,
                        FLOOR_TILE + 0.5,
                        0.0,
                    )
                    .pivot(0.0.into()),
                    LightMaterial::default()
                        .normal_map(map("textures/forest-floor-normal", "u_normal_map")),
                )
                .draw(context.draw, context.graphics);
            }
        }

        for fern in visible(&self.ferns, camera) {
            Lit::new(
                sprite("textures/fern", fern.position, fern.size, fern.rotation),
                LightMaterial::filter(Rgb::new(0.3, 0.6, 0.25), 10.0)
                    .normal_map(map("textures/fern-normal", "u_normal_map"))
                    .absorption_map(map("textures/fern-absorption", "u_absorption_map")),
            )
            .draw(context.draw, context.graphics);
        }

        for (index, mushroom) in visible(&self.mushrooms, camera).enumerate() {
            let pulse = 1.0 + self.flicker(time * 0.1, index as f32) * 0.5;
            Lit::new(
                sprite(
                    "textures/mushrooms",
                    mushroom.position,
                    mushroom.size,
                    mushroom.rotation,
                ),
                LightMaterial::filter(Rgb::new(0.6, 0.8, 0.7), 8.0)
                    .emission(MUSHROOM_COLOR, 0.8 * pulse)
                    .emission_map(map("textures/mushrooms-emission", "u_emission_map")),
            )
            .draw(context.draw, context.graphics);
        }
    }

    /// Fireflies fade out as `night` goes from `1` to `0`.
    pub fn draw_solids(&self, context: &mut GameContext, camera: Vec2<f32>, time: f32, night: f32) {
        for rock in visible(&self.rocks, camera) {
            let (albedo, normal) = ROCKS[rock.variant];
            Lit::new(
                sprite(albedo, rock.position, rock.size, rock.rotation),
                LightMaterial::opaque().normal_map(map(normal, "u_normal_map")),
            )
            .draw(context.draw, context.graphics);
        }

        for tree in visible(&self.trees, camera) {
            Lit::new(
                sprite("textures/trunk", tree.position, 90.0, tree.rotation),
                LightMaterial::opaque()
                    .normal_map(map("textures/trunk-normal", "u_normal_map"))
                    .absorption_map(map("textures/trunk-absorption", "u_absorption_map")),
            )
            .draw(context.draw, context.graphics);
        }

        for (index, lantern) in self.lanterns.iter().enumerate() {
            if lantern.position.distance(camera) > VIEW_RADIUS {
                continue;
            }
            let flicker = 1.0 + self.flicker(time, index as f32) * 0.35;
            Lit::new(
                sprite(
                    "textures/stone-lantern",
                    lantern.position,
                    80.0,
                    lantern.rotation,
                ),
                LightMaterial::filter(Rgb::broadcast(0.35), 25.0)
                    .normal_map(map("textures/stone-lantern-normal", "u_normal_map"))
                    .absorption_map(map("textures/stone-lantern-absorption", "u_absorption_map")),
            )
            .draw(context.draw, context.graphics);
            Lit::new(
                sprite("textures/flame", lantern.position, 16.0, 0.0),
                LightMaterial::emitter(FLAME_COLOR, EMBER + 8.0 * lantern.lit * flicker),
            )
            .draw(context.draw, context.graphics);
        }

        let fireflies = Lit::new(
            ParticleEmitter::default(),
            LightMaterial::emitter(FIREFLY_COLOR, 12.0).vertex_emission(),
        );
        fireflies
            .object()
            .emit(self.fireflies.iter().filter_map(|firefly| {
                let phase = firefly.variant as f32;
                let wander = Vec2::new(
                    self.flicker(time * 0.05, phase),
                    self.flicker(time * 0.05, phase + 100.0),
                ) * 120.0;
                let position = firefly.position + wander;
                let blink = (self.flicker(time * 0.2, phase + 200.0) * 3.0).clamp(0.0, 1.0) * night;
                (position.distance(camera) <= VIEW_RADIUS && blink > 0.0).then(|| {
                    ParticleInstance::default()
                        .position(position)
                        .size(firefly.size.into())
                        .pivot(0.5.into())
                        .tint(Rgba::new(blink, blink, blink, 1.0))
                })
            }))
            .draw(context.draw, context.graphics);
    }

    pub fn draw_shrine(&self, context: &mut GameContext, time: f32) {
        Lit::new(
            sprite("textures/shrine-roof", SHRINE, 150.0, 0.0),
            LightMaterial::opaque().normal_map(map("textures/shrine-roof-normal", "u_normal_map")),
        )
        .draw(context.draw, context.graphics);
        Lit::new(
            sprite("textures/offering", Vec2::new(0.0, 40.0), 36.0, 0.0),
            LightMaterial::opaque().normal_map(map("textures/offering-normal", "u_normal_map")),
        )
        .draw(context.draw, context.graphics);
        for x in [-TORII_HALF_WIDTH, TORII_HALF_WIDTH] {
            Lit::new(
                sprite("textures/torii-pillar", Vec2::new(x, TORII_Y), 20.0, 0.0),
                LightMaterial::opaque()
                    .normal_map(map("textures/torii-pillar-normal", "u_normal_map")),
            )
            .draw(context.draw, context.graphics);
        }
        for (index, position) in CHOCHINS.into_iter().enumerate() {
            let flicker = 1.0 + self.flicker(time, 30.0 + index as f32) * 0.25;
            Lit::new(
                sprite("textures/chochin", position, 30.0, 0.0),
                LightMaterial::filter(Rgb::new(1.0, 0.45, 0.3), 6.0)
                    .emission(PAPER_COLOR, 0.6 * flicker)
                    .emission_map(map("textures/chochin-emission", "u_emission_map")),
            )
            .draw(context.draw, context.graphics);
        }
    }

    pub fn draw_torii_beam(&self, context: &mut GameContext) {
        Lit::forward(
            sprite("textures/torii-beam", Vec2::new(0.0, TORII_Y), 1.0, 0.0)
                .size(Vec2::new(150.0, 37.5)),
            LightMaterial::default(),
        )
        .draw(context.draw, context.graphics);
    }

    /// The canopy casts a soft shadow away from the sun, and a thin column joins it to the trunk.
    pub fn draw_canopy_shadows(&self, context: &mut GameContext, camera: Vec2<f32>) {
        for tree in visible(&self.trees, camera) {
            Shadow::new(
                sprite(
                    CANOPIES[tree.variant],
                    tree.position,
                    tree.size,
                    tree.rotation,
                ),
                ShadowMaterial::new(CANOPY_HEIGHT)
                    .opacity(0.6)
                    .softness(14.0),
            )
            .draw(context.draw, context.graphics);
            for step in 1..=10 {
                Shadow::new(
                    sprite("textures/trunk", tree.position, 40.0, tree.rotation),
                    ShadowMaterial::new(CANOPY_HEIGHT * step as f32 / 10.0)
                        .opacity(0.8)
                        .softness(2.0 + step as f32 * 0.6),
                )
                .draw(context.draw, context.graphics);
            }
        }
    }

    pub fn draw_canopies(
        &self,
        context: &mut GameContext,
        camera: Vec2<f32>,
        player: Vec2<f32>,
        sky_light: Rgb<f32>,
    ) {
        for tree in visible(&self.trees, camera) {
            let distance = tree.position.distance(player);
            let opacity = smoothstep(40.0, tree.size * 0.6, distance).max(0.2) * 0.95;
            Lit::forward(
                sprite(
                    CANOPIES[tree.variant],
                    tree.position,
                    tree.size,
                    tree.rotation,
                )
                .tint(Rgba::new(1.3, 1.3, 1.3, opacity)),
                LightMaterial::default(),
            )
            .draw(context.draw, context.graphics);
        }

        for roof in visible(&self.cave_roof, camera) {
            let distance = roof.position.distance(player);
            let opacity = smoothstep(60.0, 140.0, distance);
            if opacity <= 0.0 {
                continue;
            }
            let albedo = ROCKS[roof.variant].0;
            Lit::forward(
                sprite(albedo, roof.position, roof.size, roof.rotation)
                    .tint(Rgba::new(0.0, 0.0, 0.0, opacity)),
                LightMaterial::default()
                    .emission(sky_light, ROOF_SKY_LIGHT)
                    .emission_map(map(albedo, "u_emission_map")),
            )
            .draw(context.draw, context.graphics);
        }
    }
}

pub fn sprite(texture: &'static str, position: Vec2<f32>, size: f32, rotation: f32) -> Sprite {
    Sprite::single(map(texture, "u_image"))
        .position(position)
        .size(size.into())
        .rotation(rotation)
        .pivot(0.5.into())
}

fn cave_radius() -> f32 {
    CAVE_PATH_RADIUS + CAVE_HALF_WIDTH + CAVE_ROCK_SIZE
}

fn cave_start_angle() -> f32 {
    let toward_cave = CAVE.normalized();
    (-toward_cave.x).atan2(toward_cave.y)
}

fn cave_roof(random: &mut Random) -> Vec<Placement> {
    let start = cave_start_angle();
    let count = (CAVE_PATH_RADIUS * CAVE_SWEEP / CAVE_ROOF_SPACING).ceil() as usize;
    (0..=count)
        .map(|index| {
            let angle = start + CAVE_SWEEP * index as f32 / count as f32;
            Placement {
                position: CAVE + Vec2::new(angle.cos(), angle.sin()) * CAVE_PATH_RADIUS,
                rotation: random.range(0.0, TAU),
                size: CAVE_ROOF_SIZE * random.range(0.9, 1.1),
                variant: (random.next() * ROCKS.len() as f32) as usize % ROCKS.len(),
            }
        })
        .collect()
}

fn cave_rocks(random: &mut Random) -> Vec<Placement> {
    let start = cave_start_angle();
    let end = start + CAVE_SWEEP;
    let inner = CAVE_PATH_RADIUS - CAVE_HALF_WIDTH;
    let outer = CAVE_PATH_RADIUS + CAVE_HALF_WIDTH;
    let walls = [inner, outer].into_iter().flat_map(|radius| {
        let count = (radius * CAVE_SWEEP / CAVE_ROCK_SPACING).ceil() as usize;
        (0..=count).map(move |index| (radius, start + CAVE_SWEEP * index as f32 / count as f32))
    });
    let cap_angle = end + CAVE_ROCK_SPACING / CAVE_PATH_RADIUS;
    let cap_count = ((outer - inner) / CAVE_ROCK_SPACING).ceil() as usize;
    let cap = (0..=cap_count).map(|index| {
        let radius = inner + (outer - inner) * index as f32 / cap_count as f32;
        (radius, cap_angle)
    });
    let core = [(0.0, 0.0)]
        .into_iter()
        .chain((0..6).map(|index| (inner * 0.55, index as f32 * TAU / 6.0)));
    walls
        .chain(cap)
        .chain(core)
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(radius, angle)| Placement {
            position: CAVE + Vec2::new(angle.cos(), angle.sin()) * radius,
            rotation: random.range(0.0, TAU),
            size: CAVE_ROCK_SIZE * random.range(0.9, 1.1),
            variant: (random.next() * ROCKS.len() as f32) as usize % ROCKS.len(),
        })
        .collect()
}

pub fn map(texture: &'static str, sampler: &'static str) -> SpriteTexture {
    SpriteTexture {
        sampler: sampler.into(),
        texture: TextureRef::name(texture),
        filtering: GlowTextureFiltering::LinearMipmap,
    }
}

pub fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let factor = ((value - from) / (to - from)).clamp(0.0, 1.0);
    factor * factor * (3.0 - 2.0 * factor)
}

fn visible(items: &[Placement], camera: Vec2<f32>) -> impl Iterator<Item = &Placement> {
    items
        .iter()
        .filter(move |item| item.position.distance(camera) < VIEW_RADIUS + item.size)
}

/// SplitMix64. The forest must be the same on every platform and every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^= value >> 31;
        (value >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next()
    }

    fn scatter(
        &mut self,
        taken: &mut Vec<Obstacle>,
        count: usize,
        clearance: f32,
        mut make: impl FnMut(&mut Self) -> Placement,
    ) -> Vec<Placement> {
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            for _ in 0..50 {
                let angle = self.range(0.0, TAU);
                let distance = self.next().sqrt() * WORLD_RADIUS;
                let position = Vec2::new(angle.cos(), angle.sin()) * distance;
                if taken
                    .iter()
                    .all(|other| other.position.distance(position) > other.radius + clearance)
                {
                    taken.push(Obstacle {
                        position,
                        radius: clearance,
                    });
                    result.push(Placement {
                        position,
                        ..make(self)
                    });
                    break;
                }
            }
        }
        result
    }
}
