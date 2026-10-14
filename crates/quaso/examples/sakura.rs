use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    lighting::{
        Lighting, LightingDebugView, LightingStyle,
        color::srgb_to_linear,
        material::{LightMaterial, ShadowMaterial, WaterMaterial},
    },
    scene::{
        Emitter, Forward, Light, NodeId, Primitive, Scene, SceneNode, ShadowCaster, Silhouette,
        Water,
    },
    third_party::{
        noise::{NoiseFn, Perlin},
        spitfire_draw::{
            particles::{ParticleEmitter, ParticleInstance},
            sprite::{Sprite, SpriteTexture},
            utils::TextureRef,
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::{GlowTextureFiltering, GlowUniformValue},
        },
        spitfire_input::KeyCode,
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        vek::{Rect, Rgb, Rgba, Vec2},
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
    .title("Sakura")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

const TEXTURES: &[&str] = &[
    "grass",
    "path",
    "sakura-a",
    "sakura-b",
    "trunk",
    "blanket",
    "jubako",
    "tea-set",
    "sake",
    "chochin",
    "girl",
    "boy",
    "mother",
    "father",
    "grandmother",
    "grandfather",
    "student",
    "photographer",
    "petals",
    "river-depth",
    "river-bed",
    "bridge",
    "koi-a",
    "koi-b",
    "lily",
];
const VIEWS: [(KeyCode, LightingDebugView); 11] = [
    (KeyCode::Digit1, LightingDebugView::None),
    (KeyCode::Digit2, LightingDebugView::Albedo),
    (KeyCode::Digit3, LightingDebugView::Emission),
    (KeyCode::Digit4, LightingDebugView::Absorption),
    (KeyCode::Digit5, LightingDebugView::Normal),
    (KeyCode::Digit6, LightingDebugView::LightMap),
    (KeyCode::Digit7, LightingDebugView::Shadows),
    (KeyCode::Digit8, LightingDebugView::Lighting),
    (KeyCode::Digit9, LightingDebugView::Distance),
    (KeyCode::Minus, LightingDebugView::Bloom),
    (KeyCode::Backquote, LightingDebugView::Overview),
];

/// The light snaps to bands of `1 / u_bands` stops, so a lantern draws flat rings of light
/// and a shadow gets a hard edge. The bands are in log space, so they look the same by day
/// and at night. `u_band_edge` is the width of the soft step between two bands.
/// The height shadows also turn the light to `u_shade_tint`, a cool anime shade color.
const SHADE: &str = r#"
uniform float u_bands;
uniform float u_band_edge;
uniform vec3 u_shade_tint;

vec3 shade(Surface surface) {
    float luminance = max(dot(surface.light, vec3(0.2126, 0.7152, 0.0722)), 1.0e-5);
    float stops = log2(luminance) * u_bands;
    float band = floor(stops) + smoothstep(0.5 - u_band_edge, 0.5 + u_band_edge, fract(stops));
    vec3 light = surface.light / luminance * exp2(band / u_bands);
    float shadow = smoothstep(0.3, 0.6, surface.shadow);
    return surface.albedo * light * mix(vec3(1.0), u_shade_tint, shadow);
}
"#;

/// The tone curve maps only the luminance and keeps the hue and saturation, so colors stay
/// vivid. Near the top of the curve the color fades to white, so bright lights burn out
/// like anime highlights. `u_lift` raises the blacks to a soft lavender, and `u_vignette`
/// darkens the corners.
const GRADE: &str = r#"
uniform float u_saturation;
uniform vec3 u_lift;
uniform float u_vignette;

float grade_luminance(vec3 color) {
    return dot(color, vec3(0.2126, 0.7152, 0.0722));
}

vec3 grade(vec3 color, vec2 screen_uv) {
    float luminance = max(grade_luminance(color), 1.0e-5);
    float mapped = clamp(
        luminance * (2.51 * luminance + 0.03) / (luminance * (2.43 * luminance + 0.59) + 0.14),
        0.0,
        1.0
    );
    vec3 result = max(color * (mapped / luminance), vec3(0.0));
    result = max(mix(vec3(grade_luminance(result)), result, u_saturation), vec3(0.0));
    result = mix(result, vec3(mapped), smoothstep(0.7, 1.0, mapped));
    result = min(result, vec3(1.0));
    result += u_lift * (1.0 - result);
    vec2 offset = screen_uv - 0.5;
    return result * (1.0 - u_vignette * dot(offset, offset) * 2.0);
}
"#;

/// The pass draws ink around every object that absorbs light. A person absorbs only a little,
/// so the person is lit like the ground under it, but the pass still finds the edge.
/// The outer side of an edge gets an ink line, and the inner side gets a rim of the light
/// that reaches the object from outside.
const OUTLINE: &str = r#"
uniform vec3 u_ink;
uniform float u_ink_width;
uniform float u_rim;

float object_mask(vec2 scene_uv) {
    return smoothstep(1.0e-4, 1.0e-3, scene_opacity(scene_uv));
}

vec4 pass_color(vec2 screen_uv, vec2 scene_uv) {
    vec4 color = source_color(screen_uv);
    vec2 reach = scene_pixel() * u_ink_width;
    float center = object_mask(scene_uv);
    float most = center;
    float least = center;
    for (int index = 0; index < 8; ++index) {
        float angle = float(index) * 0.7853982;
        float mask = object_mask(scene_uv + vec2(cos(angle), sin(angle)) * reach);
        most = max(most, mask);
        least = min(least, mask);
    }
    float edge = most - least;
    vec3 rim = scene_albedo(scene_uv).rgb * scene_light(scene_uv) * u_rim;
    color.rgb = mix(color.rgb, color.rgb * u_ink, edge * (1.0 - center));
    color.rgb += rim * edge * center;
    return color;
}
"#;

/// The water gets flat depth bands, lit like the ground, with the bed showing in the shallows.
/// Thin light lines drift with the flow, and the glints snap to hard white stars.
/// The foam becomes a clean white line along the bank.
const WATER: &str = r#"
uniform vec3 u_water_shallow;
uniform vec3 u_water_deep;
uniform float u_water_lines;
uniform float u_water_flow;

float water_line(vec2 position) {
    float value = gradient_noise(position);
    return 1.0 - smoothstep(0.015, 0.035, abs(value));
}

vec4 water_color(WaterSurface water) {
    float steps = water.depth * 3.0;
    float band = (floor(steps) + smoothstep(0.45, 0.55, fract(steps))) / 3.0;
    vec3 tint = mix(u_water_shallow, u_water_deep, band) * water.light;
    vec3 body = mix(water.bed, tint, 0.2 + 0.45 * band);
    vec3 color = mix(body, water.reflection, water.fresnel * 0.5);
    vec2 flow = water.ripple_uv - vec2(u_water_flow * u_time, 0.0);
    float lines = water_line(flow * vec2(0.18, 0.7)) * smoothstep(-0.1, 0.25, gradient_noise(flow * 0.11));
    color += water.light * lines * smoothstep(0.15, 0.4, water.depth) * u_water_lines;
    float glint = smoothstep(0.6, 0.7, dot(water.glint, vec3(0.2126, 0.7152, 0.0722)));
    color = mix(color, water.light * 3.0, glint);
    color += water.halo;
    color *= water.shadow_dim;
    float foam = smoothstep(0.4, 0.5, water.foam);
    color = mix(color, water.light * 1.3, foam);
    return vec4(color, max(smoothstep(0.0, 0.1, water.depth), foam) * water.shoreline);
}
"#;

const DAY_LENGTH: f32 = 120.0;
const FAST_FORWARD: f32 = 8.0;
const VIEW_HEIGHT: f32 = 600.0;
const LIGHT_TEXEL: f32 = 8.0;
const VIEW_REACH: f32 = 750.0;
const WORLD: Vec2<f32> = Vec2::new(1400.0, 900.0);
const TILE: f32 = 200.0;
/// The ground covers the widest view at the clamped camera.
const GROUND_EXTENT: Vec2<f32> = Vec2::new(1600.0, 1200.0);
const PATH_WIDTH: f32 = 90.0;
const PLAYER_SPEED: f32 = 120.0;
const PLAYER_RUN: f32 = 2.2;
const PLAYER_SIZE: f32 = 84.0;
const TREE_ROWS: [f32; 3] = [-210.0, 210.0, 880.0];
const TREE_SPACING: f32 = 320.0;
const TREE_COUNT: i32 = 9;
const TREE_HEIGHT: f32 = 110.0;
const CANOPY_SIZE: f32 = 330.0;
const TRUNK_SIZE: f32 = 70.0;
const LANTERN_ROWS: [f32; 2] = [-62.0, 62.0];
const LANTERN_SPACING: f32 = 140.0;
const LANTERN_SIZE: f32 = 22.0;
const LANTERN_FADE: f32 = 80.0;
const LANTERN_COLOR: Rgb<f32> = Rgb::new(1.0, 0.84, 0.68);
const PICNIC: Vec2<f32> = Vec2::new(-60.0, 360.0);
const BLANKET_SIZE: Vec2<f32> = Vec2::new(300.0, 225.0);
/// Two lanterns stand at the picnic, so the family is lit at night.
const PICNIC_LANTERNS: [Vec2<f32>; 2] = [Vec2::new(-190.0, 260.0), Vec2::new(110.0, 470.0)];
const RIVER_Y: f32 = 660.0;
const RIVER_EXTENT: Vec2<f32> = Vec2::new(2800.0, 320.0);
const RIVER_FLOW: f32 = 22.0;
const RIVER_LANTERN_Y: f32 = 530.0;
const RIVER_LANTERN_SPACING: f32 = 280.0;
const BRIDGE_X: f32 = 330.0;
const BRIDGE_SIZE: Vec2<f32> = Vec2::new(130.0, 260.0);
const BRIDGE_WALK: f32 = 24.0;
const KOI_COUNT: usize = 6;
const KOI_SIZE: f32 = 80.0;
/// A lily pad: x position, the place across the river (-1 to 1), size and rotation.
const LILIES: [(f32, f32, f32, f32); 6] = [
    (-520.0, 0.55, 40.0, 0.3),
    (-470.0, 0.7, 30.0, 2.1),
    (-120.0, -0.6, 36.0, 4.0),
    (620.0, 0.5, 42.0, 1.2),
    (680.0, 0.65, 28.0, 5.1),
    (1000.0, -0.5, 38.0, 3.3),
];
const PETAL_COUNT: usize = 280;
const PETAL_SIZE: f32 = 13.0;
const PETAL_TOP: f32 = 140.0;
const PETAL_REST: f32 = 6.0;
/// Shadow orders: the columns draw and erase first, then the crowns, then the petals.
const COLUMN_SHADOWS: f32 = 0.0;
const CROWN_SHADOWS: f32 = 1.0;
const PETAL_SHADOWS: f32 = 2.0;
const NIGHT_KEY: f32 = 0.07;
const DAY_KEY: f32 = 0.1;

/// A person at the picnic: texture, offset from the picnic center, size, and the height
/// of the shadow caster. Each one faces the middle of the blanket.
const PEOPLE: [(&str, Vec2<f32>, f32, f32); 7] = [
    ("girl", Vec2::new(-50.0, -88.0), 58.0, 14.0),
    ("boy", Vec2::new(55.0, -88.0), 64.0, 16.0),
    ("mother", Vec2::new(-80.0, 88.0), 72.0, 22.0),
    ("father", Vec2::new(55.0, 92.0), 78.0, 22.0),
    ("grandmother", Vec2::new(125.0, 8.0), 70.0, 20.0),
    ("grandfather", Vec2::new(-195.0, -4.0), 78.0, 30.0),
    ("photographer", Vec2::new(-40.0, -240.0), 82.0, 44.0),
];
const PROPS: [(&str, Vec2<f32>, f32, f32); 3] = [
    ("jubako", Vec2::new(-10.0, 4.0), 76.0, 0.1),
    ("tea-set", Vec2::new(-78.0, 22.0), 62.0, -0.3),
    ("sake", Vec2::new(48.0, 34.0), 42.0, 0.6),
];

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
                .ensure(format!("texture://sakura/{name}.png"))
                .unwrap();
        }
    }

    fn update(&mut self, context: GameContext, _: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(State::default()));
        }
    }
}

