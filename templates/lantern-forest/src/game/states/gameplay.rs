use crate::game::{
    forest::{Forest, LANTERN_COUNT, PLAYER_START, map, smoothstep},
    kitsune::Kitsune,
};
use quaso::{
    context::GameContext,
    game::{GameState, GameStateChange},
    lighting::{
        Lighting, LightingDebugView,
        material::{LightMaterial, Lit},
    },
    third_party::{
        raui_core::{
            layout::CoordsMappingScaling,
            widget::{
                component::text_box::TextBoxProps,
                unit::text::{TextBoxFont, TextBoxHorizontalAlign, TextBoxVerticalAlign},
                utils::Color,
            },
        },
        raui_immediate_widgets::core::text_box,
        spitfire_draw::{sprite::Sprite, utils::Drawable},
        spitfire_glow::graphics::CameraScaling,
        spitfire_input::KeyCode,
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        vek::{Rgb, Rgba, Vec2},
    },
};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const SEED: u64 = 7;
const PLAYER_SPEED: f32 = 110.0;
const PLAYER_RADIUS: f32 = 16.0;
const PLAYER_SIZE: f32 = 62.0;
const TURN_SPEED: f32 = 10.0;
/// The lantern hangs from the right hand, in front of the player and to the right.
const LANTERN_OFFSET: Vec2<f32> = Vec2::new(20.5, -21.5);
/// The lantern keeps out of solid objects, or its light stays inside them.
const LANTERN_RADIUS: f32 = 6.0;
const LANTERN_COLOR: Rgb<f32> = Rgb::new(1.0, 0.6, 0.25);
const DAWN_DURATION: f32 = 25.0;
const KITSUNE_START: Vec2<f32> = Vec2::new(0.0, -700.0);

const NIGHT_SKY: Rgb<f32> = Rgb::new(0.01, 0.015, 0.035);
const DAY_SKY: Rgb<f32> = Rgb::new(0.3, 0.38, 0.5);
const MOON: Rgb<f32> = Rgb::new(0.05, 0.07, 0.12);
const SUNRISE: Rgb<f32> = Rgb::new(1.4, 0.55, 0.2);
const MORNING: Rgb<f32> = Rgb::new(1.5, 1.35, 1.1);
const MOON_DIRECTION: Vec2<f32> = Vec2::new(-0.5, -0.85);
const SUN_DIRECTION: Vec2<f32> = Vec2::new(1.0, 0.3);
/// Eye adaptation keys. The day key keeps the sunlit forest from washing out.
const NIGHT_KEY: f32 = 0.4;
const DAY_KEY: f32 = 0.22;
/// The number keys 1 to 6 select these render views.
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

pub struct Gameplay {
    lighting: Lighting,
    forest: Forest,
    time: f32,
    dawn: f32,
    caught: f32,
    kitsune: Kitsune,
    player: Vec2<f32>,
    facing: f32,
    movement: CardinalInputCombinator,
    restart: InputActionRef,
    exit: InputActionRef,
    views: [InputActionRef; VIEWS.len()],
}

impl Default for Gameplay {
    fn default() -> Self {
        Self {
            lighting: Lighting::default().with_debug_font("roboto"),
            forest: Forest::generate(SEED),
            time: 0.0,
            dawn: 0.0,
            caught: 0.0,
            kitsune: Kitsune::new(KITSUNE_START),
            player: PLAYER_START,
            facing: 0.0,
            movement: Default::default(),
            restart: Default::default(),
            exit: Default::default(),
            views: Default::default(),
        }
    }
}

impl Gameplay {
    fn lantern_position(&self) -> Vec2<f32> {
        self.player + rotate(LANTERN_OFFSET, self.facing)
    }

    fn update_player(&mut self, delta_time: f32) {
        if self.caught > 0.0 {
            return;
        }
        let movement = Vec2::<f32>::from(self.movement.get())
            .try_normalized()
            .unwrap_or_default();
        if movement != Vec2::zero() {
            let target = movement.y.atan2(movement.x) + FRAC_PI_2;
            let turn = (target - self.facing + PI).rem_euclid(TAU) - PI;
            self.facing += turn * (TURN_SPEED * delta_time).min(1.0);
        }
        self.player = self.forest.collide(
            self.player + movement * PLAYER_SPEED * delta_time,
            PLAYER_RADIUS,
        );
        let lantern = self.lantern_position();
        self.player += self.forest.collide(lantern, LANTERN_RADIUS) - lantern;
        self.player = self.forest.collide(self.player, PLAYER_RADIUS);
    }

    fn update_sky(&mut self) {
        let dawn = smoothstep(0.0, 1.0, self.dawn);
        let config = &mut self.lighting.config;
        config.sky = mix(NIGHT_SKY, DAY_SKY, dawn);
        config.sun = if dawn < 0.5 {
            mix(MOON, SUNRISE, dawn * 2.0)
        } else {
            mix(SUNRISE, MORNING, dawn * 2.0 - 1.0)
        };
        config.sun_direction =
            Vec2::lerp(MOON_DIRECTION, SUN_DIRECTION.normalized(), dawn).normalized();
        config.shadow_length = 1.2 - dawn * 0.6;
        if let Some(adaptation) = &mut config.eye_adaptation {
            adaptation.focus = Some(self.player);
            adaptation.key = NIGHT_KEY + (DAY_KEY - NIGHT_KEY) * dawn;
        }
    }

