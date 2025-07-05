use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState},
    grid_world::{GridWorld, GridWorldLayer},
    third_party::{
        noise::{Fbm, MultiFractal, NoiseFn, SuperSimplex},
        randscape::{Grid, NoiseGenerator, RemapGenerator, SubGenerator},
        raui_core::widget::{
            component::text_box::TextBoxProps,
            unit::text::{TextBoxFont, TextBoxHorizontalAlign},
        },
        raui_immediate_widgets::core::text_box,
        spitfire_draw::{
            tiles::{TileInstance, TileMap, TileSet, TileSetItem, TilesEmitter},
            utils::{Drawable, ShaderRef},
        },
        spitfire_glow::graphics::{CameraScaling, Shader},
        spitfire_input::{ArrayInputCombinator, InputAxisRef, InputMapping, VirtualAxis},
        vek::{Rgba, Vec2},
    },
};
use std::{
    array::from_fn,
    error::Error,
    ops::{Add, Div, Mul, RangeInclusive, Sub},
};

const SIZE: usize = 50;
const WATER: usize = 0;
const FOREST: usize = 1;
const GRASS: usize = 2;
const SAND: usize = 3;
const ROCK: usize = 4;
const SNOW: usize = 5;
const WIND: Vec2<f64> = Vec2 { x: 1.0, y: 0.5 };

const CLEAR_SKY: usize = 0;
const CLOUD_SKY: usize = 1;
const RAINY_SKY: usize = 2;

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(
        GameInstance::new(State::default())
            .with_unfocused_fixed_time_step_scale(15.0)
            .setup_assets(|assets| {
                *assets = make_directory_database("./resources/").unwrap();
            }),
    )
    .title("Procedural Content Generator - Island")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

struct State {
    world: GridWorld,
    weather_tileset: TileSet,
    weather_noise: Fbm<SuperSimplex>,
    time: f64,
    mouse_position: ArrayInputCombinator<2>,
}

impl Default for State {
    fn default() -> Self {
        let mut height = Grid::<f64>::generate(
            SIZE.into(),
            NoiseGenerator::new(Fbm::<SuperSimplex>::default().set_frequency(0.025)),
        );
        height.apply_all(RemapGenerator {
            from: -1.0..1.0,
            to: 0.0..1.0,
        });

        let gradient = Grid::<f64>::generate(
            SIZE.into(),
            |location: Vec2<usize>, size: Vec2<usize>, _| {
                let center = size / 2;
                let x = location.x.abs_diff(center.x) as f64;
                let y = location.y.abs_diff(center.y) as f64;
                let result = (x / center.x as f64).max(y / center.y as f64);
                result * result
            },
        );
        height.apply_all(SubGenerator { other: &gradient });

        let mut biome = Grid::<f64>::generate(
            SIZE.into(),
            NoiseGenerator::new(Fbm::<SuperSimplex>::new(42).set_frequency(0.05)),
        );
        biome.apply_all(RemapGenerator {
            from: -1.0..1.0,
            to: 0.0..1.0,
        });

        let buffer = height
            .into_inner()
            .1
            .into_iter()
            .zip(biome.into_inner().1)
            .map(|(height, biome)| {
                if height > 0.75 {
                    SNOW
                } else if height > 0.6 {
                    ROCK
                } else if height > 0.1 {
                    if biome > 0.8 {
                        SAND
                    } else if biome > 0.5 {
                        GRASS
                    } else {
                        FOREST
                    }
                } else {
                    WATER
                }
            })
            .collect();

        Self {
            world: GridWorld::new(
                10.0.into(),
                TileSet::default()
                    .shader(ShaderRef::name("color"))
                    .mapping(WATER, TileSetItem::default().tint(Rgba::blue()))
                    .mapping(
                        FOREST,
                        TileSetItem::default().tint(Rgba::new_opaque(0.0, 0.5, 0.0)),
                    )
                    .mapping(GRASS, TileSetItem::default().tint(Rgba::green()))
                    .mapping(
                        SAND,
                        TileSetItem::default().tint(Rgba::new_opaque(1.0, 1.0, 0.5)),
                    )
                    .mapping(ROCK, TileSetItem::default().tint(Rgba::gray(0.5)))
                    .mapping(SNOW, TileSetItem::default().tint(Rgba::white())),
                GridWorldLayer::new(TileMap::with_buffer(SIZE.into(), buffer).unwrap()),
            ),
            weather_tileset: TileSet::default()
                .shader(ShaderRef::name("color"))
                .mapping(
                    CLEAR_SKY,
                    TileSetItem::default().tint(Rgba::new(1.0, 1.0, 1.0, 0.0)),
                )
                .mapping(
                    CLOUD_SKY,
                    TileSetItem::default().tint(Rgba::new(1.0, 1.0, 1.0, 0.8)),
                )
                .mapping(
                    RAINY_SKY,
                    TileSetItem::default().tint(Rgba::new(0.3, 0.3, 0.3, 0.8)),
                ),
            weather_noise: Fbm::<SuperSimplex>::default().set_frequency(0.03),
            time: 0.0,
            mouse_position: Default::default(),
        }
    }
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        context.graphics.main_camera.scaling = CameraScaling::FitVertical(SIZE as f32 * 10.0);