/// A petal falls from `height` to the ground, then rests until `rest` runs out.
/// A petal that leaves the view comes back on the other side.
struct Petal {
    position: Vec2<f32>,
    height: f32,
    fall: f32,
    flip: f32,
    flip_speed: f32,
    turn: f32,
    frame: usize,
    rest: f32,
    seed: f32,
}

struct State {
    lighting: Lighting,
    scene: Scene,
    nodes: Nodes,
    anime: bool,
    time: f32,
    day_time: f32,
    /// SAKURA_DAY sets the day time and stops the day clock, for screenshots.
    frozen_day: bool,
    /// SAKURA_TIME sets the animation time and stops it and the petals, for screenshots.
    frozen_time: bool,
    noise: Perlin,
    /// SAKURA_PLAYER ("x,y") sets the start position, for screenshots.
    player: Vec2<f32>,
    facing: f32,
    petals: Vec<Petal>,
    spawned: f32,
    movement: CardinalInputCombinator,
    toggle_style: InputActionRef,
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
        let frozen_day = frozen("SAKURA_DAY");
        let frozen_time = frozen("SAKURA_TIME");
        let player = std::env::var("SAKURA_PLAYER")
            .ok()
            .and_then(|value| {
                let (x, y) = value.split_once(',')?;
                Some(Vec2::new(x.parse().ok()?, y.parse().ok()?))
            })
            .unwrap_or(Vec2::new(160.0, 120.0));
        let mut scene = Scene::default();
        let nodes = Nodes::build(&mut scene, player);
        let mut result = Self {
            scene,
            nodes,
            lighting: Lighting::default()
                .with_debug_font("roboto.ttf")
                .with_style(anime_style())
                .with_uniform("u_bands", GlowUniformValue::F1(1.5))
                .with_uniform("u_band_edge", GlowUniformValue::F1(0.08))
                .with_uniform("u_shade_tint", srgb_uniform(0.62, 0.6, 0.95))
                .with_uniform("u_saturation", GlowUniformValue::F1(1.15))
                .with_uniform("u_lift", srgb_uniform(0.035, 0.02, 0.06))
                .with_uniform("u_vignette", GlowUniformValue::F1(0.35))
                .with_uniform("u_ink", srgb_uniform(0.3, 0.18, 0.3))
                .with_uniform("u_ink_width", GlowUniformValue::F1(1.5))
                .with_uniform("u_rim", GlowUniformValue::F1(0.6))
                .with_uniform("u_water_shallow", srgb_uniform(0.55, 0.85, 0.82))
                .with_uniform("u_water_deep", srgb_uniform(0.12, 0.38, 0.58))
                .with_uniform("u_water_lines", GlowUniformValue::F1(0.5))
                .with_uniform("u_water_flow", GlowUniformValue::F1(RIVER_FLOW / 60.0)),
            anime: true,
            time: frozen_time.unwrap_or_default(),
            day_time: DAY_LENGTH * frozen_day.unwrap_or(0.3),
            frozen_day: frozen_day.is_some(),
            frozen_time: frozen_time.is_some(),
            noise: Perlin::new(11),
            player,
            facing: 0.0,
            petals: Vec::with_capacity(PETAL_COUNT),
            spawned: 0.0,
            movement: CardinalInputCombinator::default(),
            toggle_style: InputActionRef::default(),
            fast_forward: InputActionRef::default(),
            run: InputActionRef::default(),
            views: Vec::new(),
        };
        result.lighting.config.texel_world = Some(LIGHT_TEXEL);
        for _ in 0..PETAL_COUNT {
            let mut petal = result.spawn_petal();
            petal.height = hash(petal.seed * 3.1) * PETAL_TOP;
            result.petals.push(petal);
        }
        result
    }
}

