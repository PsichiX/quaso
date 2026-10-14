use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    lighting::{Lighting, LightingConfig, LightingDebugView, material::LightMaterial},
    scene::{Emitter, Forward, Light, NodeId, Primitive, Scene, SceneNode},
    third_party::{
        noise::{Fbm, MultiFractal, NoiseFn, Perlin},
        spitfire_draw::{
            particles::{ParticleEmitter, ParticleInstance},
            sprite::{Sprite, SpriteTexture},
            text::Text,
            utils::TextureRef,
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::{GlowTextureFiltering, GlowTextureFormat},
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
    let instance = GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    });
    #[cfg(feature = "agent")]
    let instance = instance.with_agent_server(quaso::agent::AgentServerConfig::default());
    GameLauncher::new(instance)
        .title("Lighting")
        .config(Config::load_from_file("./resources/GameConfig.toml")?)
        .run();
    Ok(())
}

const TEXTURES: &[&str] = &[
    "paving",
    "paving-normal",
    "gravel",
    "gravel-normal",
    "asphalt",
    "asphalt-normal",
    "sidewalk",
    "sidewalk-normal",
    "yatai",
    "yatai-normal",
    "yatai-emission",
    "awning-red",
    "awning-indigo",
    "sakura",
    "yukata-a",
    "yukata-a-normal",
    "yukata-b",
    "yukata-b-normal",
    "goldfish-pool",
    "chochin",
    "chochin-emission",
    "torii-beam",
    "torii-pillar",
    "torii-pillar-normal",
    "building-a",
    "building-a-normal",
    "building-a-emission",
    "building-b",
    "building-b-normal",
    "building-b-emission",
    "neon-a",
    "neon-a-emission",
    "neon-b",
    "neon-b-emission",
    "neon-c",
    "neon-c-emission",
    "vending",
    "vending-emission",
    "car",
    "car-normal",
    "car-emission",
    "hologram",
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

const MASK_SIZE: u32 = 64;
const LIGHT_TEXEL: f32 = 8.0;
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
const LANTERN_COLOR: Rgb<f32> = Rgb::new(1.0, 0.8, 0.54);
const ROAD_END: f32 = 1500.0;
const ROAD_HALF: f32 = 100.0;
const TILE: f32 = 100.0;
/// The festival is west of this line, the city is east of it.
const BORDER: f32 = 0.0;
const TORII_X: [f32; 2] = [-1400.0, -80.0];
const STALL_X: [f32; 5] = [-1240.0, -970.0, -700.0, -430.0, -200.0];
const STALL_SIZE: Vec2<f32> = Vec2::new(240.0, 120.0);
const STALL_Y: f32 = ROAD_HALF + 75.0;
const GOLDFISH_STALL: usize = 2;
const CHOCHIN_SPACING: f32 = 42.0;
const CHOCHIN_Y: f32 = ROAD_HALF + 6.0;
const SAKURA: [Vec2<f32>; 6] = [
    Vec2::new(-1350.0, -330.0),
    Vec2::new(-1080.0, 340.0),
    Vec2::new(-840.0, -350.0),
    Vec2::new(-560.0, 330.0),
    Vec2::new(-320.0, -340.0),
    Vec2::new(-120.0, 320.0),
];
const FIREWORK_PERIOD: f32 = 2.2;
const FIREWORK_SPARKS: usize = 24;
const SIDEWALK: f32 = ROAD_HALF + 70.0;
const BUILDING_SIZE: f32 = 250.0;
const CROSS_STREET: f32 = 900.0;
const CROSS_HALF: f32 = 60.0;
const BUILDING_X: [f32; 5] = [290.0, 560.0, 1110.0, 1380.0, 1650.0];
const CAR_SPEED: f32 = 260.0;
const CAR_SIZE: Vec2<f32> = Vec2::new(110.0, 55.0);
const STEAM_VENTS: [Vec2<f32>; 3] = [
    Vec2::new(420.0, 40.0),
    Vec2::new(760.0, -50.0),
    Vec2::new(1240.0, 30.0),
];
const SHELTER: Vec2<f32> = Vec2::new(700.0, SIDEWALK - 25.0);
const HOLOGRAM: Vec2<f32> = Vec2::new(CROSS_STREET, -SIDEWALK - 40.0);
const NIGHT_KEY: f32 = 0.28;
const DAY_KEY: f32 = 0.1;

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
        context
            .assets
            .spawn(
                "shader://text",
                (ShaderAsset::new(Shader::TEXT_VERTEX, Shader::TEXT_FRAGMENT),),
            )
            .unwrap();
        context.assets.ensure("font://roboto.ttf").unwrap();
        for name in TEXTURES {
            context
                .assets
                .ensure(format!("texture://lighting/{name}.png"))
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
    /// LIGHTING_DAY sets the day time and stops the day clock, for screenshots.
    frozen_day: bool,
    /// LIGHTING_TIME sets the animation time and stops it, for screenshots.
    frozen_time: bool,
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
        let frozen = |name| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
        };
        let frozen_day = frozen("LIGHTING_DAY");
        let frozen_time = frozen("LIGHTING_TIME");
        let mut scene = Scene::default();
        let nodes = Nodes::build(&mut scene);
        Self {
            lighting: Lighting::new(LightingConfig {
                texel_world: Some(LIGHT_TEXEL),
                ..Default::default()
            })
            .with_debug_font("roboto.ttf"),
            scene,
            nodes,
            time: frozen_time.unwrap_or_default(),
            day_time: DAY_LENGTH * frozen_day.unwrap_or(0.56),
            frozen_day: frozen_day.is_some(),
            frozen_time: frozen_time.is_some(),
            noise: Perlin::new(3),
            player: Vec2::new(-260.0, 30.0),
            facing: FRAC_PI_2,
            lamp: true,
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
    stalls: Vec<(NodeId, f32)>,
    walkers: Vec<NodeId>,
    chochins: Vec<(NodeId, Vec2<f32>, f32)>,
    festival_smoke: NodeId,
    glitch_sign: NodeId,
    steam: NodeId,
    cars: Vec<(NodeId, f32)>,
    traffic_lights: Vec<NodeId>,
    hologram: NodeId,
    player: NodeId,
    lantern: NodeId,
    fireworks: Vec<NodeId>,
    fades: Vec<Fade>,
}

impl Nodes {
    /// The sibling order is the draw order of the old immediate passes.
    fn build(scene: &mut Scene) -> Self {
        let root = scene.root();
        let ground = scene.spawn(root, SceneNode::group());
        build_ground(scene, ground);
        let festival = scene.spawn(root, SceneNode::group());
        let (stalls, walkers, chochins) = build_festival(scene, festival);
        let festival_smoke = smoke(scene, festival, Rgb::new(0.55, 0.5, 0.45));
        let city = scene.spawn(root, SceneNode::group());
        let glitch_sign = build_city(scene, city);
        let steam = smoke(scene, city, Rgb::new(0.8, 0.85, 0.9));
        let cars = [1.0, -1.0]
            .into_iter()
            .map(|direction| {
                let car = lit(
                    scene,
                    city,
                    SceneNode::sprite(sprite("car", Vec2::zero(), CAR_SIZE, 0.0))
                        .rotation(direction * FRAC_PI_2),
                    LightMaterial::opaque()
                        .emission_map(asset("car-emission", "u_emission_map"))
                        .normal_map(asset("car-normal", "u_normal_map")),
                );
                scene.attach(car, Emitter::new(Rgb::white(), 10.0));
                (car, direction)
            })
            .collect();
        let traffic_lights = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0)]
            .into_iter()
            .map(|corner| {
                let position = Vec2::new(CROSS_STREET, 0.0)
                    + corner * Vec2::new(CROSS_HALF + 12.0, ROAD_HALF + 12.0);
                emitter(
                    scene,
                    city,
                    rectangle(position, 7.0.into(), Rgba::white()),
                    Rgb::white(),
                    7.5,
                )
            })
            .collect();
        let hologram = lit(
            scene,
            city,
            SceneNode::sprite(
                Sprite::single(asset("hologram", "u_image"))
                    .size(170.0.into())
                    .pivot(0.5.into()),
            )
            .position(HOLOGRAM),
            LightMaterial::filter(Rgb::new(0.4, 0.9, 1.0), 4.0)
                .absorption_map(asset("hologram", "u_absorption_map"))
                .emission_map(asset("hologram", "u_emission_map")),
        );
        scene.attach(hologram, Emitter::new(Rgb::white(), 5.0));
        let sign = scene.spawn(
            city,
            SceneNode::text(
                Text::default()
                    .font("roboto.ttf")
                    .size(30.0)
                    .text("NEO TOKYO")
                    .position(Vec2::new(BORDER + 140.0, ROAD_HALF + 20.0)),
            ),
        );
        scene.attach(
            sign,
            Light(LightMaterial::emitter(Rgb::new(1.0, 0.2, 0.6), 3.0)),
        );
        let actors = scene.spawn(root, SceneNode::group());
        let player = lit(
            scene,
            actors,
            SceneNode::sprite(sprite("player", Vec2::zero(), PLAYER_SIZE.into(), 0.0)),
            LightMaterial::opaque()
                .normal_map(asset("player-normal", "u_normal_map"))
                .absorption_map(asset("player-absorption", "u_absorption_map")),
        );
        let lantern = emitter(
            scene,
            actors,
            rectangle(Vec2::zero(), 5.0.into(), Rgba::white()),
            LANTERN_COLOR,
            10.0,
        );
        let fireworks = (0..2)
            .map(|_| {
                let node = lit(
                    scene,
                    actors,
                    SceneNode::particles(ParticleEmitter::default(), Vec::new()).visible(false),
                    LightMaterial::opaque(),
                );
                scene.attach(node, Emitter::new(Rgb::white(), 0.0));
                node
            })
            .collect();
        let overhead = scene.spawn(root, SceneNode::group());
        let fades = build_overhead(scene, overhead);
        Self {
            stalls,
            walkers,
            chochins,
            festival_smoke,
            glitch_sign,
            steam,
            cars,
            traffic_lights,
            hologram,
            player,
            lantern,
            fireworks,
            fades,
        }
    }
}

