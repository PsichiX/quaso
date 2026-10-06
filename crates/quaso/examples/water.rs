use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    lighting::{
        Lighting, LightingDebugView,
        material::{LightMaterial, Lit, Shadow, ShadowMaterial, Water, WaterMaterial},
    },
    third_party::{
        noise::{Fbm, MultiFractal, NoiseFn, Perlin},
        spitfire_draw::{
            particles::{ParticleEmitter, ParticleInstance},
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, ShaderRef, TextureRef},
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::{GlowBlending, GlowTextureFiltering, GlowTextureFormat},
        },
        spitfire_input::KeyCode,
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        vek::{Rgb, Rgba, Vec2},
    },
};
use std::{
    error::Error,
    f32::consts::{FRAC_PI_2, PI, TAU},
};

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Water")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

const TEXTURES: &[&str] = &[
    "seabed",
    "island",
    "island-normal",
    "sea-depth",
    "planks",
    "palm-a",
    "palm-b",
    "palm-trunk",
    "rock-a",
    "rock-a-normal",
    "rock-b",
    "rock-b-normal",
    "seagrass",
    "starfish",
    "boat",
    "boat-normal",
    "boat-emission",
    "umbrella",
    "towel",
    "tiki",
    "tiki-emission",
    "lighthouse",
    "campfire",
    "campfire-emission",
    "player",
    "player-normal",
    "player-absorption",
];
const VIEWS: [(KeyCode, LightingDebugView); 12] = [
    (KeyCode::Digit1, LightingDebugView::None),
    (KeyCode::Digit2, LightingDebugView::Albedo),
    (KeyCode::Digit3, LightingDebugView::Emission),
    (KeyCode::Digit4, LightingDebugView::Absorption),
    (KeyCode::Digit5, LightingDebugView::Normal),
    (KeyCode::Digit6, LightingDebugView::LightMap),
    (KeyCode::Digit7, LightingDebugView::Shadows),
    (KeyCode::Digit8, LightingDebugView::Lighting),
    (KeyCode::Digit9, LightingDebugView::Distance),
    (KeyCode::Digit0, LightingDebugView::Water),
    (KeyCode::Minus, LightingDebugView::Bloom),
    (KeyCode::Backquote, LightingDebugView::Overview),
];

const FOG_SIZE: u32 = 128;
const DAY_LENGTH: f32 = 60.0;
const FAST_FORWARD: f32 = 8.0;
const VIEW_HEIGHT: f32 = 600.0;
const VIEW_REACH: f32 = 750.0;
const PLAYER_SPEED: f32 = 140.0;
const PLAYER_RUN: f32 = 2.5;
const PLAYER_SIZE: f32 = 62.0;
/// The lantern hangs from the right hand, in front of the player and to the right.
const LANTERN_OFFSET: Vec2<f32> = Vec2::new(20.5, -21.5);
const LANTERN_COLOR: Rgb<f32> = Rgb::new(1.0, 0.6, 0.25);
/// The island shape and the two baked textures must match `island_radius` in the asset script.
const ISLAND_RADIUS: f32 = 380.0;
const LAND_EXTENT: f32 = 1000.0;
const SEA_EXTENT: f32 = 2400.0;
const TILE: f32 = 100.0;
const PIER_FROM: Vec2<f32> = Vec2::new(340.0, 32.0);
const PIER_TO: Vec2<f32> = Vec2::new(760.0, 88.0);
const BOAT: Vec2<f32> = Vec2::new(720.0, 150.0);
const PIER_HEIGHT: f32 = 20.0;
const BOAT_HEIGHT: f32 = 12.0;
const LIGHTHOUSE: Vec2<f32> = Vec2::new(-250.0, -250.0);
const LIGHTHOUSE_COLOR: Rgb<f32> = Rgb::new(1.0, 0.9, 0.7);
const CAMPFIRE: Vec2<f32> = Vec2::new(-40.0, 10.0);
const FIRE_COLOR: Rgb<f32> = Rgb::new(1.0, 0.45, 0.12);
const PALMS: [(Vec2<f32>, usize); 8] = [
    (Vec2::new(-220.0, -110.0), 0),
    (Vec2::new(-60.0, -250.0), 1),
    (Vec2::new(130.0, -200.0), 0),
    (Vec2::new(260.0, -60.0), 1),
    (Vec2::new(-280.0, 110.0), 1),
    (Vec2::new(60.0, 240.0), 0),
    (Vec2::new(-130.0, 280.0), 0),
    (Vec2::new(200.0, 140.0), 1),
];
const PALM_HEIGHT: f32 = 120.0;
const UMBRELLA_HEIGHT: f32 = 40.0;
const LIGHTHOUSE_HEIGHT: f32 = 150.0;
const UMBRELLAS: [Vec2<f32>; 2] = [Vec2::new(130.0, 260.0), Vec2::new(-250.0, 260.0)];
const ROCK_ANGLES: [f32; 6] = [0.9, 2.6, 3.4, 3.75, 4.4, 5.6];
const NIGHT_KEY: f32 = 0.16;
const DAY_KEY: f32 = 0.22;

