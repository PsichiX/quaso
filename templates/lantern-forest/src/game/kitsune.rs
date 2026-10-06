use crate::game::forest::{Forest, map};
use quaso::{
    context::GameContext,
    lighting::material::{LightMaterial, Lit},
    third_party::{
        spitfire_draw::{
            particles::{ParticleEmitter, ParticleInstance},
            primitives::PrimitivesEmitter,
            utils::{Drawable, Vertex},
        },
        vek::{Rgb, Rgba, Vec2},
    },
};
use std::f32::consts::{FRAC_PI_2, TAU};

const SPEED: f32 = 72.0;
const WANDER_SPEED: f32 = 40.0;
const SIZE: f32 = 150.0;
const CATCH_RADIUS: f32 = 26.0;
/// The kitsune stops this far from the player. At zero distance its aim direction flips every frame.
const HOLD_DISTANCE: f32 = 20.0;
const AWAKE_DELAY: f32 = 8.0;
const FOXFIRE_COUNT: usize = 3;
const FOXFIRE_ORBIT: f32 = 42.0;
const FOXFIRE_COLOR: Rgb<f32> = Rgb::new(0.15, 0.45, 1.0);
const TATTOO_COLOR: Rgb<f32> = Rgb::new(1.0, 0.12, 0.18);
/// The body is a strip mesh along a chain of joints, from the head to the tail tips.
const JOINTS: usize = 10;
/// The angle between two chain links stays below this limit, so the body bends smoothly.
const MAX_BEND: f32 = 0.22;
const SWAY: f32 = 2.5;
const STRIDE: f32 = 0.12;

/// The kitsune hunts the player in the dark. It never enters the light of a lit
/// stone lantern or the shrine, and it fades away at dawn. Its position is the tip of its snout.
pub struct Kitsune {
    pub position: Vec2<f32>,
    facing: f32,
    time: f32,
    presence: f32,
    joints: [Vec2<f32>; JOINTS],
    stride: f32,
    gait: f32,
}

impl Kitsune {
    pub fn new(position: Vec2<f32>) -> Self {
        Self {
            position,
            facing: 0.0,
            time: 0.0,
            presence: 0.0,
            joints: std::array::from_fn(|index| position + Vec2::new(0.0, row(index) * SIZE)),
            stride: 0.0,
            gait: 0.0,
        }
    }

    /// Returns `1` when the kitsune is next to the player and `0` when it is far away.
    pub fn dread(&self, player: Vec2<f32>) -> f32 {
        (1.0 - (self.position.distance(player) - CATCH_RADIUS) / 300.0).clamp(0.0, 1.0)
            * self.presence
    }

    pub fn caught(&self, player: Vec2<f32>) -> bool {
        self.presence > 0.9 && self.position.distance(player) < CATCH_RADIUS
    }

    pub fn update(&mut self, forest: &Forest, player: Vec2<f32>, dawn: f32, delta_time: f32) {
        self.time += delta_time;
        let awake = self.time > AWAKE_DELAY && dawn <= 0.0;
        let target_presence = if awake { 1.0 } else { 0.0 };
        self.presence += (target_presence - self.presence) * (delta_time * 0.5).min(1.0);

        let wander = Vec2::new((self.time * 0.31).sin(), (self.time * 0.23).cos());
        let offset = player - self.position;
        let (direction, speed) = if awake && !forest.is_safe(player) {
            let distance = offset.magnitude();
            (
                offset.try_normalized().unwrap_or_default(),
                SPEED.min((distance - HOLD_DISTANCE).max(0.0) / delta_time.max(1.0e-4)),
            )
        } else {
            (wander, WANDER_SPEED)
        };
        let next = self.position + direction * speed * delta_time;
        let previous = self.position;
        self.position = forest.repel_spirit(next);
        let moved = self.position.distance(previous);
        self.stride += moved * STRIDE;
        let target_gait = (moved / (SPEED * delta_time.max(1.0e-4))).min(1.0);
        self.gait += (target_gait - self.gait) * (delta_time * 6.0).min(1.0);
        if direction != Vec2::zero() {
            let target = direction.y.atan2(direction.x) + FRAC_PI_2;
            let turn = (target - self.facing + std::f32::consts::PI).rem_euclid(TAU)
                - std::f32::consts::PI;
            self.facing += turn * (delta_time * 4.0).min(1.0);
        }
        self.update_joints();
    }