fn build_ground(scene: &mut Scene, parent: NodeId) {
    let reach = ROAD_END - VIEW_HEIGHT * 0.6 + VIEW_REACH;
    let first = (Vec2::new(-reach, -60.0 - VIEW_HEIGHT) / TILE).floor();
    let last = (Vec2::new(reach, 60.0 + VIEW_HEIGHT) / TILE).ceil();
    for y in first.y as i32..last.y as i32 {
        for x in first.x as i32..last.x as i32 {
            let position = Vec2::new(x as f32, y as f32) * TILE;
            let center = position + TILE * 0.5;
            let road = center.y.abs() < ROAD_HALF;
            let (name, shade) = match (center.x < BORDER, road) {
                (true, true) => ("paving", 0.8),
                (true, false) => ("gravel", 0.5),
                (false, true) => ("asphalt", 1.0),
                (false, false) => ("sidewalk", 0.8),
            };
            lit(
                scene,
                parent,
                SceneNode::sprite(
                    Sprite::single(asset(name, "u_image"))
                        .position(position)
                        .size((TILE + 0.5).into())
                        .tint(Rgba::new(shade, shade, shade, 1.0)),
                ),
                LightMaterial::default()
                    .normal_map(asset(&format!("{name}-normal"), "u_normal_map")),
            );
        }
    }

    for y in [-1.0, 1.0] {
        for row in 0..((VIEW_HEIGHT - ROAD_HALF) / TILE).ceil() as i32 {
            let center = Vec2::new(CROSS_STREET, y * (ROAD_HALF + TILE * (row as f32 + 0.5)));
            lit(
                scene,
                parent,
                SceneNode::sprite(
                    Sprite::single(asset("asphalt", "u_image"))
                        .position(center)
                        .size(Vec2::new(CROSS_HALF * 2.0, TILE + 0.5))
                        .pivot(0.5.into()),
                ),
                LightMaterial::default().normal_map(asset("asphalt-normal", "u_normal_map")),
            );
        }
    }

    let paint = Rgba::new(0.85, 0.85, 0.8, 1.0);
    let mut x = BORDER + 160.0;
    while x < ROAD_END + VIEW_REACH {
        if (x - CROSS_STREET).abs() > CROSS_HALF + 30.0 {
            plain(
                scene,
                parent,
                rectangle(Vec2::new(x, 0.0), Vec2::new(36.0, 3.0), paint),
            );
        }
        x += 70.0;
    }
    for stripe in 0..9 {
        let y = -ROAD_HALF + 12.0 + stripe as f32 * 22.0;
        plain(
            scene,
            parent,
            rectangle(Vec2::new(BORDER + 70.0, y), Vec2::new(70.0, 11.0), paint),
        );
    }
    let curbs = [-ROAD_HALF, ROAD_HALF].into_iter().flat_map(|y| {
        [
            (
                Vec2::new(BORDER, y - 2.0),
                Vec2::new(CROSS_STREET - CROSS_HALF, y + 2.0),
            ),
            (
                Vec2::new(CROSS_STREET + CROSS_HALF, y - 2.0),
                Vec2::new(ROAD_END + VIEW_REACH, y + 2.0),
            ),
        ]
    });
    let cross_curbs = [-CROSS_HALF, CROSS_HALF].into_iter().flat_map(|x| {
        [
            (
                Vec2::new(CROSS_STREET + x - 2.0, -VIEW_HEIGHT),
                Vec2::new(CROSS_STREET + x + 2.0, -ROAD_HALF),
            ),
            (
                Vec2::new(CROSS_STREET + x - 2.0, ROAD_HALF),
                Vec2::new(CROSS_STREET + x + 2.0, VIEW_HEIGHT),
            ),
        ]
    });
    for (from, to) in curbs.chain(cross_curbs) {
        plain(scene, parent, span(from, to, Rgba::gray(0.55)));
    }

    for vent in STEAM_VENTS {
        plain(
            scene,
            parent,
            Sprite::single(texture("disc-mask", "u_image"))
                .position(vent)
                .size(26.0.into())
                .pivot(0.5.into())
                .tint(Rgba::new(0.12, 0.12, 0.13, 1.0)),
        );
    }
}

