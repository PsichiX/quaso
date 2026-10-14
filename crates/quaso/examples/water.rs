use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    lighting::{
        Lighting, LightingDebugView,
        material::{LightMaterial, ShadowMaterial, WaterMaterial},
    },
    scene::{
        Emitter, Forward, Light, NodeId, Primitive, Scene, SceneNode, ShadowCaster, Silhouette,
        Unlit, Water,
    },
    third_party::{
        noise::{Fbm, MultiFractal, NoiseFn, Perlin},
        spitfire_draw::{
            particles::{ParticleEmitter, ParticleInstance},
            sprite::{Sprite, SpriteTexture},
            utils::{ShaderRef, TextureRef},
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
/// The seabed covers the widest view at the clamped camera.
const SEABED_EXTENT: f32 = 3000.0;
const PLAYER_SPEED: f32 = 140.0;
const PLAYER_RUN: f32 = 2.5;
const PLAYER_SIZE: f32 = 62.0;
/// The lantern hangs from the right hand, in front of the player and to the right.
const LANTERN_OFFSET: Vec2<f32> = Vec2::new(20.5, -21.5);
const LANTERN_COLOR: Rgb<f32> = Rgb::new(1.0, 0.8, 0.54);
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
const LIGHTHOUSE_COLOR: Rgb<f32> = Rgb::new(1.0, 0.95, 0.85);
const CAMPFIRE: Vec2<f32> = Vec2::new(-40.0, 10.0);
const FIRE_COLOR: Rgb<f32> = Rgb::new(1.0, 0.7, 0.38);
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
const LIGHTHOUSE_SHUTTERS: usize = 28;
const UMBRELLAS: [Vec2<f32>; 2] = [Vec2::new(130.0, 260.0), Vec2::new(-250.0, 260.0)];
const ROCK_ANGLES: [f32; 6] = [0.9, 2.6, 3.4, 3.75, 4.4, 5.6];
/// The fine view window uses this texel size, and the upper cascades use twice of it.
const WORLD_TEXEL: f32 = 4.0;
const NIGHT_KEY: f32 = 0.14;
const DAY_KEY: f32 = 0.12;
/// Linear lamp light spreads wider than gamma light, so a lower exposure cap keeps the night dark.
const MAX_EXPOSURE: f32 = 1.7;

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
    scene: Scene,
    nodes: Nodes,
    time: f32,
    day_time: f32,
    /// WATER_DAY sets the day time and stops the day clock, for screenshots.
    frozen_day: bool,
    /// WATER_TIME sets the animation time and stops it, for screenshots.
    frozen_time: bool,
    noise: Perlin,
    /// WATER_PLAYER ("x,y") sets the start position, for screenshots.
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
        let frozen = |name| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
        };
        let frozen_day = frozen("WATER_DAY");
        let frozen_time = frozen("WATER_TIME");
        let start = std::env::var("WATER_PLAYER")
            .ok()
            .and_then(|value| {
                let (x, y) = value.split_once(',')?;
                Some(Vec2::new(x.parse().ok()?, y.parse().ok()?))
            })
            .unwrap_or(Vec2::new(380.0, 60.0));
        let mut lighting = Lighting::default().with_debug_font("roboto.ttf");
        lighting.config.texel_world = Some(WORLD_TEXEL * 2.0);
        let mut scene = Scene::default();
        let nodes = Nodes::build(&mut scene);
        Self {
            lighting,
            scene,
            nodes,
            time: frozen_time.unwrap_or_default(),
            day_time: DAY_LENGTH * frozen_day.unwrap_or(0.42),
            frozen_day: frozen_day.is_some(),
            frozen_time: frozen_time.is_some(),
            noise: Perlin::new(5),
            player: start,
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

/// An overhead node that fades near the player. The alpha of `tint` scales the opacity.
struct Fade {
    node: NodeId,
    position: Vec2<f32>,
    near: f32,
    far: f32,
    tint: Rgba<f32>,
}

/// The nodes that change in `update`. All other nodes stay as `build` makes them.
struct Nodes {
    campfire: NodeId,
    smoke: NodeId,
    tikis: Vec<NodeId>,
    shutters: Vec<(NodeId, f32)>,
    lighthouse_lamp: NodeId,
    lighthouse: NodeId,
    pier_lamp: NodeId,
    boat: NodeId,
    player: NodeId,
    lantern: NodeId,
    sea: NodeId,
    /// Each caster keeps its day opacity. The night makes all shadows weaker.
    shadows: Vec<(NodeId, f32)>,
    fades: Vec<Fade>,
}

impl Nodes {
    /// The sibling order is the draw order of the old immediate passes.
    fn build(scene: &mut Scene) -> Self {
        let root = scene.root();
        let mut shadows = Vec::new();
        let seabed = scene.spawn(root, SceneNode::group());
        build_seabed(scene, seabed);
        let island = scene.spawn(root, SceneNode::group());
        build_island(scene, island, &mut shadows);
        let campfire = scene.spawn(
            island,
            SceneNode::sprite(sprite("campfire", Vec2::zero(), 56.0.into(), 0.0))
                .position(CAMPFIRE),
        );
        scene.attach(campfire, Light(LightMaterial::opaque()));
        scene.attach(
            campfire,
            Emitter::new(FIRE_COLOR, 26.0).map(asset("campfire-emission", "u_emission_map")),
        );
        let smoke = scene.spawn(
            island,
            SceneNode::particles(
                ParticleEmitter::single(texture("fog-mask", "u_image")),
                Vec::new(),
            )
            .position(CAMPFIRE),
        );
        scene.attach(
            smoke,
            Light(
                LightMaterial::filter(Rgb::new(0.6, 0.58, 0.55), 12.0)
                    .absorption_map(texture("fog-density", "u_absorption_map")),
            ),
        );
        let tikis = (0..6)
            .map(|index| {
                let angle = index as f32 / 6.0 * TAU + 0.3;
                emitter(
                    scene,
                    island,
                    sprite("tiki", Vec2::zero(), 22.0.into(), 0.0),
                    CAMPFIRE + from_angle(angle) * 120.0,
                    Emitter::new(FIRE_COLOR, 7.5).map(asset("tiki-emission", "u_emission_map")),
                )
            })
            .collect();
        let shutters = (0..LIGHTHOUSE_SHUTTERS)
            .map(|index| {
                let angle = index as f32 / LIGHTHOUSE_SHUTTERS as f32 * TAU;
                let shutter = scene.spawn(
                    island,
                    SceneNode::sprite(rectangle(Vec2::zero(), 6.0.into(), Rgba::gray(0.2)))
                        .position(LIGHTHOUSE + from_angle(angle) * 20.0),
                );
                scene.attach(shutter, Light(LightMaterial::opaque()));
                (shutter, angle)
            })
            .collect();
        let lighthouse_lamp = emitter(
            scene,
            island,
            rectangle(Vec2::zero(), 12.0.into(), Rgba::white()),
            LIGHTHOUSE,
            Emitter::new(LIGHTHOUSE_COLOR, 0.0),
        );

        let pier = scene.spawn(root, SceneNode::group());
        let (pier_lamp, boat) = build_pier(scene, pier, &mut shadows);

        let player = scene.spawn(
            root,
            SceneNode::sprite(sprite("player", Vec2::zero(), PLAYER_SIZE.into(), 0.0)),
        );
        scene.attach(
            player,
            Light(
                LightMaterial::opaque()
                    .normal_map(asset("player-normal", "u_normal_map"))
                    .absorption_map(asset("player-absorption", "u_absorption_map")),
            ),
        );
        let lantern = emitter(
            scene,
            root,
            rectangle(Vec2::zero(), 5.0.into(), Rgba::white()),
            Vec2::zero(),
            Emitter::new(LANTERN_COLOR, 8.0),
        );

        let sea = scene.spawn(
            root,
            SceneNode::sprite(sprite("sea-depth", Vec2::zero(), SEA_EXTENT.into(), 0.0)),
        );
        scene.attach(sea, Water(sea_material(0.0)));

        let overhead = scene.spawn(root, SceneNode::group());
        let (fades, lighthouse) = build_overhead(scene, overhead, &mut shadows);
        Self {
            campfire,
            smoke,
            tikis,
            shutters,
            lighthouse_lamp,
            lighthouse,
            pier_lamp,
            boat,
            player,
            lantern,
            sea,
            shadows,
            fades,
        }
    }
}

/// Each seabed row is a group, so the scene skips the rows outside the view.
fn build_seabed(scene: &mut Scene, parent: NodeId) {
    let tiles = (SEABED_EXTENT * 0.5 / TILE).ceil() as i32;
    for y in -tiles..tiles {
        let row = scene.spawn(parent, SceneNode::group());
        for x in -tiles..tiles {
            lit(
                scene,
                row,
                SceneNode::sprite(
                    Sprite::single(asset("seabed", "u_image")).size((TILE + 0.5).into()),
                )
                .position(Vec2::new(x as f32, y as f32) * TILE),
                LightMaterial::default().submerged(),
            );
        }
    }
    for index in 0..18 {
        let seed = index as f32;
        let angle = seed * 2.399;
        let position = from_angle(angle) * island_radius(angle) * (1.2 + hash(seed) * 0.7);
        let (name, size) = if index % 4 == 0 {
            ("starfish", 22.0)
        } else {
            ("seagrass", 120.0)
        };
        lit(
            scene,
            parent,
            SceneNode::sprite(sprite(name, Vec2::zero(), size.into(), 0.0))
                .position(position)
                .rotation(angle * 3.0),
            LightMaterial::default().submerged(),
        );
    }
}

/// Palm trunks cast a column shadow, so the shadow joins the ground to the crown.
fn build_island(scene: &mut Scene, parent: NodeId, shadows: &mut Vec<(NodeId, f32)>) {
    lit(
        scene,
        parent,
        SceneNode::sprite(sprite("island", Vec2::zero(), LAND_EXTENT.into(), 0.0)),
        LightMaterial::default().normal_map(asset("island-normal", "u_normal_map")),
    );
    for (index, angle) in ROCK_ANGLES.into_iter().enumerate() {
        let name = if index % 2 == 0 { "rock-a" } else { "rock-b" };
        lit(
            scene,
            parent,
            SceneNode::sprite(sprite(name, Vec2::zero(), 70.0.into(), 0.0))
                .position(from_angle(angle) * (island_radius(angle) + 6.0))
                .rotation(angle),
            LightMaterial::opaque().normal_map(asset(&format!("{name}-normal"), "u_normal_map")),
        );
    }
    for umbrella in UMBRELLAS {
        scene.spawn(
            parent,
            SceneNode::sprite(sprite("towel", Vec2::zero(), Vec2::new(40.0, 80.0), 0.0))
                .position(umbrella + Vec2::new(30.0, 20.0))
                .rotation(0.4),
        );
    }
    for (position, _) in PALMS {
        let trunk = lit(
            scene,
            parent,
            SceneNode::sprite(sprite("palm-trunk", Vec2::zero(), 26.0.into(), 0.0))
                .position(position),
            LightMaterial::opaque(),
        );
        scene.attach(
            trunk,
            ShadowCaster::new(ShadowMaterial::new(PALM_HEIGHT).softness(3.0))
                .shape(Silhouette::sprite(sprite(
                    "palm-trunk",
                    Vec2::zero(),
                    18.0.into(),
                    0.0,
                )))
                .column(0.08),
        );
        shadows.push((trunk, 1.0));
    }
}

fn build_pier(
    scene: &mut Scene,
    parent: NodeId,
    shadows: &mut Vec<(NodeId, f32)>,
) -> (NodeId, NodeId) {
    let width = PIER_TO.y - PIER_FROM.y;
    let mut x = PIER_FROM.x;
    while x < PIER_TO.x {
        let planks = scene.spawn(
            parent,
            SceneNode::sprite(
                Sprite::single(asset("planks", "u_image"))
                    .size(Vec2::new(width.min(PIER_TO.x - x), width)),
            )
            .position(Vec2::new(x, PIER_FROM.y)),
        );
        scene.attach(
            planks,
            ShadowCaster::new(ShadowMaterial::new(PIER_HEIGHT).softness(2.0)).erase_self(),
        );
        shadows.push((planks, 1.0));
        x += width;
    }
    let posts = ((PIER_TO.x - PIER_FROM.x) / 70.0) as i32;
    for index in 0..=posts {
        for y in [PIER_FROM.y, PIER_TO.y] {
            scene.spawn(
                parent,
                SceneNode::sprite(rectangle(
                    Vec2::zero(),
                    8.0.into(),
                    Rgba::new(0.25, 0.18, 0.12, 1.0),
                ))
                .position(Vec2::new(PIER_FROM.x + 30.0 + index as f32 * 70.0, y)),
            );
        }
    }
    let lamp = emitter(
        scene,
        parent,
        rectangle(Vec2::zero(), 7.0.into(), Rgba::white()),
        Vec2::new(PIER_TO.x - 8.0, PIER_FROM.y + 4.0),
        Emitter::new(Rgb::new(1.0, 0.91, 0.74), 0.0),
    );
    let boat = lit(
        scene,
        parent,
        SceneNode::sprite(sprite("boat", Vec2::zero(), Vec2::new(120.0, 60.0), 0.0)).position(BOAT),
        LightMaterial::opaque().normal_map(asset("boat-normal", "u_normal_map")),
    );
    scene.attach(
        boat,
        Emitter::new(Rgb::new(1.0, 0.85, 0.63), 0.0).map(asset("boat-emission", "u_emission_map")),
    );
    scene.attach(
        boat,
        ShadowCaster::new(ShadowMaterial::new(BOAT_HEIGHT).softness(4.0)).erase_self(),
    );
    shadows.push((boat, 0.8));
    (lamp, boat)
}

/// Overhead objects use the forward pass. They are see-through near the player.
/// The crowns and the umbrellas cast a softer shadow, because light passes between the fronds.
fn build_overhead(
    scene: &mut Scene,
    parent: NodeId,
    shadows: &mut Vec<(NodeId, f32)>,
) -> (Vec<Fade>, NodeId) {
    let mut fades = Vec::new();
    for umbrella in UMBRELLAS {
        let shape = sprite("umbrella", Vec2::zero(), 110.0.into(), 0.0);
        let node = scene.spawn(
            parent,
            SceneNode::sprite(shape.clone())
                .position(umbrella)
                .rotation(umbrella.x * 0.01),
        );
        scene.attach(node, Forward);
        scene.attach(
            node,
            ShadowCaster::new(ShadowMaterial::new(UMBRELLA_HEIGHT).softness(10.0))
                .shape(Silhouette::sprite(shape)),
        );
        shadows.push((node, 0.85));
        fades.push(Fade {
            node,
            position: umbrella,
            near: 40.0,
            far: 110.0,
            tint: Rgba::white(),
        });
        let pole = scene.spawn(parent, SceneNode::group().position(umbrella));
        scene.attach(
            pole,
            ShadowCaster::new(ShadowMaterial::new(UMBRELLA_HEIGHT).softness(2.0))
                .shape(Silhouette::sprite(rectangle(
                    Vec2::zero(),
                    4.0.into(),
                    Rgba::white(),
                )))
                .column(0.08),
        );
        shadows.push((pole, 1.0));
    }
    for (index, (position, variant)) in PALMS.into_iter().enumerate() {
        let name = if variant == 0 { "palm-a" } else { "palm-b" };
        let shape = sprite(name, Vec2::zero(), 230.0.into(), 0.0);
        let node = scene.spawn(
            parent,
            SceneNode::sprite(shape.clone())
                .position(position)
                .rotation(index as f32 * 1.7),
        );
        scene.attach(node, Forward);
        scene.attach(
            node,
            ShadowCaster::new(ShadowMaterial::new(PALM_HEIGHT).softness(24.0))
                .shape(Silhouette::sprite(shape)),
        );
        shadows.push((node, 0.6));
        fades.push(Fade {
            node,
            position,
            near: 50.0,
            far: 150.0,
            tint: Rgba::new(1.1, 1.1, 1.1, 0.95),
        });
    }
    let lighthouse = scene.spawn(
        parent,
        SceneNode::sprite(
            sprite("lighthouse", Vec2::zero(), 90.0.into(), 0.0)
                .shader(ShaderRef::name("image"))
                .blending(GlowBlending::Alpha),
        )
        .position(LIGHTHOUSE),
    );
    scene.attach(lighthouse, Unlit);
    scene.attach(
        lighthouse,
        ShadowCaster::new(ShadowMaterial::new(LIGHTHOUSE_HEIGHT).softness(3.0))
            .shape(Silhouette::sprite(sprite(
                "lighthouse",
                Vec2::zero(),
                80.0.into(),
                0.0,
            )))
            .column(0.08),
    );
    shadows.push((lighthouse, 1.0));
    (fades, lighthouse)
}

fn lit(scene: &mut Scene, parent: NodeId, node: SceneNode, material: LightMaterial) -> NodeId {
    let node = scene.spawn(parent, node);
    scene.attach(node, Light(material));
    node
}

fn emitter(
    scene: &mut Scene,
    parent: NodeId,
    sprite: Sprite,
    position: Vec2<f32>,
    emitter: Emitter,
) -> NodeId {
    let node = lit(
        scene,
        parent,
        SceneNode::sprite(sprite).position(position),
        LightMaterial::opaque(),
    );
    scene.attach(node, emitter);
    node
}

fn sea_material(time: f32) -> WaterMaterial {
    WaterMaterial {
        transmittance: Rgb::new(0.2, 0.55, 0.7),
        scatter: Rgb::new(0.01, 0.04, 0.1),
        ripple_strength: 0.8,
        shininess: 400.0,
        foam: 0.8,
        ripple_scale: Vec2::broadcast(SEA_EXTENT / 70.0),
        time,
        ..Default::default()
    }
}

impl State {
    fn camera(&self) -> Vec2<f32> {
        let reach =
            Vec2::broadcast(SEA_EXTENT * 0.5) - Vec2::new(VIEW_HEIGHT * 0.9, VIEW_HEIGHT * 0.5);
        Vec2::partial_max(Vec2::partial_min(self.player, reach), -reach)
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
            Rgb::new(0.21, 0.3, 0.42),
            daylight.sqrt(),
        ) + Rgb::new(0.3, 0.12, 0.04) * dusk;
        config.sun = mix(Rgb::new(1.0, 0.5, 0.2), Rgb::new(1.0, 0.95, 0.85), daylight)
            * daylight.sqrt()
            * 1.5
            + Rgb::new(0.045, 0.06, 0.105) * moonlight.sqrt();
        if let Some(adaptation) = &mut config.eye_adaptation {
            adaptation.focus = Some(self.player);
            adaptation.key = DAY_KEY + (NIGHT_KEY - DAY_KEY) * night;
            adaptation.max_exposure = MAX_EXPOSURE;
        }
    }

    fn animate(&mut self) {
        let night = self.night();
        let glow = 1.0 + self.flicker(3.0, 1.0) * 0.4;
        let tiki_glow = (0..self.nodes.tikis.len())
            .map(|index| 1.0 + self.flicker(4.0, index as f32 * 3.0) * 0.3)
            .collect::<Vec<_>>();
        let lantern_glow = 1.0 + self.flicker(5.0, -1.0) * 0.2;
        let lantern = self.lantern_position();
        let bob = Vec2::new((self.time * 0.7).sin() * 4.0, (self.time * 0.9).cos() * 3.0);
        let beam = self.time * 0.8;
        let time = self.time;
        let (player, facing, lamp) = (self.player, self.facing, self.lamp);
        let Self { scene, nodes, .. } = self;

        set_emitter(scene, nodes.campfire, 26.0 * glow);
        let puffs = (0..3)
            .map(|puff| {
                let age = (time * 0.2 + puff as f32 / 3.0).fract();
                ParticleInstance::default()
                    .position(Vec2::new(age * 120.0, -age * 60.0))
                    .size((40.0 + age * 140.0).into())
                    .rotation(age * 2.0 + puff as f32)
                    .pivot(0.5.into())
                    .tint(Rgba::new(0.6, 0.58, 0.55, 0.7 * (1.0 - age)))
            })
            .collect();
        if let Some(mut node) = scene.node_mut(nodes.smoke)
            && let Primitive::Particles(particles) = &mut node.primitive
        {
            particles.instances = puffs;
        }
        for (node, glow) in nodes.tikis.iter().zip(tiki_glow) {
            set_emitter(scene, *node, 7.5 * glow);
        }

        for (node, angle) in &nodes.shutters {
            let open = ((angle - beam + PI).rem_euclid(TAU) - PI).abs() < 0.3;
            set_visible(scene, *node, !open);
        }
        set_emitter(scene, nodes.lighthouse_lamp, 70.0 * (0.3 + night * 0.7));
        set_emitter(scene, nodes.pier_lamp, 5.5 * night);
        set_emitter(scene, nodes.boat, 5.5 * night);
        place(scene, nodes.boat, BOAT + bob, (time * 0.5).sin() * 0.05);

        place(scene, nodes.player, player, facing);
        place(scene, nodes.lantern, lantern, 0.0);
        set_visible(scene, nodes.lantern, lamp);
        set_emitter(scene, nodes.lantern, 8.0 * lantern_glow);

        if let Some(mut water) = scene.attr_mut::<Water>(nodes.sea) {
            water.0.time = time;
        }

        let strength = 1.0 - night * 0.6;
        for (node, opacity) in &nodes.shadows {
            if let Some(mut caster) = scene.attr_mut::<ShadowCaster>(*node) {
                caster.material.opacity = opacity * strength;
            }
        }
        for fade in &nodes.fades {
            let opacity = smoothstep(fade.near, fade.far, fade.position.distance(player)).max(0.15);
            set_tint(
                scene,
                fade.node,
                Rgba::new(fade.tint.r, fade.tint.g, fade.tint.b, fade.tint.a * opacity),
            );
        }
        let shade = 1.0 - night * 0.85;
        let opacity = smoothstep(40.0, 100.0, LIGHTHOUSE.distance(player)).max(0.15);
        set_tint(
            scene,
            nodes.lighthouse,
            Rgba::new(shade, shade, shade, opacity),
        );
    }
}

fn place(scene: &mut Scene, node: NodeId, position: Vec2<f32>, rotation: f32) {
    if let Some(mut node) = scene.node_mut(node) {
        node.transform.position = position;
        node.transform.rotation = rotation;
    }
}

/// Writes only a changed flag, because a write marks the light field of an occupant.
fn set_visible(scene: &mut Scene, node: NodeId, visible: bool) {
    if scene.node(node).is_some_and(|node| node.visible != visible)
        && let Some(mut node) = scene.node_mut(node)
    {
        node.visible = visible;
    }
}

fn set_emitter(scene: &mut Scene, node: NodeId, intensity: f32) {
    if let Some(mut emitter) = scene.attr_mut::<Emitter>(node) {
        emitter.intensity = intensity;
    }
}

fn set_tint(scene: &mut Scene, node: NodeId, tint: Rgba<f32>) {
    if let Some(mut node) = scene.node_mut(node)
        && let Primitive::Sprite(sprite) = &mut node.primitive
    {
        sprite.tint = tint;
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
        if !self.frozen_time {
            self.time += delta_time;
        }
        let speed = if self.fast_forward.get().is_down() {
            FAST_FORWARD
        } else {
            1.0
        };
        if !self.frozen_day {
            self.day_time = (self.day_time + delta_time * speed) % DAY_LENGTH;
        }
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
        self.animate();
        context.graphics.state.main_camera.transform.position = self.camera().into();
    }

    fn draw(&mut self, context: GameContext) {
        self.scene
            .render(&mut self.lighting, context.draw, context.graphics);
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