    /// The head joint leads, and each next joint is dragged behind the previous one at the
    /// link length, like a chain. The bend limit turns a sharp turn into a smooth curve.
    fn update_joints(&mut self) {
        let forward = self.forward();
        self.joints[0] = self.position;
        let mut previous_direction = -forward;
        for index in 1..self.joints.len() {
            let length = SIZE / (JOINTS - 1) as f32;
            let anchor = self.joints[index - 1];
            let direction = (self.joints[index] - anchor)
                .try_normalized()
                .unwrap_or(previous_direction);
            let bend = (previous_direction.x * direction.y - previous_direction.y * direction.x)
                .atan2(previous_direction.dot(direction))
                .clamp(-MAX_BEND, MAX_BEND);
            let (sin, cos) = bend.sin_cos();
            let direction = Vec2::new(
                previous_direction.x * cos - previous_direction.y * sin,
                previous_direction.x * sin + previous_direction.y * cos,
            );
            self.joints[index] = anchor + direction * length;
            previous_direction = direction;
        }
    }

    fn forward(&self) -> Vec2<f32> {
        let angle = self.facing - FRAC_PI_2;
        Vec2::new(angle.cos(), angle.sin())
    }

    pub fn draw(&self, context: &mut GameContext, forest: &Forest) {
        if self.presence <= 0.01 {
            return;
        }
        let color = [1.0, 1.0, 1.0, self.presence * 0.85];
        let edges = std::array::from_fn::<_, JOINTS, _>(|index| {
            let ahead = self.joints[index.saturating_sub(1)];
            let behind = self.joints[(index + 1).min(JOINTS - 1)];
            let direction = (ahead - behind)
                .try_normalized()
                .unwrap_or_else(|| self.forward());
            let right = Vec2::new(-direction.y, direction.x);
            let v = row(index);
            let sway = (self.stride - v * 5.0).sin() * SWAY * self.gait;
            let center = self.joints[index] + right * sway;
            [
                (center - right * SIZE * 0.5, 0.0),
                (center + right * SIZE * 0.5, 1.0),
            ]
            .map(|(position, u)| Vertex {
                position: position.into_array(),
                uv: [u, v, 0.0],
                color,
            })
        });
        let triangles = edges.windows(2).flat_map(|pair| {
            let [[a, b], [d, c]] = [pair[0], pair[1]];
            let [u, v] = average([a, b, c, d].map(|vertex| [vertex.uv[0], vertex.uv[1]]));
            let middle = Vertex {
                position: average([a, b, c, d].map(|vertex| vertex.position)),
                uv: [u, v, 0.0],
                color,
            };
            [
                [a, b, middle],
                [b, c, middle],
                [c, d, middle],
                [d, a, middle],
            ]
        });
        Lit::new(
            PrimitivesEmitter::single(map("textures/kitsune", "u_image")),
            LightMaterial::filter(Rgb::new(0.8, 0.85, 0.95), 40.0)
                .emission(
                    TATTOO_COLOR,
                    8.0 * (1.0 + (self.time * 2.0).sin() * 0.3) * self.presence,
                )
                .emission_map(map("textures/kitsune-emission", "u_emission_map"))
                .normal_map(map("textures/kitsune-normal", "u_normal_map")),
        )
        .object()
        .emit_triangles(triangles)
        .draw(context.draw, context.graphics);

        let foxfire = Lit::new(
            ParticleEmitter::single(map("textures/flame", "u_image")),
            LightMaterial::emitter(FOXFIRE_COLOR, 6.0 * self.presence).vertex_emission(),
        );
        foxfire
            .object()
            .emit((0..FOXFIRE_COUNT).map(|index| {
                let angle = self.time * 1.3 + index as f32 * TAU / FOXFIRE_COUNT as f32;
                let bob = 1.0 + (self.time * 2.1 + index as f32).sin() * 0.15;
                let flicker = 1.0 + forest.flicker(self.time, 50.0 + index as f32) * 0.4;
                ParticleInstance::default()
                    .position(
                        self.joints[JOINTS / 2]
                            + Vec2::new(angle.cos(), angle.sin()) * FOXFIRE_ORBIT * bob,
                    )
                    .size(7.0.into())
                    .pivot(0.5.into())
                    .tint(Rgba::new(flicker, flicker, flicker, 1.0))
            }))
            .draw(context.draw, context.graphics);
    }
}

fn row(joint: usize) -> f32 {
    joint as f32 / (JOINTS - 1) as f32
}

fn average(points: [[f32; 2]; 4]) -> [f32; 2] {
    points.iter().fold([0.0, 0.0], |sum, point| {
        [sum[0] + point[0] * 0.25, sum[1] + point[1] * 0.25]
    })
}