type FestivalNodes = (
    Vec<(NodeId, f32)>,
    Vec<NodeId>,
    Vec<(NodeId, Vec2<f32>, f32)>,
);

fn build_festival(scene: &mut Scene, parent: NodeId) -> FestivalNodes {
    let mut stalls = Vec::new();
    for (index, x) in STALL_X.into_iter().enumerate() {
        for side in [-1.0, 1.0] {
            let position = Vec2::new(x, side * STALL_Y);
            if index == GOLDFISH_STALL && side < 0.0 {
                build_goldfish(scene, parent, position);
                continue;
            }
            let rotation = if side < 0.0 { 0.0 } else { PI };
            let stall = lit(
                scene,
                parent,
                SceneNode::sprite(sprite("yatai", position, STALL_SIZE, rotation)),
                LightMaterial::opaque()
                    .emission_map(asset("yatai-emission", "u_emission_map"))
                    .absorption_map(asset("yatai-emission", "u_absorption_map"))
                    .normal_map(asset("yatai-normal", "u_normal_map")),
            );
            scene.attach(stall, Emitter::new(Rgb::new(1.0, 0.7, 0.42), 7.5));
            stalls.push((stall, index as f32 * 2.0 + side));
        }
    }

    for (index, position) in SAKURA.into_iter().enumerate() {
        lit(
            scene,
            parent,
            SceneNode::sprite(rectangle(
                position + Vec2::new(10.0, 5.0) * (index as f32).cos(),
                22.0.into(),
                Rgba::new(0.25, 0.15, 0.12, 1.0),
            )),
            LightMaterial::opaque(),
        );
    }

    for x in TORII_X {
        for y in [-ROAD_HALF - 18.0, ROAD_HALF + 18.0] {
            lit(
                scene,
                parent,
                SceneNode::sprite(sprite("torii-pillar", Vec2::new(x, y), 24.0.into(), 0.0)),
                LightMaterial::opaque().normal_map(asset("torii-pillar-normal", "u_normal_map")),
            );
        }
    }

    let walkers = (0..6)
        .map(|index| {
            let name = if index % 2 == 0 {
                "yukata-a"
            } else {
                "yukata-b"
            };
            lit(
                scene,
                parent,
                SceneNode::sprite(sprite(name, Vec2::zero(), 50.0.into(), 0.0)),
                LightMaterial::opaque()
                    .normal_map(asset(&format!("{name}-normal"), "u_normal_map")),
            )
        })
        .collect();

    let mut chochins = Vec::new();
    let mut x = TORII_X[0] + 30.0;
    let mut index = 0;
    while x < TORII_X[1] - 20.0 {
        for side in [-1.0, 1.0] {
            let color = if index % 3 == 0 {
                Rgb::new(1.0, 0.91, 0.74)
            } else {
                Rgb::new(1.0, 0.58, 0.42)
            };
            let home = Vec2::new(x, side * CHOCHIN_Y);
            let chochin = lit(
                scene,
                parent,
                SceneNode::sprite(sprite("chochin", Vec2::zero(), 22.0.into(), 0.0)).position(home),
                LightMaterial::filter(Rgb::new(1.0, 0.7, 0.58), 6.0)
                    .emission_map(asset("chochin-emission", "u_emission_map")),
            );
            scene.attach(chochin, Emitter::new(color, 0.75));
            chochins.push((chochin, home, x * 0.1 + side));
        }
        x += CHOCHIN_SPACING;
        index += 1;
    }
    (stalls, walkers, chochins)
}