/// An overhead node that fades near the player.
struct Fade {
    node: NodeId,
    position: Vec2<f32>,
}

/// The nodes that change in `update`. All other nodes stay as `build` makes them.
struct Nodes {
    koi: Vec<NodeId>,
    lanterns: Vec<NodeId>,
    player: NodeId,
    river: NodeId,
    resting_petals: NodeId,
    falling_petals: NodeId,
    petal_shadows: NodeId,
    /// Each caster keeps its day opacity. The night makes all shadows weaker.
    shadows: Vec<(NodeId, f32)>,
    crowns: Vec<Fade>,
}

impl Nodes {
    /// The sibling order is the draw order of the old immediate passes.
    fn build(scene: &mut Scene, player: Vec2<f32>) -> Self {
        let root = scene.root();
        let mut shadows = Vec::new();
        let ground = scene.spawn(root, SceneNode::group());
        build_ground(scene, ground);
        let river = scene.spawn(root, SceneNode::group());
        let koi = build_river(scene, river, &mut shadows);
        let picnic = scene.spawn(root, SceneNode::group());
        build_picnic(scene, picnic, &mut shadows);
        let trunks = scene.spawn(root, SceneNode::group());
        for (position, _) in State::trees() {
            let trunk = lit(
                scene,
                trunks,
                SceneNode::sprite(sprite(
                    "trunk",
                    Vec2::zero(),
                    TRUNK_SIZE.into(),
                    position.x * 0.01,
                ))
                .position(position),
                LightMaterial::opaque(),
            );
            let shape = sprite("trunk", Vec2::zero(), (TRUNK_SIZE * 0.4).into(), 0.0);
            scene.attach(
                trunk,
                column_caster(shape, TREE_HEIGHT * 0.7, TRUNK_SIZE * 0.4),
            );
            shadows.push((trunk, 1.0));
        }
        let lanterns = scene.spawn(root, SceneNode::group());
        let lanterns = State::lanterns()
            .map(|position| {
                let lantern = lit(
                    scene,
                    lanterns,
                    SceneNode::sprite(sprite("chochin", Vec2::zero(), LANTERN_SIZE.into(), 0.0))
                        .position(position),
                    LightMaterial::opaque(),
                );
                scene.attach(lantern, Emitter::new(LANTERN_COLOR, 0.3));
                lantern
            })
            .collect();
        let player = lit(
            scene,
            root,
            SceneNode::sprite(sprite("student", Vec2::zero(), PLAYER_SIZE.into(), 0.0))
                .position(player),
            figure(),
        );
        let shape = sprite("student", Vec2::zero(), (PLAYER_SIZE * 0.7).into(), 0.0);
        scene.attach(player, column_caster(shape, 46.0, PLAYER_SIZE * 0.4));
        shadows.push((player, 0.8));

        let river_water = scene.spawn(
            root,
            SceneNode::sprite(sprite("river-depth", Vec2::zero(), RIVER_EXTENT, 0.0))
                .position(Vec2::new(0.0, RIVER_Y)),
        );
        scene.attach(river_water, Water(river_material(0.0)));

        let overhead = scene.spawn(root, SceneNode::group());
        let resting_petals = petals(scene, overhead);
        let crowns = State::trees()
            .enumerate()
            .map(|(index, (position, variant))| {
                let shape = sprite(canopy(variant), Vec2::zero(), CANOPY_SIZE.into(), 0.0);
                let node = scene.spawn(
                    overhead,
                    SceneNode::sprite(shape.clone())
                        .position(position)
                        .rotation(index as f32 * 1.3),
                );
                scene.attach(node, Forward);
                scene.attach(
                    node,
                    ShadowCaster::new(ShadowMaterial::new(TREE_HEIGHT).softness(16.0))
                        .shape(Silhouette::sprite(shape))
                        .order(CROWN_SHADOWS),
                );
                shadows.push((node, 0.55));
                Fade { node, position }
            })
            .collect();
        let falling_petals = petals(scene, overhead);
        let petal_shadows = scene.spawn(overhead, SceneNode::group());
        scene.attach(
            petal_shadows,
            ShadowCaster::new(ShadowMaterial::new(1.0).softness(1.5).vertex_height())
                .shape(Silhouette::particles(
                    ParticleEmitter::single(asset("petals", "u_image")),
                    Vec::new(),
                ))
                .order(PETAL_SHADOWS),
        );
        shadows.push((petal_shadows, 0.5));
        Self {
            koi,
            lanterns,
            player,
            river: river_water,
            resting_petals,
            falling_petals,
            petal_shadows,
            shadows,
            crowns,
        }
    }
}

