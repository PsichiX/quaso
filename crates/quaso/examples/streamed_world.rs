use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    lighting::{
        Lighting, LightingDebugView, LightingStats, material::LightMaterial,
        quality::LightingQuality,
    },
    scene::{Emitter, Light, NodeId, Occluder, Scene, SceneNode, SceneStats},
    third_party::{
        fontdue::layout::{HorizontalAlign, VerticalAlign},
        noise::{NoiseFn, Perlin},
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            text::Text,
            utils::{Drawable, ShaderRef, TextureRef},
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::{GlowTextureFiltering, GlowTextureFormat},
        },
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, KeyCode,
            VirtualAction,
        },
        vek::{Rgb, Rgba, Vec2},
    },
};
use std::{
    collections::HashMap,
    error::Error,
    f32::consts::{PI, TAU},
};

fn main() -> Result<(), Box<dyn Error>> {
    let instance = GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    });
    #[cfg(feature = "agent")]
    let instance = instance.with_agent_server(quaso::agent::AgentServerConfig::default());
    GameLauncher::new(instance)
        .title("Streamed world")
        .config(Config::load_from_file("./resources/GameConfig.toml")?)
        .run();
    Ok(())
}

const TEXTURES: &[&str] = &[
    "gravel",
    "gravel-normal",
    "paving",
    "paving-normal",
    "sakura",
    "chochin",
    "chochin-emission",
];
const VIEWS: [(KeyCode, LightingDebugView); 6] = [
    (KeyCode::Digit1, LightingDebugView::None),
    (KeyCode::Digit2, LightingDebugView::LightMap),
    (KeyCode::Digit3, LightingDebugView::Lighting),
    (KeyCode::Digit4, LightingDebugView::Distance),
    (KeyCode::Digit5, LightingDebugView::Shadows),
    (KeyCode::Backquote, LightingDebugView::Overview),
];
const MASK_SIZE: u32 = 64;
const DAY_LENGTH: f32 = 90.0;
const FAST_FORWARD: f32 = 8.0;
const VIEW_HEIGHT: f32 = 600.0;
const WORLD_TEXEL: f32 = 4.0;
/// Lights up to this distance outside the view still light the view.
const LIGHT_REACH: f32 = 384.0;
const CHUNK: f32 = 512.0;
const TILE: f32 = 128.0;
const CELL: f32 = 64.0;
const WALL: f32 = 16.0;
const DOOR: f32 = 96.0;
/// Chunks load when they come this close to the view, and unload one chunk farther away.
const LOAD_MARGIN: f32 = 256.0;
const UNLOAD_MARGIN: f32 = LOAD_MARGIN + CHUNK;
const CAMERA_SPEED: f32 = 220.0;
const CAMERA_RUN: f32 = 4.0;
const TOUR_DIRECTION: Vec2<f32> = Vec2::new(0.8, 0.6);
const LAMP_COLOR: Rgb<f32> = Rgb::new(1.0, 0.77, 0.54);
const FIRE_COLOR: Rgb<f32> = Rgb::new(1.0, 0.67, 0.35);
const LANTERN_COLOR: Rgb<f32> = Rgb::new(0.9, 0.88, 0.81);
const LANTERN_SIZE: f32 = 14.0;
const NIGHT_KEY: f32 = 0.28;
const DAY_KEY: f32 = 0.22;
/// Linear lamp light spreads wider than gamma light, so a lower exposure cap keeps the night dark.
const MAX_EXPOSURE: f32 = 1.6;

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

/// The content of one chunk. The chunk coordinate alone decides the content,
/// so a chunk that unloads and loads again looks the same.
struct Chunk {
    floors: Vec<(Vec2<f32>, Vec2<f32>)>,
    walls: Vec<(Vec2<f32>, Vec2<f32>)>,
    trees: Vec<(Vec2<f32>, f32, f32)>,
    lamps: Vec<Vec2<f32>>,
    fires: Vec<(Vec2<f32>, f32)>,
}