fn build_goldfish(scene: &mut Scene, parent: NodeId, stall: Vec2<f32>) {
    for offset in [-60.0, 60.0] {
        lit(
            scene,
            parent,
            SceneNode::sprite(sprite(
                "goldfish-pool",
                stall + Vec2::new(offset, 0.0),
                100.0.into(),
                0.0,
            )),
            LightMaterial::filter(Rgb::new(0.25, 0.7, 1.0), 25.0),
        );
    }
    lit(
        scene,
        parent,
        SceneNode::sprite(rectangle(
            stall + Vec2::new(0.0, -50.0),
            8.0.into(),
            Rgba::white(),
        )),
        LightMaterial::emitter(Rgb::new(1.0, 0.95, 0.85), 12.5),
    );
}

/// Returns the sign that glitches.
fn build_city(scene: &mut Scene, parent: NodeId) -> NodeId {
    for (index, x) in BUILDING_X.into_iter().enumerate() {
        for side in [-1.0, 1.0] {
            let position = Vec2::new(x, side * (SIDEWALK + BUILDING_SIZE * 0.5));
            let name = if (index + (side > 0.0) as usize).is_multiple_of(2) {
                "building-a"
            } else {
                "building-b"
            };
            let rotation = if side < 0.0 { 0.0 } else { PI };
            lit(
                scene,
                parent,
                SceneNode::sprite(sprite(name, position, BUILDING_SIZE.into(), rotation)),
                LightMaterial::opaque()
                    .emission(Rgb::white(), 7.5)
                    .emission_map(asset(&format!("{name}-emission"), "u_emission_map"))
                    .normal_map(asset(&format!("{name}-normal"), "u_normal_map")),
            );
        }
    }

    let signs = [
        (
            "neon-a",
            Vec2::new(380.0, -SIDEWALK + 50.0),
            Vec2::new(36.0, 108.0),
        ),
        (
            "neon-b",
            Vec2::new(540.0, SIDEWALK - 22.0),
            Vec2::new(120.0, 40.0),
        ),
        (
            "neon-c",
            Vec2::new(1060.0, -SIDEWALK + 30.0),
            Vec2::new(60.0, 60.0),
        ),
        (
            "neon-a",
            Vec2::new(1240.0, SIDEWALK - 50.0),
            Vec2::new(36.0, 108.0),
        ),
        (
            "neon-b",
            Vec2::new(1420.0, -SIDEWALK + 22.0),
            Vec2::new(120.0, 40.0),
        ),
    ];
    let signs = signs
        .into_iter()
        .map(|(name, position, size)| {
            let sign = lit(
                scene,
                parent,
                SceneNode::sprite(sprite(name, position, size, 0.0)),
                LightMaterial::opaque()
                    .emission_map(asset(&format!("{name}-emission"), "u_emission_map")),
            );
            scene.attach(sign, Emitter::new(Rgb::white(), 9.0));
            sign
        })
        .collect::<Vec<_>>();

    for (x, side) in [(340.0, 1.0), (390.0, 1.0), (980.0, -1.0), (1500.0, 1.0)] {
        lit(
            scene,
            parent,
            SceneNode::sprite(sprite(
                "vending",
                Vec2::new(x, side * (SIDEWALK - 24.0)),
                48.0.into(),
                if side < 0.0 { 0.0 } else { PI },
            )),
            LightMaterial::opaque()
                .emission(Rgb::white(), 5.0)
                .emission_map(asset("vending-emission", "u_emission_map")),
        );
    }

    let mut x = BORDER + 220.0;
    while x < ROAD_END + VIEW_REACH {
        for side in [-1.0, 1.0] {
            lit(
                scene,
                parent,
                SceneNode::sprite(rectangle(
                    Vec2::new(x, side * (ROAD_HALF + 8.0)),
                    Vec2::new(14.0, 6.0),
                    Rgba::white(),
                )),
                LightMaterial::emitter(Rgb::new(0.7, 0.85, 1.0), 4.5),
            );
        }
        x += 340.0;
    }

    for (from, to, tint) in [
        (
            SHELTER + Vec2::new(-60.0, 18.0),
            SHELTER + Vec2::new(60.0, 22.0),
            Rgb::new(0.2, 0.9, 1.0),
        ),
        (
            SHELTER + Vec2::new(-62.0, -22.0),
            SHELTER + Vec2::new(-58.0, 22.0),
            Rgb::new(1.0, 0.2, 0.8),
        ),
        (
            SHELTER + Vec2::new(58.0, -22.0),
            SHELTER + Vec2::new(62.0, 22.0),
            Rgb::new(1.0, 0.2, 0.8),
        ),
    ] {
        lit(
            scene,
            parent,
            SceneNode::sprite(span(from, to, Rgba::new(tint.r, tint.g, tint.b, 0.5))),
            LightMaterial::filter(tint, 3.0),
        );
    }
    signs[3]
}