/// The ground covers the widest view at the clamped camera. Each grass row is a group, so the
/// scene skips the rows outside the view.
fn build_ground(scene: &mut Scene, parent: NodeId) {
    let tiles = (GROUND_EXTENT / TILE).ceil().as_::<i32>();
    for y in -tiles.y..tiles.y {
        let row = scene.spawn(parent, SceneNode::group());
        for x in -tiles.x..tiles.x {
            scene.spawn(
                row,
                SceneNode::sprite(
                    Sprite::single(asset("grass", "u_image")).size((TILE + 0.5).into()),
                )
                .position(Vec2::new(x as f32, y as f32) * TILE),
            );
        }
    }
    let path_tiles = (GROUND_EXTENT.x / PATH_WIDTH).ceil() as i32;
    for x in -path_tiles..path_tiles {
        scene.spawn(
            parent,
            SceneNode::sprite(
                Sprite::single(asset("path", "u_image")).size((PATH_WIDTH + 0.5).into()),
            )
            .position(Vec2::new(x as f32 * PATH_WIDTH, -PATH_WIDTH * 0.5)),
        );
    }
}

fn build_river(scene: &mut Scene, parent: NodeId, shadows: &mut Vec<(NodeId, f32)>) -> Vec<NodeId> {
    lit(
        scene,
        parent,
        SceneNode::sprite(sprite("river-bed", Vec2::zero(), RIVER_EXTENT, 0.0))
            .position(Vec2::new(0.0, RIVER_Y)),
        LightMaterial::default().submerged(),
    );
    let koi = (0..KOI_COUNT)
        .map(|index| {
            let name = if index % 3 == 0 { "koi-b" } else { "koi-a" };
            lit(
                scene,
                parent,
                SceneNode::sprite(sprite(name, Vec2::zero(), KOI_SIZE.into(), 0.0)),
                LightMaterial::default().submerged(),
            )
        })
        .collect();
    for (x, across, size, rotation) in LILIES {
        lit(
            scene,
            parent,
            SceneNode::sprite(sprite("lily", Vec2::zero(), size.into(), 0.0))
                .position(Vec2::new(
                    x,
                    RIVER_Y + river_offset(x) + across * river_half_width(x),
                ))
                .rotation(rotation),
            figure(),
        );
    }
    let bridge = lit(
        scene,
        parent,
        SceneNode::sprite(sprite("bridge", Vec2::zero(), BRIDGE_SIZE, 0.0))
            .position(State::bridge()),
        figure(),
    );
    let shape = sprite("bridge", Vec2::zero(), BRIDGE_SIZE, 0.0);
    scene.attach(bridge, column_caster(shape, 14.0, BRIDGE_SIZE.x));
    shadows.push((bridge, 0.8));
    koi
}