impl Chunk {
    fn generate(coord: Vec2<i32>) -> Self {
        let mut random = Random::new(coord);
        let origin = coord.as_::<f32>() * CHUNK;
        let mut chunk = Self {
            floors: Vec::new(),
            walls: Vec::new(),
            trees: Vec::new(),
            lamps: Vec::new(),
            fires: Vec::new(),
        };
        if random.next() < 0.45 {
            chunk.generate_room(&mut random, origin);
        } else {
            chunk.generate_forest(&mut random, origin);
        }
        chunk
    }

    fn generate_room(&mut self, random: &mut Random, origin: Vec2<f32>) {
        let cells = Vec2::new(random.range(4, 7), random.range(3, 6)).as_::<f32>();
        let size = cells * CELL;
        let free = Vec2::broadcast(CHUNK - 2.0 * CELL) - size;
        let min = origin + Vec2::broadcast(CELL) + (free / CELL * random.next()).floor() * CELL;
        let max = min + size;
        self.floors.push((min, max));
        let door_side = random.range(0, 4);
        let sides = [
            (Vec2::new(min.x, max.y - WALL), Vec2::new(max.x, max.y)),
            (Vec2::new(min.x, min.y), Vec2::new(max.x, min.y + WALL)),
            (Vec2::new(min.x, min.y), Vec2::new(min.x + WALL, max.y)),
            (Vec2::new(max.x - WALL, min.y), Vec2::new(max.x, max.y)),
        ];
        for (index, (from, to)) in sides.into_iter().enumerate() {
            if index as u32 != door_side {
                self.walls.push((from, to));
                continue;
            }
            let center = (from + to) * 0.5;
            let half_door = if to.x - from.x > to.y - from.y {
                Vec2::new(DOOR * 0.5, 0.0)
            } else {
                Vec2::new(0.0, DOOR * 0.5)
            };
            self.walls
                .push((from, Vec2::partial_max(center - half_door, from + WALL)));
            self.walls
                .push((Vec2::partial_min(center + half_door, to - WALL), to));
        }
        self.lamps.push((min + max) * 0.5);
        if random.next() < 0.5 {
            self.fires.push((
                origin + Vec2::new(random.next(), 0.0) * (CHUNK - CELL) + CELL * 0.5,
                random.next() * 10.0,
            ));
        }
    }

    fn generate_forest(&mut self, random: &mut Random, origin: Vec2<f32>) {
        for _ in 0..random.range(3, 8) {
            let position = origin + Vec2::new(random.next(), random.next()) * CHUNK;
            self.trees
                .push((position, 18.0 + random.next() * 14.0, random.next() * TAU));
        }
        if random.next() < 0.5 {
            self.fires.push((
                origin + Vec2::new(random.next(), random.next()) * CHUNK,
                random.next() * 10.0,
            ));
        }
        if random.next() < 0.6 {
            self.lamps
                .push(origin + Vec2::new(random.next(), random.next()) * CHUNK);
        }
    }
}

/// A small PCG style generator, seeded by the chunk coordinate.
struct Random(u64);

impl Random {
    fn new(coord: Vec2<i32>) -> Self {
        let seed = (coord.x as u32 as u64) << 32 | coord.y as u32 as u64;
        let mut result = Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03);
        result.step();
        result
    }

    fn step(&mut self) -> u32 {
        let state = self.0;
        self.0 = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let xorshifted = (((state >> 18) ^ state) >> 27) as u32;
        xorshifted.rotate_right((state >> 59) as u32)
    }

    fn next(&mut self) -> f32 {
        (self.step() >> 8) as f32 / (1 << 24) as f32
    }

    fn range(&mut self, from: u32, to: u32) -> u32 {
        from + self.step() % (to - from)
    }
}