#[derive(Default)]
struct Preloader;

impl GameState for Preloader {
    fn enter(&mut self, context: GameContext) {
        context.graphics.state.color = [0.0, 0.0, 0.0, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(VIEW_HEIGHT);

        context
            .assets
            .spawn(
                "shader://image",
                (ShaderAsset::new(
                    Shader::TEXTURED_VERTEX_2D,
                    Shader::TEXTURED_FRAGMENT,
                ),),
            )
            .unwrap();
        context.assets.ensure("font://roboto.ttf").unwrap();
        for name in TEXTURES {
            context
                .assets
                .ensure(format!("texture://water/{name}.png"))
                .unwrap();
        }
    }

    fn update(&mut self, context: GameContext, _: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(State::default()));
        }
    }
}

struct State {
    lighting: Lighting,
    time: f32,
    day_time: f32,
    noise: Perlin,
    player: Vec2<f32>,
    facing: f32,
    lamp: bool,
    movement: CardinalInputCombinator,
    toggle_lamp: InputActionRef,
    fast_forward: InputActionRef,
    run: InputActionRef,
    views: Vec<(InputActionRef, LightingDebugView)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            lighting: Lighting::default().with_debug_font("roboto.ttf"),
            time: 0.0,
            day_time: DAY_LENGTH * 0.42,
            noise: Perlin::new(5),
            player: Vec2::new(380.0, 60.0),
            facing: FRAC_PI_2,
            lamp: false,
            movement: CardinalInputCombinator::default(),
            toggle_lamp: InputActionRef::default(),
            fast_forward: InputActionRef::default(),
            run: InputActionRef::default(),
            views: Vec::new(),
        }
    }
}

impl State {
    fn camera(&self) -> Vec2<f32> {
        let reach =
            Vec2::broadcast(SEA_EXTENT * 0.5) - Vec2::new(VIEW_HEIGHT * 0.9, VIEW_HEIGHT * 0.5);
        Vec2::partial_max(Vec2::partial_min(self.player, reach), -reach)
    }

    fn visible(&self, position: Vec2<f32>, size: f32) -> bool {
        (position.x - self.camera().x).abs() < VIEW_REACH + size
            && (position.y - self.camera().y).abs() < VIEW_HEIGHT + size
    }

    fn night(&self) -> f32 {
        smoothstep(0.0, 0.25, -(self.day_time / DAY_LENGTH * TAU).sin())
    }

    fn flicker(&self, speed: f32, seed: f32) -> f32 {
        self.noise.get([(self.time * speed) as f64, seed as f64]) as f32
    }

    fn lantern_position(&self) -> Vec2<f32> {
        self.player + rotate(LANTERN_OFFSET, self.facing)
    }

    fn walkable(position: Vec2<f32>) -> bool {
        let on_pier = position.x > PIER_FROM.x
            && position.x < PIER_TO.x - 10.0
            && position.y > PIER_FROM.y + 10.0
            && position.y < PIER_TO.y - 10.0;
        on_pier || position.magnitude() < island_radius(position.y.atan2(position.x)) - 14.0
    }