fn build_picnic(scene: &mut Scene, parent: NodeId, shadows: &mut Vec<(NodeId, f32)>) {
    scene.spawn(
        parent,
        SceneNode::sprite(sprite("blanket", Vec2::zero(), BLANKET_SIZE, 0.0))
            .position(PICNIC)
            .rotation(0.04),
    );
    for (name, offset, size, rotation) in PROPS {
        lit(
            scene,
            parent,
            SceneNode::sprite(sprite(name, Vec2::zero(), size.into(), 0.0))
                .position(PICNIC + offset)
                .rotation(rotation),
            figure(),
        );
    }
    for (name, offset, size, height) in PEOPLE {
        let person = lit(
            scene,
            parent,
            SceneNode::sprite(sprite(name, Vec2::zero(), size.into(), 0.0))
                .position(PICNIC + offset)
                .rotation(facing(offset)),
            figure(),
        );
        let shape = sprite(name, Vec2::zero(), (size * 0.8).into(), 0.0);
        scene.attach(person, column_caster(shape, height, size * 0.5));
        shadows.push((person, 0.8));
    }
}

/// A trunk, a person or the bridge casts copies at several heights, so the shadow joins the
/// ground. The copy at height zero covers the caster itself. All column casters share one
/// shadow order, so their erasers clear every caster before the crowns and petals add shadows.
fn column_caster(shape: Sprite, height: f32, size: f32) -> ShadowCaster {
    ShadowCaster::new(ShadowMaterial::new(height).softness(2.0))
        .shape(Silhouette::sprite(shape))
        .column(0.05)
        .column_size(size)
        .erase_self()
        .order(COLUMN_SHADOWS)
}

/// All petals of one layer draw as one particle batch.
fn petals(scene: &mut Scene, parent: NodeId) -> NodeId {
    let node = scene.spawn(
        parent,
        SceneNode::particles(
            ParticleEmitter::single(asset("petals", "u_image")),
            Vec::new(),
        ),
    );
    scene.attach(node, Forward);
    node
}

fn lit(scene: &mut Scene, parent: NodeId, node: SceneNode, material: LightMaterial) -> NodeId {
    let node = scene.spawn(parent, node);
    scene.attach(node, Light(material));
    node
}