#[derive(Default)]
struct Streaming {
    travelled: f32,
    loads: u32,
    unloads: u32,
    log: bool,
}

/// The scene nodes of one loaded chunk. Each layer has its own group, so the canopies of all
/// chunks draw over the bodies of all chunks.
struct LoadedChunk {
    groups: [NodeId; 3],
    lamps: Vec<(NodeId, f32)>,
    fires: Vec<(NodeId, f32)>,
}

struct Layers {
    ground: NodeId,
    bodies: NodeId,
    lantern: NodeId,
    canopies: NodeId,
}

impl Layers {
    fn new(scene: &mut Scene) -> Self {
        let root = scene.root();
        let ground = scene.spawn(root, SceneNode::group());
        let bodies = scene.spawn(root, SceneNode::group());
        let lantern = scene.spawn(
            root,
            SceneNode::sprite(disc(Vec2::zero(), LANTERN_SIZE, Rgba::white())),
        );
        scene.attach(lantern, Light(LightMaterial::opaque()));
        scene.attach(lantern, Emitter::new(LANTERN_COLOR, 3.0));
        let canopies = scene.spawn(root, SceneNode::group());
        Self {
            ground,
            bodies,
            lantern,
            canopies,
        }
    }
}

struct State {
    lighting: Lighting,
    scene: Scene,
    layers: Layers,
    chunks: HashMap<Vec2<i32>, LoadedChunk>,
    streaming: Streaming,
    time: f32,
    day_time: f32,
    /// STREAMED_WORLD_DAY sets the day time and stops the clock, for screenshots and measures.
    frozen_day: bool,
    noise: Perlin,
    camera: Vec2<f32>,
    view_size: Vec2<f32>,
    lamp: bool,
    tour: bool,
    frame: u64,
    movement: CardinalInputCombinator,
    toggle_lamp: InputActionRef,
    toggle_tour: InputActionRef,
    fast_forward: InputActionRef,
    run: InputActionRef,
    views: Vec<(InputActionRef, LightingDebugView)>,
}

impl Default for State {
    fn default() -> Self {
        let mut lighting = Lighting::default().with_debug_font("roboto.ttf");
        lighting.config.reach = LIGHT_REACH;
        lighting.config.texel_world = Some(WORLD_TEXEL * 2.0);
        if let Some(quality) = std::env::var("STREAMED_WORLD_QUALITY")
            .ok()
            .and_then(|name| {
                LightingQuality::ALL
                    .into_iter()
                    .find(|quality| quality.label() == name)
            })
        {
            lighting.config.apply_quality(quality);
        }
        let frozen_day = std::env::var("STREAMED_WORLD_DAY")
            .ok()
            .and_then(|value| value.parse::<f32>().ok());
        let mut scene = Scene::default();
        let layers = Layers::new(&mut scene);
        Self {
            lighting,
            scene,
            layers,
            chunks: HashMap::new(),
            streaming: Streaming {
                log: std::env::var_os("STREAMED_WORLD_LOG").is_some(),
                ..Default::default()
            },
            time: 0.0,
            day_time: DAY_LENGTH * frozen_day.unwrap_or(0.7),
            frozen_day: frozen_day.is_some(),
            noise: Perlin::new(5),
            camera: Vec2::new(CHUNK * 0.5, CHUNK * 0.5),
            view_size: Vec2::new(VIEW_HEIGHT * 16.0 / 9.0, VIEW_HEIGHT),
            lamp: true,
            tour: false,
            frame: 0,
            movement: CardinalInputCombinator::default(),
            toggle_lamp: InputActionRef::default(),
            toggle_tour: InputActionRef::default(),
            fast_forward: InputActionRef::default(),
            run: InputActionRef::default(),
            views: Vec::new(),
        }
    }
}

impl State {
    fn night(&self) -> f32 {
        smoothstep(0.0, 0.25, -(self.day_time / DAY_LENGTH * TAU).sin())
    }

