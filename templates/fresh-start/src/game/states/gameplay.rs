use quaso::{
    context::GameContext,
    game::{GameState, GameStateChange},
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
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, TextureRef},
        },
        spitfire_glow::{graphics::CameraScaling, renderer::GlowTextureFiltering},
        spitfire_input::KeyCode,
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        vek::Vec2,
    },
};

const SPEED: f32 = 100.0;

pub struct Gameplay {
    pub ferris: Sprite,
    pub movement: CardinalInputCombinator,
    pub exit: InputActionRef,
}

impl Default for Gameplay {
    fn default() -> Self {
        Self {
            ferris: Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name("ferris"),
                filtering: GlowTextureFiltering::Linear,
            })
            .pivot(0.5.into()),
            movement: Default::default(),
            exit: Default::default(),
        }
    }
}

impl GameState for Gameplay {
    fn enter(&mut self, context: GameContext) {
        context.graphics.state.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(500.0);
        context.gui.coords_map_scaling = CoordsMappingScaling::FitVertical(500.0);

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
        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .action(VirtualAction::KeyButton(KeyCode::KeyA), move_left.clone())
                .action(VirtualAction::KeyButton(KeyCode::KeyD), move_right.clone())
                .action(VirtualAction::KeyButton(KeyCode::KeyW), move_up.clone())
                .action(VirtualAction::KeyButton(KeyCode::KeyS), move_down.clone())
                .action(VirtualAction::KeyButton(KeyCode::ArrowLeft), move_left)
                .action(VirtualAction::KeyButton(KeyCode::ArrowRight), move_right)
                .action(VirtualAction::KeyButton(KeyCode::ArrowUp), move_up)
                .action(VirtualAction::KeyButton(KeyCode::ArrowDown), move_down)
                .action(VirtualAction::KeyButton(KeyCode::Escape), self.exit.clone()),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {
        let movement = Vec2::<f32>::from(self.movement.get());
        self.ferris.transform.position += movement * SPEED * delta_time;

        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw(&mut self, context: GameContext) {
        self.ferris.draw(context.draw, context.graphics);
    }

    fn draw_gui(&mut self, _: GameContext) {
        text_box(TextBoxProps {
            text: "Hello, World!".to_owned(),
            horizontal_align: TextBoxHorizontalAlign::Center,
            vertical_align: TextBoxVerticalAlign::Bottom,
            font: TextBoxFont {
                name: "roboto".to_owned(),
                size: 50.0,
            },
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            ..Default::default()
        });
    }
}