        context
            .assets
            .spawn(
                "shader://color",
                (ShaderAsset::new(
                    Shader::COLORED_VERTEX_2D,
                    Shader::PASS_FRAGMENT,
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

        let mouse_x = InputAxisRef::default();
        let mouse_y = InputAxisRef::default();
        self.mouse_position = ArrayInputCombinator::new([mouse_x.clone(), mouse_y.clone()]);
        context.input.push_mapping(
            InputMapping::default()
                .axis(VirtualAxis::MousePositionX, mouse_x)
                .axis(VirtualAxis::MousePositionY, mouse_y),
        );
    }

    fn fixed_update(&mut self, _: GameContext, delta_time: f32) {
        self.time += delta_time as f64;
    }

    fn draw(&mut self, context: GameContext) {
        self.world.draw(context.draw, context.graphics);

        TilesEmitter::default()
            .tile_size(10.0.into())
            .emit(
                &self.weather_tileset,
                (0..SIZE)
                    .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
                    .map(|(x, y)| TileInstance {
                        id: self.weather(x, y, self.time),
                        location: Vec2 { x, y },
                    }),
            )
            .draw(context.draw, context.graphics);
    }

    fn draw_gui(&mut self, context: GameContext) {
        let size = context
            .graphics
            .main_camera
            .scaling
            .world_size(context.graphics.main_camera.screen_size);
        let [x, y] = self.mouse_position.get();
        let x = remap(
            x,
            0.0..=context.graphics.main_camera.screen_size.x,
            0.0..=size.x,
        );
        let y = remap(
            y,
            0.0..=context.graphics.main_camera.screen_size.y,
            0.0..=size.y,
        );
        let x = ((x / 10.0) as usize).min(SIZE.saturating_sub(1));
        let y = ((y / 10.0) as usize).min(SIZE.saturating_sub(1));

        let forecast = from_fn::<&str, 3, _>(|index| {
            match self.weather(x, y, self.time + index as f64 * 5.0) {
                0 => "Clear sky",
                1 => "Clouds",
                2 => "Rain",
                _ => "<unknown>",
            }
        });

        text_box(TextBoxProps {
            text: format!(
                "Tile: {} x {}\nTime: {:.2}\nForecast:\n+0s: {}\n+5s: {}\n+10s: {}",
                x, y, self.time, forecast[0], forecast[1], forecast[2]
            ),
            horizontal_align: TextBoxHorizontalAlign::Right,
            font: TextBoxFont {
                name: "roboto.ttf".to_owned(),
                size: 32.0,
            },
            ..Default::default()
        });
    }
}

impl State {
    fn weather(&self, x: usize, y: usize, time: f64) -> usize {
        let x = x as f64 + WIND.x * time;
        let y = y as f64 + WIND.y * time;
        let sample = self.weather_noise.get([x, y, time]);
        let sample = remap(sample, -0.5..=1.0, 0.0..=3.0);
        (sample as usize).clamp(0, 2)
    }
}

fn remap<T: Copy + Sub<Output = T> + Div<Output = T> + Add<Output = T> + Mul<Output = T>>(
    value: T,
    from: RangeInclusive<T>,
    to: RangeInclusive<T>,
) -> T {
    let from_start = *from.start();
    let from_end = *from.end();
    let to_start = *to.start();
    let to_end = *to.end();
    let factor = (value - from_start) / (from_end - from_start);
    (to_end - to_start) * factor + to_start
}