    fn flicker(&self, speed: f32, seed: f32) -> f32 {
        self.noise.get([(self.time * speed) as f64, seed as f64]) as f32
    }

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
            adaptation.focus = Some(self.camera);
            adaptation.key = DAY_KEY + (NIGHT_KEY - DAY_KEY) * night;
            adaptation.max_exposure = MAX_EXPOSURE;
        }
    }

    fn chunk_range(&self, margin: f32) -> (Vec2<i32>, Vec2<i32>) {
        let half = self.view_size * 0.5 + margin;
        let first = ((self.camera - half) / CHUNK).floor().as_::<i32>();
        let last = ((self.camera + half) / CHUNK).floor().as_::<i32>();
        (first, last)
    }

    /// The scene marks the occupancy changes of loaded and unloaded chunks by itself.
    fn stream_chunks(&mut self) {
        let (keep_first, keep_last) = self.chunk_range(UNLOAD_MARGIN);
        let unloaded = self
            .chunks
            .keys()
            .filter(|coord| {
                coord.x < keep_first.x
                    || coord.y < keep_first.y
                    || coord.x > keep_last.x
                    || coord.y > keep_last.y
            })
            .copied()
            .collect::<Vec<_>>();
        for coord in unloaded {
            if let Some(chunk) = self.chunks.remove(&coord) {
                for group in chunk.groups {
                    self.scene.remove(group);
                }
            }
            self.streaming.unloads += 1;
        }
        let (first, last) = self.chunk_range(LOAD_MARGIN);
        for y in first.y..=last.y {
            for x in first.x..=last.x {
                let coord = Vec2::new(x, y);
                if !self.chunks.contains_key(&coord) {
                    let chunk = self.spawn_chunk(coord, &Chunk::generate(coord));
                    self.chunks.insert(coord, chunk);
                    self.streaming.loads += 1;
                }
            }
        }
    }

    fn spawn_chunk(&mut self, coord: Vec2<i32>, chunk: &Chunk) -> LoadedChunk {
        let scene = &mut self.scene;
        let ground = scene.spawn(self.layers.ground, SceneNode::group());
        let bodies = scene.spawn(self.layers.bodies, SceneNode::group());
        let canopies = scene.spawn(self.layers.canopies, SceneNode::group());
        let origin = coord.as_::<f32>() * CHUNK;
        let tiles = (CHUNK / TILE) as i32;
        for y in 0..tiles {
            for x in 0..tiles {
                let tile = scene.spawn(
                    ground,
                    SceneNode::sprite(
                        Sprite::single(asset("gravel", "u_image"))
                            .size(TILE.into())
                            .tint(Rgba::new(0.6, 0.62, 0.55, 1.0)),
                    )
                    .position(origin + Vec2::new(x as f32, y as f32) * TILE),
                );
                scene.attach(
                    tile,
                    Light(
                        LightMaterial::default().normal_map(asset("gravel-normal", "u_normal_map")),
                    ),
                );
            }
        }
        for (min, max) in &chunk.floors {
            let floor = scene.spawn(
                ground,
                SceneNode::sprite(Sprite::single(asset("paving", "u_image")).size(*max - *min))
                    .position(*min),
            );
            scene.attach(
                floor,
                Light(LightMaterial::default().normal_map(asset("paving-normal", "u_normal_map"))),
            );
        }
        for (min, max) in &chunk.walls {
            let wall = scene.spawn(
                bodies,
                SceneNode::sprite(span(
                    Vec2::zero(),
                    *max - *min,
                    Rgba::new(0.35, 0.33, 0.3, 1.0),
                ))
                .position(*min),
            );
            scene.attach(wall, Occluder::default());
        }
        for (position, radius, rotation) in &chunk.trees {
            let trunk = scene.spawn(
                bodies,
                SceneNode::sprite(disc(
                    Vec2::zero(),
                    *radius * 2.0,
                    Rgba::new(0.25, 0.17, 0.1, 1.0),
                ))
                .position(*position),
            );
            scene.attach(trunk, Occluder::default());
            let canopy = scene.spawn(
                canopies,
                SceneNode::sprite(
                    Sprite::single(asset("sakura", "u_image"))
                        .size((*radius * 6.0).into())
                        .pivot(0.5.into())
                        .tint(Rgba::new(0.8, 1.1, 0.7, 0.9)),
                )
                .position(*position)
                .rotation(*rotation),
            );
            scene.attach(canopy, Light(LightMaterial::default().overhead()));
        }
        let lamps = chunk
            .lamps
            .iter()
            .enumerate()
            .map(|(index, position)| {
                let lamp = scene.spawn(
                    bodies,
                    SceneNode::sprite(
                        Sprite::single(asset("chochin", "u_image"))
                            .size(22.0.into())
                            .pivot(0.5.into()),
                    )
                    .position(*position),
                );
                scene.attach(
                    lamp,
                    Light(LightMaterial::filter(Rgb::new(1.0, 0.7, 0.58), 6.0)),
                );
                scene.attach(
                    lamp,
                    Emitter::new(LAMP_COLOR, 4.0).map(asset("chochin-emission", "u_emission_map")),
                );
                (lamp, position.x + index as f32)
            })
            .collect();
        let fires = chunk
            .fires
            .iter()
            .map(|(position, seed)| {
                let fire = scene.spawn(
                    bodies,
                    SceneNode::sprite(disc(Vec2::zero(), 18.0, Rgba::new(1.0, 0.6, 0.3, 1.0)))
                        .position(*position),
                );
                scene.attach(fire, Light(LightMaterial::opaque()));
                scene.attach(fire, Emitter::new(FIRE_COLOR, 6.0));
                (fire, *seed)
            })
            .collect();
        LoadedChunk {
            groups: [ground, bodies, canopies],
            lamps,
            fires,
        }
    }

    /// Flicker changes only the emitter intensity, so it rebuilds no light field.
    fn update_scene(&mut self) {
        let lamps = self.chunks.values().flat_map(|chunk| {
            chunk
                .lamps
                .iter()
                .map(|(node, seed)| (*node, 4.0 * (1.0 + self.flicker(2.0, *seed) * 0.2)))
        });
        let fires = self.chunks.values().flat_map(|chunk| {
            chunk
                .fires
                .iter()
                .map(|(node, seed)| (*node, 6.0 * (1.0 + self.flicker(6.0, *seed) * 0.5)))
        });
        let intensities = lamps.chain(fires).collect::<Vec<_>>();
        for (node, intensity) in intensities {
            if let Some(mut emitter) = self.scene.attr_mut::<Emitter>(node) {
                emitter.intensity = intensity;
            }
        }
        if let Some(mut lantern) = self.scene.node_mut(self.layers.lantern) {
            lantern.visible = self.lamp;
            lantern.transform.position = self.camera;
        }
    }

    fn draw_stats(&self, context: &mut GameContext) {
        let stats = self.lighting.stats();
        Text::new(ShaderRef::name("text"))
            .text(stats_line(
                &stats,
                &self.scene.stats(),
                self.chunks.len(),
                &self.streaming,
            ))
            .font("roboto.ttf")
            .size(14.0)
            .position(self.camera - self.view_size * 0.5 + Vec2::new(8.0, 40.0))
            .tint(Rgba::new(1.0, 1.0, 1.0, 0.8))
            .horizontal_align(HorizontalAlign::Left)
            .vertical_align(VerticalAlign::Top)
            .draw(context.draw, context.graphics);
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
                VirtualAction::KeyButton(KeyCode::KeyT),
                self.toggle_tour.clone(),
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

        if let Ok(texture) = context.graphics.texture(
            MASK_SIZE,
            MASK_SIZE,
            1,
            GlowTextureFormat::Rgba,
            Some(&disc_mask()),
        ) {
            context.draw.textures.insert("disc-mask".into(), texture);
        }
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn update(&mut self, context: GameContext, delta_time: f32) {
        self.time += delta_time;
        self.frame += 1;
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
        if self.toggle_tour.get().is_pressed() {
            self.tour = !self.tour;
        }
        for (action, view) in &self.views {
            if action.get().is_pressed() {
                self.lighting.debug_view = *view;
            }
        }
        let movement = if self.tour {
            TOUR_DIRECTION
        } else {
            Vec2::<f32>::from(self.movement.get())
                .try_normalized()
                .unwrap_or_default()
        };
        let run = if self.run.get().is_down() || self.tour {
            CAMERA_RUN
        } else {
            1.0
        };
        let step = movement * CAMERA_SPEED * run * delta_time;
        self.camera += step;
        self.streaming.travelled += step.magnitude() / CHUNK;
        let screen = context.graphics.state.main_camera.screen_size;
        if screen.y > 0.0 {
            self.view_size = Vec2::new(VIEW_HEIGHT * screen.x / screen.y, VIEW_HEIGHT);
        }
        if self.streaming.log {
            println!(
                "frame {} | {}",
                self.frame,
                stats_line(
                    &self.lighting.stats(),
                    &self.scene.stats(),
                    self.chunks.len(),
                    &self.streaming
                )
                .replace('\n', " | ")
            );
        }
        self.stream_chunks();
        self.update_scene();
        self.update_sky();
        context.graphics.state.main_camera.transform.position = self.camera.into();
    }

    fn draw(&mut self, mut context: GameContext) {
        self.scene
            .render(&mut self.lighting, context.draw, context.graphics);
        self.draw_stats(&mut context);
    }
}