    /// The sun goes from east over north to west in the first half of the day.
    /// The moon takes the same path in the second half.
    fn update_sky(&mut self) {
        let phase = self.day_time / DAY_LENGTH * TAU;
        let daylight = phase.sin().max(0.0);
        let moonlight = (-phase.sin()).max(0.0);
        let dusk = 1.0 - (phase.sin().abs() * 3.0).min(1.0);
        let path = if daylight > 0.0 { phase } else { phase - PI };
        let night = self.night();
        let config = &mut self.lighting.config;
        config.sun_direction = Vec2::new(path.cos(), -path.sin() * 0.6);
        config.shadow_length = (1.0 / daylight.max(moonlight).max(0.2) - 0.5).min(3.0);
        config.sky = mix(
            Rgb::new(0.01, 0.015, 0.04),
            Rgb::new(0.35, 0.5, 0.7),
            daylight.sqrt(),
        ) + Rgb::new(0.3, 0.12, 0.04) * dusk;
        config.sun = mix(Rgb::new(1.0, 0.5, 0.2), Rgb::new(1.0, 0.95, 0.85), daylight)
            * daylight.sqrt()
            * 1.5
            + Rgb::new(0.06, 0.08, 0.14) * moonlight.sqrt();
        if let Some(adaptation) = &mut config.eye_adaptation {
            adaptation.focus = Some(self.player);
            adaptation.key = DAY_KEY + (NIGHT_KEY - DAY_KEY) * night;
        }
    }

    /// Tall objects cast their silhouette at several heights, so the shadow joins the ground
    /// to the top. The crowns cast a softer shadow, because light passes between the fronds.
    fn draw_shadows(&self, context: &mut GameContext) {
        let strength = 1.0 - self.night() * 0.6;
        let length = self.lighting.config.shadow_length;
        for planks in pier_planks() {
            Shadow::new(
                planks,
                ShadowMaterial::new(PIER_HEIGHT)
                    .opacity(strength)
                    .softness(2.0),
            )
            .draw(context.draw, context.graphics);
        }
        Shadow::new(
            self.boat(),
            ShadowMaterial::new(BOAT_HEIGHT)
                .opacity(0.8 * strength)
                .softness(4.0),
        )
        .draw(context.draw, context.graphics);
        for planks in pier_planks() {
            Shadow::new(planks, ShadowMaterial::eraser()).draw(context.draw, context.graphics);
        }
        Shadow::new(self.boat(), ShadowMaterial::eraser()).draw(context.draw, context.graphics);
        for (index, (position, variant)) in PALMS.into_iter().enumerate() {
            let name = if variant == 0 { "palm-a" } else { "palm-b" };
            Shadow::new(
                sprite(name, position, 230.0.into(), index as f32 * 1.7),
                ShadowMaterial::new(PALM_HEIGHT)
                    .opacity(0.6 * strength)
                    .softness(24.0),
            )
            .draw(context.draw, context.graphics);
            for height in column(PALM_HEIGHT, 18.0, length) {
                Shadow::new(
                    sprite("palm-trunk", position, 18.0.into(), 0.0),
                    ShadowMaterial::new(height)
                        .opacity(strength)
                        .softness(3.0 + height * 0.08),
                )
                .draw(context.draw, context.graphics);
            }
        }
        for umbrella in UMBRELLAS {
            Shadow::new(
                sprite("umbrella", umbrella, 110.0.into(), umbrella.x * 0.01),
                ShadowMaterial::new(UMBRELLA_HEIGHT)
                    .opacity(0.85 * strength)
                    .softness(10.0),
            )
            .draw(context.draw, context.graphics);
            for height in column(UMBRELLA_HEIGHT, 4.0, length) {
                Shadow::new(
                    rectangle(umbrella, 4.0.into(), Rgba::white()),
                    ShadowMaterial::new(height)
                        .opacity(strength)
                        .softness(2.0 + height * 0.08),
                )
                .draw(context.draw, context.graphics);
            }
        }
        for height in column(LIGHTHOUSE_HEIGHT, 80.0, length) {
            Shadow::new(
                sprite("lighthouse", LIGHTHOUSE, 80.0.into(), 0.0),
                ShadowMaterial::new(height)
                    .opacity(strength)
                    .softness(3.0 + height * 0.08),
            )
            .draw(context.draw, context.graphics);
        }
    }

    fn draw_seabed(&self, context: &mut GameContext) {
        let camera = self.camera();
        let first = ((camera - Vec2::new(VIEW_REACH, VIEW_HEIGHT)) / TILE).floor();
        let last = ((camera + Vec2::new(VIEW_REACH, VIEW_HEIGHT)) / TILE).ceil();
        for y in first.y as i32..last.y as i32 {
            for x in first.x as i32..last.x as i32 {
                Lit::new(
                    Sprite::single(asset("seabed", "u_image"))
                        .position(Vec2::new(x as f32, y as f32) * TILE)
                        .size((TILE + 0.5).into()),
                    LightMaterial::default().submerged(),
                )
                .draw(context.draw, context.graphics);
            }
        }

        for index in 0..18 {
            let seed = index as f32;
            let angle = seed * 2.399;
            let position = from_angle(angle) * island_radius(angle) * (1.2 + hash(seed) * 0.7);
            if self.visible(position, 100.0) {
                let (name, size) = if index % 4 == 0 {
                    ("starfish", 22.0)
                } else {
                    ("seagrass", 120.0)
                };
                Lit::new(
                    sprite(name, position, size.into(), angle * 3.0),
                    LightMaterial::default().submerged(),
                )
                .draw(context.draw, context.graphics);
            }
        }
    }