/// Overhead nodes use the forward stage. They are see-through near the player,
/// so the stalls, the street and the player under them stay visible.
fn build_overhead(scene: &mut Scene, parent: NodeId) -> Vec<Fade> {
    let mut fades = Vec::new();
    for (index, x) in STALL_X.into_iter().enumerate() {
        for side in [-1.0, 1.0] {
            if index == GOLDFISH_STALL && side < 0.0 {
                continue;
            }
            let position = Vec2::new(x, side * STALL_Y);
            let name = if (index + (side > 0.0) as usize).is_multiple_of(2) {
                "awning-red"
            } else {
                "awning-indigo"
            };
            let node = forward(
                scene,
                parent,
                sprite(
                    name,
                    position + Vec2::new(0.0, side * 8.0),
                    STALL_SIZE + 10.0,
                    if side < 0.0 { 0.0 } else { PI },
                ),
            );
            fades.push(Fade {
                node,
                position,
                near: 70.0,
                far: 150.0,
                tint: Rgba::white(),
            });
        }
    }

    for side in [-1.0, 1.0] {
        forward(
            scene,
            parent,
            span(
                Vec2::new(TORII_X[0], side * CHOCHIN_Y - 1.0),
                Vec2::new(TORII_X[1], side * CHOCHIN_Y + 1.0),
                Rgba::new(0.1, 0.08, 0.06, 1.0),
            ),
        );
    }

    for x in TORII_X {
        forward(
            scene,
            parent,
            sprite(
                "torii-beam",
                Vec2::new(x, 0.0),
                Vec2::new(ROAD_HALF * 2.0 + 90.0, 60.0),
                FRAC_PI_2,
            ),
        );
    }

    for (index, position) in SAKURA.into_iter().enumerate() {
        let node = forward(
            scene,
            parent,
            sprite("sakura", position, 260.0.into(), index as f32 * 1.3),
        );
        fades.push(Fade {
            node,
            position,
            near: 60.0,
            far: 160.0,
            tint: Rgba::new(1.2, 1.2, 1.2, 0.95),
        });
    }

    let node = forward(
        scene,
        parent,
        rectangle(SHELTER, Vec2::new(130.0, 50.0), Rgba::white()),
    );
    fades.push(Fade {
        node,
        position: SHELTER,
        near: 40.0,
        far: 110.0,
        tint: Rgba::new(0.3, 0.8, 1.0, 0.45),
    });
    fades
}