fn stats_line(
    stats: &LightingStats,
    scene: &SceneStats,
    chunks: usize,
    streaming: &Streaming,
) -> String {
    let field = stats.field_update;
    format!(
        "passes {} | {:.2} Mpx\nfields {} areas {:.1}%{}\nchunks {} (+{} -{}) | moved {:.0}\nscene {} nodes{} | update {:.2} ms | render {:.2} ms | marks {}",
        stats.passes,
        stats.pass_pixels as f32 / 1.0e6,
        field.areas,
        field.texels as f32 / field.field_texels.max(1) as f32 * 100.0,
        if field.shifted { " shift" } else { "" },
        chunks,
        streaming.loads,
        streaming.unloads,
        streaming.travelled,
        scene.nodes,
        if scene.rebuilt { " rebuilt" } else { "" },
        scene.update_ms,
        scene.render_ms,
        scene.marks,
    )
}

fn asset(name: &str, sampler: &'static str) -> SpriteTexture {
    SpriteTexture {
        sampler: sampler.into(),
        texture: TextureRef::name(format!("lighting/{name}.png")),
        filtering: GlowTextureFiltering::Linear,
    }
}

fn disc(position: Vec2<f32>, size: f32, color: Rgba<f32>) -> Sprite {
    Sprite::single(SpriteTexture {
        sampler: "u_image".into(),
        texture: TextureRef::name("disc-mask"),
        filtering: GlowTextureFiltering::Linear,
    })
    .position(position)
    .size(size.into())
    .pivot(0.5.into())
    .tint(color)
}

fn span(from: Vec2<f32>, to: Vec2<f32>, color: Rgba<f32>) -> Sprite {
    Sprite::default().position(from).size(to - from).tint(color)
}

fn mix(from: Rgb<f32>, to: Rgb<f32>, factor: f32) -> Rgb<f32> {
    from * (1.0 - factor) + to * factor
}

fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let factor = ((value - from) / (to - from)).clamp(0.0, 1.0);
    factor * factor * (3.0 - 2.0 * factor)
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