    fn draw_island(&self, context: &mut GameContext) {
        Lit::new(
            sprite("island", Vec2::zero(), LAND_EXTENT.into(), 0.0),
            LightMaterial::default().normal_map(asset("island-normal", "u_normal_map")),
        )
        .draw(context.draw, context.graphics);

        for (index, angle) in ROCK_ANGLES.into_iter().enumerate() {
            let position = from_angle(angle) * (island_radius(angle) + 6.0);
            let name = if index % 2 == 0 { "rock-a" } else { "rock-b" };
            Lit::new(
                sprite(name, position, 70.0.into(), angle),
                LightMaterial::opaque()
                    .normal_map(asset(&format!("{name}-normal"), "u_normal_map")),
            )
            .draw(context.draw, context.graphics);
        }

        for umbrella in UMBRELLAS {
            Lit::new(
                sprite(
                    "towel",
                    umbrella + Vec2::new(30.0, 20.0),
                    Vec2::new(40.0, 80.0),
                    0.4,
                ),
                LightMaterial::default(),
            )
            .draw(context.draw, context.graphics);
        }

        for (position, _) in PALMS {
            Lit::new(
                sprite("palm-trunk", position, 26.0.into(), 0.0),
                LightMaterial::opaque(),
            )
            .draw(context.draw, context.graphics);
        }

        let glow = 1.0 + self.flicker(3.0, 1.0) * 0.4;
        Lit::new(
            sprite("campfire", CAMPFIRE, 56.0.into(), 0.0),
            LightMaterial::opaque()
                .emission(FIRE_COLOR, 10.0 * glow)
                .emission_map(asset("campfire-emission", "u_emission_map")),
        )
        .draw(context.draw, context.graphics);
        let smoke = Lit::new(
            ParticleEmitter::single(texture("fog-mask", "u_image")),
            LightMaterial::filter(Rgb::new(0.6, 0.58, 0.55), 12.0)
                .absorption_map(texture("fog-density", "u_absorption_map")),
        );
        smoke
            .object()
            .emit((0..3).map(|puff| {
                let age = (self.time * 0.2 + puff as f32 / 3.0).fract();
                ParticleInstance::default()
                    .position(CAMPFIRE + Vec2::new(age * 120.0, -age * 60.0))
                    .size((40.0 + age * 140.0).into())
                    .rotation(age * 2.0 + puff as f32)
                    .pivot(0.5.into())
                    .tint(Rgba::new(0.6, 0.58, 0.55, 0.7 * (1.0 - age)))
            }))
            .draw(context.draw, context.graphics);

        for index in 0..6 {
            let angle = index as f32 / 6.0 * TAU + 0.3;
            let position = CAMPFIRE + from_angle(angle) * 120.0;
            let flicker = 1.0 + self.flicker(4.0, index as f32 * 3.0) * 0.3;
            Lit::new(
                sprite("tiki", position, 22.0.into(), 0.0),
                LightMaterial::opaque()
                    .emission(FIRE_COLOR, 8.0 * flicker)
                    .emission_map(asset("tiki-emission", "u_emission_map")),
            )
            .draw(context.draw, context.graphics);
        }

        self.draw_lighthouse_lamp(context);
    }

    /// The lamp sits inside a ring of opaque shutters with one gap.
    /// The gap turns, so the radiance cascades cast a sweeping beam.
    fn draw_lighthouse_lamp(&self, context: &mut GameContext) {
        let beam = self.time * 0.8;
        let shutters = 28;
        for index in 0..shutters {
            let angle = index as f32 / shutters as f32 * TAU;
            let gap = ((angle - beam + PI).rem_euclid(TAU) - PI).abs();
            if gap < 0.3 {
                continue;
            }
            Lit::new(
                rectangle(
                    LIGHTHOUSE + from_angle(angle) * 20.0,
                    6.0.into(),
                    Rgba::gray(0.2),
                ),
                LightMaterial::opaque(),
            )
            .draw(context.draw, context.graphics);
        }
        Lit::new(
            rectangle(LIGHTHOUSE, 12.0.into(), Rgba::white()),
            LightMaterial::emitter(LIGHTHOUSE_COLOR, 40.0 * (0.3 + self.night() * 0.7)),
        )
        .draw(context.draw, context.graphics);
    }