fn river_material(time: f32) -> WaterMaterial {
    WaterMaterial {
        transmittance: Rgb::new(0.3, 0.62, 0.68),
        scatter: Rgb::new(0.02, 0.06, 0.08),
        ripple_strength: 0.7,
        foam: 0.7,
        sparkles: 8.0,
        ripple_scale: RIVER_EXTENT / 60.0,
        time,
        ..Default::default()
    }
}

fn place(scene: &mut Scene, node: NodeId, position: Vec2<f32>, rotation: f32) {
    if let Some(mut node) = scene.node_mut(node) {
        node.transform.position = position;
        node.transform.rotation = rotation;
    }
}

fn set_particles(scene: &mut Scene, node: NodeId, instances: Vec<ParticleInstance>) {
    if let Some(mut node) = scene.node_mut(node)
        && let Primitive::Particles(particles) = &mut node.primitive
    {
        particles.instances = instances;
    }
}

impl State {
    fn camera(&self) -> Vec2<f32> {
        let reach = WORLD - Vec2::new(VIEW_REACH * 0.85, VIEW_HEIGHT * 0.5);
        Vec2::partial_max(Vec2::partial_min(self.player, reach), -reach)
    }

    fn night(&self) -> f32 {
        smoothstep(0.0, 0.25, -(self.day_time / DAY_LENGTH * TAU).sin())
    }

    fn wind(&self) -> Vec2<f32> {
        let gust = self.noise.get([(self.time * 0.15) as f64, 0.5]) as f32;
        Vec2::new(28.0 + gust * 30.0, 9.0 + gust * 6.0)
    }

    fn trees() -> impl Iterator<Item = (Vec2<f32>, usize)> {
        TREE_ROWS.into_iter().enumerate().flat_map(|(row, y)| {
            (0..TREE_COUNT).map(move |index| {
                let seed = (row as i32 * TREE_COUNT + index) as f32;
                let x = (index - TREE_COUNT / 2) as f32 * TREE_SPACING
                    + (row as f32 - 0.5) * TREE_SPACING * 0.5;
                let jitter = Vec2::new(hash(seed) - 0.5, hash(seed + 7.0) - 0.5) * 40.0;
                (Vec2::new(x, y) + jitter, (index as usize + row) % 2)
            })
        })
    }

    fn lanterns() -> impl Iterator<Item = Vec2<f32>> {
        let count = (WORLD.x / LANTERN_SPACING) as i32;
        LANTERN_ROWS
            .into_iter()
            .flat_map(move |y| {
                (-count..=count).map(move |index| Vec2::new(index as f32 * LANTERN_SPACING, y))
            })
            .chain(PICNIC_LANTERNS)
            .chain((-count / 2..=count / 2).map(|index| {
                Vec2::new(
                    (index as f32 + 0.5) * RIVER_LANTERN_SPACING,
                    RIVER_LANTERN_Y,
                )
            }))
    }

    fn bridge() -> Vec2<f32> {
        Vec2::new(BRIDGE_X, RIVER_Y + river_offset(BRIDGE_X))
    }

    /// Each koi swims along the river and weaves from bank to bank.
    fn koi_at(&self, index: usize, time: f32) -> Vec2<f32> {
        let seed = index as f32;
        let direction = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
        let speed = 18.0 + hash(seed) * 16.0;
        let x = wrap(seed * 470.0 + time * speed * direction, WORLD.x);
        let across = (time * (0.2 + hash(seed + 1.0) * 0.2) + seed * 2.0).sin() * 0.5;
        Vec2::new(x, RIVER_Y + river_offset(x) + across * river_half_width(x))
    }

    fn walkable(position: Vec2<f32>) -> bool {
        if position.x.abs() > WORLD.x - 20.0 || position.y.abs() > WORLD.y - 20.0 {
            return false;
        }
        if Self::trees().any(|(tree, _)| tree.distance(position) < TRUNK_SIZE * 0.5 + 12.0) {
            return false;
        }
        let across = (position.y - RIVER_Y - river_offset(position.x)).abs();
        if across < river_half_width(position.x) + 6.0
            && (position.x - BRIDGE_X).abs() > BRIDGE_WALK
        {
            return false;
        }
        PEOPLE
            .iter()
            .all(|(_, offset, size, _)| (PICNIC + *offset).distance(position) > size * 0.4 + 14.0)
    }

    fn spawn_petal(&mut self) -> Petal {
        self.spawned += 1.0;
        let seed = self.spawned;
        let camera = self.camera();
        let upwind = -self.wind().try_normalized().unwrap_or_default();
        let spread = Vec2::new(
            (hash(seed) - 0.5) * VIEW_REACH * 2.2,
            (hash(seed + 1.0) - 0.5) * VIEW_HEIGHT * 1.2,
        );
        Petal {
            position: camera + spread + upwind * 60.0,
            height: PETAL_TOP * (0.7 + hash(seed + 2.0) * 0.3),
            fall: 9.0 + hash(seed + 3.0) * 9.0,
            flip: hash(seed + 4.0) * TAU,
            flip_speed: 2.0 + hash(seed + 5.0) * 4.0,
            turn: hash(seed + 6.0) * TAU,
            frame: (hash(seed + 8.0) * 4.0) as usize % 4,
            rest: PETAL_REST * (0.5 + hash(seed + 9.0)),
            seed,
        }
    }