fn lit(scene: &mut Scene, parent: NodeId, node: SceneNode, material: LightMaterial) -> NodeId {
    let node = scene.spawn(parent, node);
    scene.attach(node, Light(material));
    node
}

fn plain(scene: &mut Scene, parent: NodeId, sprite: Sprite) -> NodeId {
    scene.spawn(parent, SceneNode::sprite(sprite))
}

fn forward(scene: &mut Scene, parent: NodeId, sprite: Sprite) -> NodeId {
    let node = scene.spawn(parent, SceneNode::sprite(sprite));
    scene.attach(node, Forward);
    node
}

fn emitter(
    scene: &mut Scene,
    parent: NodeId,
    sprite: Sprite,
    color: Rgb<f32>,
    intensity: f32,
) -> NodeId {
    let node = lit(
        scene,
        parent,
        SceneNode::sprite(sprite),
        LightMaterial::opaque(),
    );
    scene.attach(node, Emitter::new(color, intensity));
    node
}

/// All puffs of one smoke node draw as one particle batch.
fn smoke(scene: &mut Scene, parent: NodeId, tint: Rgb<f32>) -> NodeId {
    lit(
        scene,
        parent,
        SceneNode::particles(
            ParticleEmitter::single(texture("fog-mask", "u_image")),
            Vec::new(),
        ),
        LightMaterial::filter(tint, 12.0)
            .absorption_map(texture("fog-density", "u_absorption_map")),
    )
}

impl State {
    fn camera(&self) -> Vec2<f32> {
        let reach = ROAD_END - VIEW_HEIGHT * 0.6;
        Vec2::new(
            self.player.x.clamp(-reach, reach),
            self.player.y.clamp(-60.0, 60.0),
        )
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
        config.sky = mix(
            Rgb::new(0.01, 0.015, 0.04),
            Rgb::new(0.21, 0.27, 0.36),
            daylight.sqrt(),
        ) + Rgb::new(0.25, 0.1, 0.03) * dusk;
        config.sun = mix(Rgb::new(1.0, 0.5, 0.2), Rgb::new(1.0, 0.95, 0.85), daylight)
            * daylight.sqrt()
            * 1.5
            + Rgb::new(0.06, 0.08, 0.14) * moonlight.sqrt();
        if let Some(adaptation) = &mut config.eye_adaptation {
            adaptation.focus = Some(self.player);
            adaptation.key = DAY_KEY + (NIGHT_KEY - DAY_KEY) * night;
        }
    }