    fn draw_pier(&self, context: &mut GameContext) {
        let length = PIER_TO.x - PIER_FROM.x;
        for planks in pier_planks() {
            Lit::new(planks, LightMaterial::default()).draw(context.draw, context.graphics);
        }
        let posts = (length / 70.0) as i32;
        for index in 0..=posts {
            for y in [PIER_FROM.y, PIER_TO.y] {
                Lit::new(
                    rectangle(
                        Vec2::new(PIER_FROM.x + 30.0 + index as f32 * 70.0, y),
                        8.0.into(),
                        Rgba::new(0.25, 0.18, 0.12, 1.0),
                    ),
                    LightMaterial::default(),
                )
                .draw(context.draw, context.graphics);
            }
        }
        let lamp = Vec2::new(PIER_TO.x - 8.0, PIER_FROM.y + 4.0);
        Lit::new(
            rectangle(lamp, 7.0.into(), Rgba::white()),
            LightMaterial::emitter(Rgb::new(1.0, 0.8, 0.5), 10.0 * self.night()),
        )
        .draw(context.draw, context.graphics);

        Lit::new(
            self.boat(),
            LightMaterial::opaque()
                .emission(Rgb::new(1.0, 0.7, 0.35), 12.0 * self.night())
                .emission_map(asset("boat-emission", "u_emission_map"))
                .normal_map(asset("boat-normal", "u_normal_map")),
        )
        .draw(context.draw, context.graphics);
    }

    fn boat(&self) -> Sprite {
        let bob = Vec2::new((self.time * 0.7).sin() * 4.0, (self.time * 0.9).cos() * 3.0);
        sprite(
            "boat",
            BOAT + bob,
            Vec2::new(120.0, 60.0),
            (self.time * 0.5).sin() * 0.05,
        )
    }

    fn draw_player(&self, context: &mut GameContext) {
        Lit::new(
            sprite("player", self.player, PLAYER_SIZE.into(), self.facing),
            LightMaterial::opaque()
                .normal_map(asset("player-normal", "u_normal_map"))
                .absorption_map(asset("player-absorption", "u_absorption_map")),
        )
        .draw(context.draw, context.graphics);
        if self.lamp {
            let flicker = 1.0 + self.flicker(5.0, -1.0) * 0.2;
            Lit::new(
                rectangle(self.lantern_position(), 5.0.into(), Rgba::white()),
                LightMaterial::emitter(LANTERN_COLOR, 8.0 * flicker),
            )
            .draw(context.draw, context.graphics);
        }
    }

    fn draw_water(&self, context: &mut GameContext) {
        Water::new(
            sprite("sea-depth", Vec2::zero(), SEA_EXTENT.into(), 0.0),
            WaterMaterial {
                transmittance: Rgb::new(0.2, 0.55, 0.7),
                scatter: Rgb::new(0.01, 0.04, 0.1),
                ripple_strength: 0.8,
                shininess: 400.0,
                foam: 0.8,
                ripple_scale: Vec2::broadcast(SEA_EXTENT / 70.0),
                time: self.time,
                ..Default::default()
            },
        )
        .draw(context.draw, context.graphics);
    }

    /// Overhead objects use the forward pass. They are see-through near the player.
    fn draw_overhead(&self, context: &mut GameContext) {
        for umbrella in UMBRELLAS {
            Lit::forward(
                sprite("umbrella", umbrella, 110.0.into(), umbrella.x * 0.01).tint(Rgba::new(
                    1.0,
                    1.0,
                    1.0,
                    self.fade(umbrella, 40.0, 110.0),
                )),
                LightMaterial::default(),
            )
            .draw(context.draw, context.graphics);
        }
        for (index, (position, variant)) in PALMS.into_iter().enumerate() {
            let name = if variant == 0 { "palm-a" } else { "palm-b" };
            Lit::forward(
                sprite(name, position, 230.0.into(), index as f32 * 1.7).tint(Rgba::new(
                    1.1,
                    1.1,
                    1.1,
                    self.fade(position, 50.0, 150.0) * 0.95,
                )),
                LightMaterial::default(),
            )
            .draw(context.draw, context.graphics);
        }
        let shade = 1.0 - self.night() * 0.85;
        sprite("lighthouse", LIGHTHOUSE, 90.0.into(), 0.0)
            .tint(Rgba::new(
                shade,
                shade,
                shade,
                self.fade(LIGHTHOUSE, 40.0, 100.0),
            ))
            .shader(ShaderRef::name("image"))
            .blending(GlowBlending::Alpha)
            .draw(context.draw, context.graphics);
    }

