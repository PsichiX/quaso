use crate::game::machine::SlotMachine;
use quaso::{
    assets::shader::ShaderAsset,
    context::GameContext,
    coroutine::async_heartbeat_bound,
    game::{GameObject, GameState, GameStateChange},
    gc::Gc,
    third_party::{
        spitfire_glow::graphics::{CameraScaling, Shader},
        spitfire_input::{InputActionRef, InputConsume, InputMapping, VirtualAction},
        windowing::event::VirtualKeyCode,
    },
};

pub const WRAPPED_TEXTURED_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
in vec4 v_color;
in vec3 v_uv;
out vec4 o_color;
uniform sampler2DArray u_image;

void main() {
    o_color = texture(u_image, fract(v_uv)) * v_color;
}
"#;

#[derive(Default)]
pub struct Gameplay {
    pub machine: Gc<SlotMachine>,
    pub action: InputActionRef,
    pub exit: InputActionRef,
}

impl GameState for Gameplay {
    fn enter(&mut self, context: GameContext) {
        context.graphics.state.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(624.0);

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
                "shader://image-wrapped",
                (ShaderAsset::new(
                    Shader::TEXTURED_VERTEX_2D,
                    WRAPPED_TEXTURED_FRAGMENT,
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

        context.assets.ensure("group://index.txt").unwrap();

        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Space),
                    self.action.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Escape),
                    self.exit.clone(),
                ),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, context: GameContext, _: f32) {
        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
            return;
        }
        if self.action.get().is_down() {
            let machine = self.machine.reference();
            context
                .jobs
                .unwrap()
                .coroutine(async_heartbeat_bound([machine.heartbeat()], async {
                    if let Some(index) = SlotMachine::spin(machine).await {
                        match index {
                            0 => println!("Payline: 1"),
                            1 => println!("Payline: 10"),
                            2 => println!("Payline: 100"),
                            3 => println!("Payline: 1000"),
                            _ => {}
                        }
                    }
                }));
        }
    }

    fn draw(&mut self, mut context: GameContext) {
        self.machine.write().draw(&mut context);
    }
}