    fn animate(&mut self) {
        let time = self.time;
        let night = self.night();
        let stall_glows = self
            .nodes
            .stalls
            .iter()
            .map(|(_, seed)| 1.0 + self.flicker(3.0, *seed) * 0.4)
            .collect::<Vec<_>>();
        let chochin_glows = self
            .nodes
            .chochins
            .iter()
            .map(|(_, _, seed)| 1.0 + self.flicker(2.0, *seed) * 0.25)
            .collect::<Vec<_>>();
        let hologram_glow = 0.85 + self.flicker(6.0, 90.0).abs() * 0.3;
        let lantern_glow = 1.0 + self.flicker(5.0, -1.0) * 0.2;
        let lantern = self.lantern_position();
        let Self {
            scene,
            nodes,
            player,
            facing,
            lamp,
            ..
        } = self;

        for ((node, _), glow) in nodes.stalls.iter().zip(stall_glows) {
            set_emitter(scene, *node, None, 7.5 * glow);
        }

        for (index, node) in nodes.walkers.iter().enumerate() {
            let seed = index as f32;
            let home = -1300.0 + seed * 210.0;
            let lane = -60.0 + (seed * 1.7).sin().abs() * 120.0;
            let phase = time * (0.12 + seed * 0.013) + seed;
            let heading = if phase.cos() >= 0.0 {
                FRAC_PI_2
            } else {
                -FRAC_PI_2
            };
            place(
                scene,
                *node,
                Vec2::new(home + phase.sin() * 90.0, lane),
                heading,
            );
        }

        for ((node, home, _), glow) in nodes.chochins.iter().zip(chochin_glows) {
            let sway = (time * 1.3 + home.x * 0.05).sin() * 1.5;
            place(scene, *node, *home + Vec2::new(sway, 0.0), 0.0);
            set_emitter(scene, *node, None, 0.75 * glow);
        }

        let puffs = STALL_X
            .into_iter()
            .enumerate()
            .filter(|(index, _)| *index != GOLDFISH_STALL)
            .map(|(index, x)| {
                let age = (time * 0.35 + index as f32 * 0.37).fract();
                (
                    Vec2::new(x + 60.0 + age * 140.0, -STALL_Y + 10.0 - age * 70.0),
                    50.0 + age * 110.0,
                    (1.0 - age) * smoothstep(0.0, 0.15, age),
                )
            });
        set_particles(
            scene,
            nodes.festival_smoke,
            puff_instances(puffs, Rgb::new(0.55, 0.5, 0.45)),
        );

        let glitch = if (time * 7.0).sin() > 0.6 { 0.15 } else { 1.0 };
        set_emitter(scene, nodes.glitch_sign, None, 9.0 * glitch);

        let puffs = STEAM_VENTS
            .into_iter()
            .enumerate()
            .flat_map(|(index, vent)| {
                (0..3).map(move |puff| {
                    let age = (time * 0.25 + puff as f32 / 3.0 + index as f32 * 0.21).fract();
                    (
                        vent + Vec2::new(age * 90.0, -age * 50.0),
                        40.0 + age * 120.0,
                        (1.0 - age) * smoothstep(0.0, 0.2, age),
                    )
                })
            });
        set_particles(
            scene,
            nodes.steam,
            puff_instances(puffs, Rgb::new(0.8, 0.85, 0.9)),
        );

        for (index, (node, direction)) in nodes.cars.iter().enumerate() {
            let travel = VIEW_HEIGHT * 2.4;
            let progress = (time * CAR_SPEED + index as f32 * travel * 0.5) % travel;
            let position = Vec2::new(
                CROSS_STREET + direction * 28.0,
                direction * (progress - travel * 0.5),
            );
            place(scene, *node, position, direction * FRAC_PI_2);
        }

        let red = ((time / 6.0).floor() as i32) % 2 == 0;
        let color = if red {
            Rgb::new(1.0, 0.1, 0.05)
        } else {
            Rgb::new(0.1, 1.0, 0.4)
        };
        for node in &nodes.traffic_lights {
            set_emitter(scene, *node, Some(color), 7.5);
        }

        if let Some(mut node) = scene.node_mut(nodes.hologram) {
            node.transform.position = HOLOGRAM + Vec2::new(0.0, (time * 0.8).sin() * 8.0);
            node.transform.rotation = (time * 0.4).sin() * 0.3;
            if let Primitive::Sprite(sprite) = &mut node.primitive {
                sprite.tint.a = 0.8 * hologram_glow;
            }
        }
        set_emitter(scene, nodes.hologram, None, 5.0 * hologram_glow);

        place(scene, nodes.player, *player, *facing);
        if let Some(mut node) = scene.node_mut(nodes.lantern) {
            node.transform.position = lantern;
            node.visible = *lamp;
        }
        set_emitter(scene, nodes.lantern, None, 10.0 * lantern_glow);

        for (back, node) in nodes.fireworks.iter().enumerate() {
            let shot = (time / FIREWORK_PERIOD).floor() - back as f32;
            let age = (time - shot * FIREWORK_PERIOD) / (FIREWORK_PERIOD * 1.6);
            let live = night > 0.0 && (0.0..1.0).contains(&age);
            if let Some(mut node) = scene.node_mut(*node) {
                node.visible = live;
            }
            if !live {
                continue;
            }
            let center = Vec2::new(
                -1300.0 + hash(shot) * 1150.0,
                (-1.0f32).powf(shot) * (170.0 + hash(shot + 0.5) * 120.0),
            );
            let color = [
                Rgb::new(1.0, 0.3, 0.5),
                Rgb::new(0.3, 0.6, 1.0),
                Rgb::new(1.0, 0.75, 0.3),
                Rgb::new(0.4, 1.0, 0.5),
            ][(hash(shot + 0.25) * 4.0) as usize % 4];
            let spread = 1.0 - (1.0 - age).powi(3);
            let fade = (1.0 - age).powi(2) * night;
            set_emitter(scene, *node, Some(color), 17.5 * fade);
            set_particles(
                scene,
                *node,
                (0..FIREWORK_SPARKS)
                    .map(|spark| {
                        let angle = spark as f32 / FIREWORK_SPARKS as f32 * TAU + shot;
                        let reach = 150.0 * spread * (0.8 + hash(shot + spark as f32) * 0.4);
                        ParticleInstance::default()
                            .position(
                                center
                                    + Vec2::new(angle.cos(), angle.sin()) * reach
                                    + Vec2::new(0.0, 30.0 * age * age),
                            )
                            .size(6.0.into())
                            .pivot(0.5.into())
                    })
                    .collect(),
            );
        }

        for fade in &nodes.fades {
            let opacity =
                smoothstep(fade.near, fade.far, fade.position.distance(*player)).max(0.15);
            if let Some(mut node) = scene.node_mut(fade.node)
                && let Primitive::Sprite(sprite) = &mut node.primitive
            {
                sprite.tint =
                    Rgba::new(fade.tint.r, fade.tint.g, fade.tint.b, fade.tint.a * opacity);
            }
        }
    }
}