    fn fade(&self, position: Vec2<f32>, near: f32, far: f32) -> f32 {
        smoothstep(near, far, position.distance(self.player)).max(0.15)
    }
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        let move_left = InputActionRef::default();
        let move_right = InputActionRef::default();
        let move_up = InputActionRef::default();
        let move_down = InputActionRef::default();
        self.movement = CardinalInputCombinator::new(
            move_left.clone(),
            move_right.clone(),
            move_up.clone(),
            move_down.clone(),
        );
        let mut mapping = InputMapping::default()
            .consume(InputConsume::Hit)
            .action(VirtualAction::KeyButton(KeyCode::KeyA), move_left)
            .action(VirtualAction::KeyButton(KeyCode::KeyD), move_right)
            .action(VirtualAction::KeyButton(KeyCode::KeyW), move_up)
            .action(VirtualAction::KeyButton(KeyCode::KeyS), move_down)
            .action(
                VirtualAction::KeyButton(KeyCode::KeyF),
                self.toggle_lamp.clone(),
            )
            .action(
                VirtualAction::KeyButton(KeyCode::Space),
                self.fast_forward.clone(),
            )
            .action(
                VirtualAction::KeyButton(KeyCode::ShiftLeft),
                self.run.clone(),
            );
        for (key, view) in VIEWS {
            let action = InputActionRef::default();
            mapping = mapping.action(VirtualAction::KeyButton(key), action.clone());
            self.views.push((action, view));
        }
        context.input.push_mapping(mapping);

        let fog_density = fog_density();
        let fog_mask = fog_density
            .chunks(4)
            .flat_map(|pixel| [255, 255, 255, pixel[0]])
            .collect::<Vec<_>>();
        for (name, size, bytes) in [
            ("fog-density", FOG_SIZE, fog_density),
            ("fog-mask", FOG_SIZE, fog_mask),
        ] {
            if let Ok(texture) =
                context
                    .graphics
                    .texture(size, size, 1, GlowTextureFormat::Rgba, Some(&bytes))
            {
                context.draw.textures.insert(name.into(), texture);
            }
        }
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn update(&mut self, context: GameContext, delta_time: f32) {
        self.time += delta_time;
        let speed = if self.fast_forward.get().is_down() {
            FAST_FORWARD
        } else {
            1.0
        };
        self.day_time = (self.day_time + delta_time * speed) % DAY_LENGTH;
        if self.toggle_lamp.get().is_pressed() {
            self.lamp = !self.lamp;
        }
        for (action, view) in &self.views {
            if action.get().is_pressed() {
                self.lighting.debug_view = *view;
            }
        }
        let movement = Vec2::<f32>::from(self.movement.get())
            .try_normalized()
            .unwrap_or_default();
        if movement != Vec2::zero() {
            let target = movement.y.atan2(movement.x) + FRAC_PI_2;
            let turn = (target - self.facing + PI).rem_euclid(TAU) - PI;
            self.facing += turn * (10.0 * delta_time).min(1.0);
        }
        let run = if self.run.get().is_down() {
            PLAYER_RUN
        } else {
            1.0
        };
        let step = movement * PLAYER_SPEED * run * delta_time;
        for candidate in [
            self.player + step,
            self.player + Vec2::new(step.x, 0.0),
            self.player + Vec2::new(0.0, step.y),
        ] {
            if Self::walkable(candidate) {
                self.player = candidate;
                break;
            }
        }
        self.update_sky();
        context.graphics.state.main_camera.transform.position = self.camera().into();
    }