    fn draw_player(&self, context: &mut GameContext) {
        Lit::new(
            crate::game::forest::sprite("textures/player", self.player, PLAYER_SIZE, self.facing),
            LightMaterial::opaque()
                .normal_map(map("textures/player-normal", "u_normal_map"))
                .absorption_map(map("textures/player-absorption", "u_absorption_map")),
        )
        .draw(context.draw, context.graphics);

        let dread = self.kitsune.dread(self.player);
        let flicker =
            1.0 + self.forest.flicker(self.time * (1.0 + dread * 3.0), -1.0) * (0.2 + dread);
        let light = (1.0 - self.caught).max(0.0) * flicker.max(0.0);
        Lit::new(
            Sprite::default()
                .position(self.lantern_position())
                .size(5.0.into())
                .pivot(0.5.into())
                .tint(Rgba::new(1.0, 0.8, 0.5, 1.0)),
            LightMaterial::emitter(LANTERN_COLOR, 40.0 * light),
        )
        .draw(context.draw, context.graphics);
    }
}

impl GameState for Gameplay {
    fn enter(&mut self, context: GameContext) {
        context.graphics.state.color = [0.0, 0.0, 0.0, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(500.0);
        context.gui.coords_map_scaling = CoordsMappingScaling::FitVertical(500.0);
        for (name, texture) in &context.draw.textures {
            if name.starts_with("textures/") {
                texture.generate_mipmaps();
            }
        }

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
        let mut mapping = InputMapping::default();
        for ((key, _), action) in VIEWS.iter().zip(&self.views) {
            mapping = mapping.action(VirtualAction::KeyButton(*key), action.clone());
        }
        context.input.push_mapping(
            mapping
                .consume(InputConsume::Hit)
                .action(VirtualAction::KeyButton(KeyCode::KeyA), move_left.clone())
                .action(VirtualAction::KeyButton(KeyCode::KeyD), move_right.clone())
                .action(VirtualAction::KeyButton(KeyCode::KeyW), move_up.clone())
                .action(VirtualAction::KeyButton(KeyCode::KeyS), move_down.clone())
                .action(VirtualAction::KeyButton(KeyCode::ArrowLeft), move_left)
                .action(VirtualAction::KeyButton(KeyCode::ArrowRight), move_right)
                .action(VirtualAction::KeyButton(KeyCode::ArrowUp), move_up)
                .action(VirtualAction::KeyButton(KeyCode::ArrowDown), move_down)
                .action(
                    VirtualAction::KeyButton(KeyCode::KeyR),
                    self.restart.clone(),
                )
                .action(VirtualAction::KeyButton(KeyCode::Escape), self.exit.clone()),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn update(&mut self, context: GameContext, delta_time: f32) {
        self.time += delta_time;
        self.update_player(delta_time);
        self.forest.update(self.player, delta_time);
        self.kitsune
            .update(&self.forest, self.player, self.dawn, delta_time);
        if self.caught > 0.0 || self.kitsune.caught(self.player) {
            self.caught += delta_time;
        } else if self.forest.lit_count() == LANTERN_COUNT {
            self.dawn = (self.dawn + delta_time / DAWN_DURATION).min(1.0);
        }
        self.update_sky();
        for ((_, view), action) in VIEWS.iter().zip(&self.views) {
            if action.get().is_pressed() {
                self.lighting.debug_view = *view;
            }
        }
        context.graphics.state.main_camera.transform.position = self.player.into();

        let finished = self.dawn >= 1.0 || self.caught > 1.0;
        if finished && self.restart.get().is_pressed() {
            *context.state_change = GameStateChange::Swap(Box::<Self>::default());
        } else if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw(&mut self, mut context: GameContext) {
        let camera = self.player;
        self.lighting.begin(context.draw, context.graphics);
        self.forest.draw_ground(&mut context, camera, self.time);
        let night = 1.0 - smoothstep(0.0, 0.3, self.dawn);
        self.forest
            .draw_solids(&mut context, camera, self.time, night);
        self.forest.draw_shrine(&mut context, self.time);
        self.draw_player(&mut context);
        self.kitsune.draw(&mut context, &self.forest);
        self.lighting.begin_shadows(context.draw, context.graphics);
        self.forest.draw_canopy_shadows(&mut context, camera);
        self.lighting.end_shadows(context.draw, context.graphics);
        self.lighting.resolve(context.draw, context.graphics);
        let sky_light = self.lighting.config.sky + self.lighting.config.sun;
        self.forest
            .draw_canopies(&mut context, camera, self.player, sky_light);
        self.forest.draw_torii_beam(&mut context);
        self.lighting.end(context.draw, context.graphics);
    }

    fn draw_gui(&mut self, _: GameContext) {
        let text = if self.caught > 0.0 {
            "The kitsune led you astray. Press R to try again.".to_owned()
        } else if self.dawn >= 1.0 {
            "The night is over. Press R to walk again.".to_owned()
        } else if self.dawn > 0.0 {
            "Dawn is coming...".to_owned()
        } else {
            format!(
                "Light the stone lanterns before the kitsune finds you: {} / {}",
                self.forest.lit_count(),
                LANTERN_COUNT
            )
        };
        text_box(TextBoxProps {
            text,
            horizontal_align: TextBoxHorizontalAlign::Center,
            vertical_align: TextBoxVerticalAlign::Bottom,
            font: TextBoxFont {
                name: "roboto".to_owned(),
                size: 24.0,
            },
            color: Color {
                r: 1.0,
                g: 0.9,
                b: 0.7,
                a: 0.8,
            },
            ..Default::default()
        });
    }
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