    fn update_petals(&mut self, delta_time: f32) {
        let wind = self.wind();
        let camera = self.camera();
        let bounds = Vec2::new(VIEW_REACH * 1.15, VIEW_HEIGHT * 0.65);
        for index in 0..self.petals.len() {
            let petal = &mut self.petals[index];
            if petal.height > 0.0 {
                let sway = self
                    .noise
                    .get([(self.time * 0.8) as f64, petal.seed as f64 * 0.37])
                    as f32;
                petal.position += (wind + Vec2::new(sway, -sway) * 25.0) * delta_time;
                petal.height = (petal.height - petal.fall * delta_time).max(0.0);
                petal.flip += petal.flip_speed * delta_time;
                petal.turn += petal.flip_speed * 0.3 * delta_time;
            } else if river_depth(petal.position) > 0.05 {
                let drift = (self.time * 0.7 + petal.seed).sin() * 4.0;
                petal.position += Vec2::new(RIVER_FLOW, drift) * delta_time;
                petal.rest -= delta_time * 0.25;
            } else {
                petal.rest -= delta_time;
            }
            let offset = petal.position - camera;
            let outside = offset.x.abs() > bounds.x || offset.y.abs() > bounds.y;
            let height = petal.height;
            if petal.rest <= 0.0 || outside {
                let mut fresh = self.spawn_petal();
                if outside && height > 0.0 {
                    let wrapped = Vec2::new(wrap(offset.x, bounds.x), wrap(offset.y, bounds.y));
                    fresh.position = camera + wrapped;
                    fresh.height = height;
                }
                self.petals[index] = fresh;
            }
        }
    }

    /// The day goes from morning to the evening, then the lanterns light the night (yozakura).
    fn update_sky(&mut self) {
        let phase = self.day_time / DAY_LENGTH * TAU;
        let daylight = phase.sin().max(0.0);
        let moonlight = (-phase.sin()).max(0.0);
        let dusk = 1.0 - (phase.sin().abs() * 3.0).min(1.0);
        let path = if daylight > 0.0 { phase } else { phase - PI };
        let night = self.night();
        let config = &mut self.lighting.config;
        config.sun_direction = Vec2::new(path.cos(), -path.sin() * 0.5 - 0.3);
        config.shadow_length = (1.0 / daylight.max(moonlight).max(0.25) - 0.5).min(2.5);
        config.sky = mix(
            Rgb::new(0.02, 0.02, 0.06),
            Rgb::new(0.27, 0.31, 0.45),
            daylight.sqrt(),
        ) + Rgb::new(0.3, 0.14, 0.1) * dusk;
        config.sun = mix(
            Rgb::new(1.0, 0.45, 0.35),
            Rgb::new(1.0, 0.93, 0.85),
            daylight,
        ) * daylight.sqrt()
            * 1.6
            + Rgb::new(0.09, 0.11, 0.27) * moonlight.sqrt();
        config.ambient = Rgb::new(0.03, 0.025, 0.05);
        if let Some(adaptation) = &mut config.eye_adaptation {
            adaptation.focus = Some(self.player);
            adaptation.key = DAY_KEY + (NIGHT_KEY - DAY_KEY) * night;
        }
    }