    fn draw(&mut self, mut context: GameContext) {
        self.lighting.begin(context.draw, context.graphics);
        self.draw_seabed(&mut context);
        self.draw_island(&mut context);
        self.draw_pier(&mut context);
        self.draw_player(&mut context);
        self.lighting.begin_shadows(context.draw, context.graphics);
        self.draw_shadows(&mut context);
        self.lighting.end_shadows(context.draw, context.graphics);
        self.lighting.resolve(context.draw, context.graphics);
        self.draw_water(&mut context);
        self.draw_overhead(&mut context);
        self.lighting.end(context.draw, context.graphics);
    }
}

/// The shore radius at a world angle. The asset script bakes the island textures with the same formula.
fn island_radius(angle: f32) -> f32 {
    ISLAND_RADIUS
        * (1.0
            + 0.12 * (3.0 * angle + 0.7).sin()
            + 0.08 * (5.0 * angle + 2.1).sin()
            + 0.05 * (7.0 * angle + 4.0).sin())
}

/// The column copies stay closer than a third of the caster size along the shadow,
/// so a long evening shadow does not break into separate dots.
fn column(height: f32, size: f32, shadow_length: f32) -> impl Iterator<Item = f32> {
    let step = size * 0.3 / shadow_length.max(0.1);
    let count = (height / step).ceil() as i32;
    (0..=count).map(move |index| height * index as f32 / count as f32)
}

fn from_angle(angle: f32) -> Vec2<f32> {
    Vec2::new(angle.cos(), angle.sin())
}

fn asset(name: &str, sampler: &'static str) -> SpriteTexture {
    SpriteTexture {
        sampler: sampler.into(),
        texture: TextureRef::name(format!("water/{name}.png")),
        filtering: GlowTextureFiltering::Linear,
    }
}

fn texture(name: &'static str, sampler: &'static str) -> SpriteTexture {
    SpriteTexture {
        sampler: sampler.into(),
        texture: TextureRef::name(name),
        filtering: GlowTextureFiltering::Linear,
    }
}

fn sprite(name: &str, position: Vec2<f32>, size: Vec2<f32>, rotation: f32) -> Sprite {
    Sprite::single(asset(name, "u_image"))
        .position(position)
        .size(size)
        .rotation(rotation)
        .pivot(0.5.into())
}

fn rectangle(position: Vec2<f32>, size: Vec2<f32>, color: Rgba<f32>) -> Sprite {
    Sprite::default()
        .position(position)
        .size(size)
        .pivot(0.5.into())
        .tint(color)
}

fn rotate(vector: Vec2<f32>, angle: f32) -> Vec2<f32> {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(
        vector.x * cos - vector.y * sin,
        vector.x * sin + vector.y * cos,
    )
}

fn mix(from: Rgb<f32>, to: Rgb<f32>, factor: f32) -> Rgb<f32> {
    from * (1.0 - factor) + to * factor
}

fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let factor = ((value - from) / (to - from)).clamp(0.0, 1.0);
    factor * factor * (3.0 - 2.0 * factor)
}

/// Returns a stable pseudo random value in `[0, 1)`, so the scene is the same on every run.
fn hash(value: f32) -> f32 {
    ((value * 12.9898).sin() * 43_758.547).rem_euclid(1.0)
}

fn fog_density() -> Vec<u8> {
    let noise = Fbm::<Perlin>::new(7).set_octaves(4).set_frequency(4.0);
    let mut bytes = Vec::with_capacity((FOG_SIZE * FOG_SIZE * 4) as usize);
    for y in 0..FOG_SIZE {
        for x in 0..FOG_SIZE {
            let u = (x as f64 + 0.5) / FOG_SIZE as f64;
            let v = (y as f64 + 0.5) / FOG_SIZE as f64;
            let radius = ((u - 0.5) * 2.0).hypot((v - 0.5) * 2.0);
            let falloff = (1.0 - radius).clamp(0.0, 1.0);
            let density = ((noise.get([u, v]) * 0.5 + 0.5) * falloff * 2.0).clamp(0.0, 1.0);
            let value = (density * 255.0) as u8;
            bytes.extend([value, value, value, 255]);
        }
    }
    bytes
}

fn pier_planks() -> impl Iterator<Item = Sprite> {
    let width = PIER_TO.y - PIER_FROM.y;
    std::iter::successors(Some(PIER_FROM.x), move |x| Some(x + width))
        .take_while(|x| *x < PIER_TO.x)
        .map(move |x| {
            Sprite::single(asset("planks", "u_image"))
                .position(Vec2::new(x, PIER_FROM.y))
                .size(Vec2::new(width.min(PIER_TO.x - x), width))
        })
}
