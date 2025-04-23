use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState},
    third_party::{
        anim8::{spline::Spline, utils::factor_iter},
        spitfire_draw::{
            primitives::PrimitivesEmitter,
            utils::{Drawable, ShaderRef},
        },
        spitfire_glow::graphics::{CameraScaling, Shader},
        spitfire_input::{
            ArrayInputCombinator, InputAxisRef, InputConsume, InputMapping, VirtualAxis,
        },
        vek::{Rgba, Vec2},
    },
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(State::default()).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Path")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

struct State {
    spline: Spline<[f32; 2]>,
    mouse_xy: Option<ArrayInputCombinator<2>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            spline: Spline::smooth(
                &[
                    [-100.0, -100.0],
                    [100.0, -100.0],
                    [-100.0, 100.0],
                    [100.0, 100.0],
                ],
                1.0,
            )
            .unwrap(),
            mouse_xy: None,
        }
    }
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        context.graphics.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.main_camera.screen_alignment = 0.5.into();
        context.graphics.main_camera.scaling = CameraScaling::FitVertical(300.0);

        let pointer_x = InputAxisRef::default();
        let pointer_y = InputAxisRef::default();
        self.mouse_xy = Some(ArrayInputCombinator::new([
            pointer_x.clone(),
            pointer_y.clone(),
        ]));
        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .axis(VirtualAxis::MousePositionX, pointer_x)
                .axis(VirtualAxis::MousePositionY, pointer_y),
        );

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
    }

    fn draw(&mut self, context: GameContext) {
        let emitter = PrimitivesEmitter::default().shader(ShaderRef::name("color"));

        let tint = Rgba::new(0.25, 0.25, 1.0, 1.0);
        emitter
            .emit_brush(factor_iter(50).map(|factor| {
                (
                    self.spline.sample(factor).into(),
                    (1.0 - factor * factor) * 5.0,
                    tint,
                )
            }))
            .draw(context.draw, context.graphics);

        for point in self.spline.points() {
            emitter
                .emit_circle(point.point.into(), 2.0, 0.1)
                .tint(Rgba::new(1.0, 0.25, 0.25, 1.0))
                .draw(context.draw, context.graphics);
        }

        if let Some(mouse_xy) = self.mouse_xy.as_ref() {
            let source = Vec2::from(mouse_xy.get());
            let source = context
                .graphics
                .main_camera
                .screen_matrix()
                .mul_point(source);
            let source = context
                .graphics
                .main_camera
                .world_matrix()
                .inverted()
                .mul_point(source);
            let time = self
                .spline
                .find_time_closest_to_point(&source.into_array())
                .0;
            let target = Vec2::from(self.spline.sample(time));
            emitter
                .emit_circle(target, 3.0, 0.1)
                .draw(context.draw, context.graphics);
            emitter
                .emit_lines([source, target])
                .draw(context.draw, context.graphics);
        }
    }
}