fn place(scene: &mut Scene, node: NodeId, position: Vec2<f32>, rotation: f32) {
    if let Some(mut node) = scene.node_mut(node) {
        node.transform.position = position;
        node.transform.rotation = rotation;
    }
}

fn set_emitter(scene: &mut Scene, node: NodeId, color: Option<Rgb<f32>>, intensity: f32) {
    if let Some(mut emitter) = scene.attr_mut::<Emitter>(node) {
        if let Some(color) = color {
            emitter.color = color;
        }
        emitter.intensity = intensity;
    }
}

fn set_particles(scene: &mut Scene, node: NodeId, instances: Vec<ParticleInstance>) {
    if let Some(mut node) = scene.node_mut(node)
        && let Primitive::Particles(particles) = &mut node.primitive
    {
        particles.instances = instances;
    }
}

/// Each puff is a position, a size and a density.
fn puff_instances(
    puffs: impl Iterator<Item = (Vec2<f32>, f32, f32)>,
    tint: Rgb<f32>,
) -> Vec<ParticleInstance> {
    puffs
        .filter(|(_, _, density)| *density > 0.01)
        .map(|(position, size, density)| {
            ParticleInstance::default()
                .position(position)
                .size(size.into())
                .rotation(position.x * 0.01)
                .pivot(0.5.into())
                .tint(Rgba::new(tint.r, tint.g, tint.b, 0.8 * density))
        })
        .collect()
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
        let fog_mask = mask(&fog_density, |pixel| pixel[0]);
        for (name, size, bytes) in [
            ("disc-mask", MASK_SIZE, disc_mask()),
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
        self.player += movement * PLAYER_SPEED * run * delta_time;
        self.player = Vec2::new(
            self.player.x.clamp(-ROAD_END, ROAD_END),
            self.player.y.clamp(-SIDEWALK + 20.0, SIDEWALK - 20.0),
        );
        self.update_sky();
        self.animate();
        context.graphics.state.main_camera.transform.position = self.camera().into();
    }

    fn draw(&mut self, context: GameContext) {
        self.scene
            .render(&mut self.lighting, context.draw, context.graphics);
    }
}

fn asset(name: &str, sampler: &'static str) -> SpriteTexture {
    SpriteTexture {
        sampler: sampler.into(),
        texture: TextureRef::name(format!("lighting/{name}.png")),
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

fn span(from: Vec2<f32>, to: Vec2<f32>, color: Rgba<f32>) -> Sprite {
    Sprite::default().position(from).size(to - from).tint(color)
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

/// Returns a stable pseudo random value in `[0, 1)`, so every firework shot looks the same on every run.
fn hash(value: f32) -> f32 {
    ((value * 12.9898).sin() * 43_758.547).rem_euclid(1.0)
}

fn mask(bytes: &[u8], alpha: impl Fn(&[u8]) -> u8) -> Vec<u8> {
    bytes
        .chunks(4)
        .flat_map(|pixel| [255, 255, 255, alpha(pixel)])
        .collect()
}

fn disc_mask() -> Vec<u8> {
    let mut bytes = Vec::with_capacity((MASK_SIZE * MASK_SIZE * 4) as usize);
    for y in 0..MASK_SIZE {
        for x in 0..MASK_SIZE {
            let u = (x as f32 + 0.5) / MASK_SIZE as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / MASK_SIZE as f32 * 2.0 - 1.0;
            let alpha = ((1.0 - u.hypot(v)) * MASK_SIZE as f32 * 0.5).clamp(0.0, 1.0);
            bytes.extend([255, 255, 255, (alpha * 255.0) as u8]);
        }
    }
    bytes
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