    fn animate(&mut self) {
        let night = self.night();
        let strength = 1.0 - night * 0.7;
        let koi = (0..KOI_COUNT)
            .map(|index| {
                let position = self.koi_at(index, self.time);
                let heading = self.koi_at(index, self.time + 0.1) - position;
                (position, heading.y.atan2(heading.x) + FRAC_PI_2)
            })
            .collect::<Vec<_>>();
        let camera = self.camera();
        let glow = Self::lanterns()
            .enumerate()
            .map(|(index, position)| {
                let flicker =
                    1.0 + self.noise.get([(self.time * 2.0) as f64, index as f64]) as f32 * 0.15;
                0.3 + 5.0 * night * flicker * view_fade(position, camera)
            })
            .collect::<Vec<_>>();
        let resting = self
            .petals
            .iter()
            .filter(|petal| petal.height <= 0.0)
            .map(|petal| {
                let alpha = (petal.rest / 1.5).clamp(0.0, 1.0);
                petal_instance(petal).tint(Rgba::new(1.0, 1.0, 1.0, alpha))
            })
            .collect();
        let falling = self
            .petals
            .iter()
            .filter(|petal| petal.height > 0.0)
            .map(petal_instance)
            .collect();
        let shadows = self
            .petals
            .iter()
            .filter(|petal| petal.height > 1.0)
            .map(|petal| petal_instance(petal).tint(Rgba::new(petal.height, 1.0, 1.0, 1.0)))
            .collect();
        let (time, player, facing) = (self.time, self.player, self.facing);
        let Self { scene, nodes, .. } = self;

        for (node, (position, rotation)) in nodes.koi.iter().zip(koi) {
            place(scene, *node, position, rotation);
        }
        for (node, intensity) in nodes.lanterns.iter().zip(glow) {
            if let Some(mut emitter) = scene.attr_mut::<Emitter>(*node) {
                emitter.intensity = intensity;
            }
        }
        place(scene, nodes.player, player, facing);
        if let Some(mut water) = scene.attr_mut::<Water>(nodes.river) {
            water.0.time = time;
        }
        set_particles(scene, nodes.resting_petals, resting);
        set_particles(scene, nodes.falling_petals, falling);
        if let Some(mut caster) = scene.attr_mut::<ShadowCaster>(nodes.petal_shadows)
            && let Silhouette::Particles(particles) = &mut caster.shape
        {
            particles.instances = shadows;
        }
        for (node, opacity) in &nodes.shadows {
            if let Some(mut caster) = scene.attr_mut::<ShadowCaster>(*node) {
                caster.material.opacity = opacity * strength;
            }
        }
        for crown in &nodes.crowns {
            let alpha = smoothstep(60.0, 170.0, crown.position.distance(player)).max(0.2);
            if let Some(mut node) = scene.node_mut(crown.node)
                && let Primitive::Sprite(sprite) = &mut node.primitive
            {
                sprite.tint = Rgba::new(1.0, 1.0, 1.0, alpha);
            }
        }
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
                VirtualAction::KeyButton(KeyCode::KeyT),
                self.toggle_style.clone(),
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
        if self.toggle_style.get().is_pressed() {
            self.anime = !self.anime;
            self.lighting.set_style(if self.anime {
                anime_style()
            } else {
                LightingStyle::default()
            });
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
        if !self.frozen_time {
            self.update_petals(delta_time);
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

/// A style uniform reaches the shader as it is, so the CPU decodes an sRGB color first.
fn srgb_uniform(r: f32, g: f32, b: f32) -> GlowUniformValue {
    GlowUniformValue::F3(srgb_to_linear(Rgb::new(r, g, b)).into_array())
}

fn anime_style() -> LightingStyle {
    LightingStyle {
        shade: Some(SHADE.into()),
        grade: Some(GRADE.into()),
        water: Some(WATER.into()),
        passes: vec![OUTLINE.into()],
    }
}

/// A person or a prop absorbs only a little light, so radiance cascades light it like the
/// ground under it and do not leave its middle dark. The height shadows give it the shade.
fn figure() -> LightMaterial {
    LightMaterial::filter(Rgb::broadcast(0.8), 200.0)
}

/// The river center moves across the world. The asset script bakes the river textures
/// with the same formulas.
fn river_offset(x: f32) -> f32 {
    25.0 * (x * 0.0025 + 0.6).sin()
}

fn river_half_width(x: f32) -> f32 {
    92.0 + 14.0 * (x * 0.004 + 1.0).sin() + 8.0 * (x * 0.011 + 2.0).sin()
}

/// Returns the relative depth: 0 on the bank, 1 in the middle of the river.
fn river_depth(position: Vec2<f32>) -> f32 {
    let across = (position.y - RIVER_Y - river_offset(position.x)) / river_half_width(position.x);
    (1.0 - across * across).max(0.0)
}

fn petal_instance(petal: &Petal) -> ParticleInstance {
    let lift = 1.0 + petal.height / PETAL_TOP * 0.6;
    let width = petal.flip.cos().abs().max(0.25);
    ParticleInstance::default()
        .region_page(Rect::new(petal.frame as f32 * 0.25, 0.0, 0.25, 1.0), 0.0)
        .position(petal.position)
        .size(Vec2::new(width, 1.0) * PETAL_SIZE * lift)
        .rotation(petal.turn)
        .pivot(0.5.into())
}

fn canopy(variant: usize) -> &'static str {
    if variant == 0 { "sakura-a" } else { "sakura-b" }
}

/// A picnic sprite faces up in its texture, so it turns to look at the middle of the blanket.
fn facing(offset: Vec2<f32>) -> f32 {
    (-offset.y).atan2(-offset.x) + FRAC_PI_2
}

fn wrap(value: f32, half: f32) -> f32 {
    (value + half).rem_euclid(half * 2.0) - half
}

fn asset(name: &str, sampler: &'static str) -> SpriteTexture {
    SpriteTexture {
        sampler: sampler.into(),
        texture: TextureRef::name(format!("sakura/{name}.png")),
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

fn mix(from: Rgb<f32>, to: Rgb<f32>, factor: f32) -> Rgb<f32> {
    from * (1.0 - factor) + to * factor
}

/// A lantern outside the view fades out at night.
/// The scene lights the whole occupancy rect, so without the fade the off-view lanterns
/// raise the night floor, the eye adaptation lowers the exposure and the halos in the view get small.
/// The fade starts at the view edge, so a lantern in the view always glows fully.
fn view_fade(position: Vec2<f32>, camera: Vec2<f32>) -> f32 {
    let offset = (position - camera).map(f32::abs);
    let outside = (offset.x - VIEW_REACH).max(offset.y - VIEW_HEIGHT * 0.5) - LANTERN_SIZE;
    1.0 - smoothstep(0.0, LANTERN_FADE, outside)
}

fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let factor = ((value - from) / (to - from)).clamp(0.0, 1.0);
    factor * factor * (3.0 - 2.0 * factor)
}

/// Returns a stable pseudo random value in `[0, 1)`, so the scene is the same on every run.
fn hash(value: f32) -> f32 {
    ((value * 12.9898).sin() * 43_758.547).rem_euclid(1.0)
}
