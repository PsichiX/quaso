use quaso::{
    GameLauncher,
    animation::spine::{
        BudgetedSpineSkeleton, BudgetedSpineSkeletonLodSwitchStrategy, LodSpineSkeleton,
        SpineSkeleton,
    },
    assets::{make_directory_database, shader::ShaderAsset, spine::SpineAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameState, GameStateChange},
    third_party::{
        spitfire_draw::utils::Drawable,
        spitfire_glow::graphics::{CameraScaling, Shader},
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        vek::Vec2,
        windowing::event::VirtualKeyCode,
    },
};
use std::error::Error;

const SPEED: f32 = 200.0;

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Spine 2D")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

#[derive(Default)]
struct Preloader;

impl GameState for Preloader {
    fn enter(&mut self, context: GameContext) {
        context.graphics.state.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(500.0);

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
                "shader://text",
                (ShaderAsset::new(Shader::TEXT_VERTEX, Shader::TEXT_FRAGMENT),),
            )
            .unwrap();

        context.assets.ensure("spine://robot-lod0.zip").unwrap();
        context.assets.ensure("spine://robot-lod1.zip").unwrap();

        *context.state_change = GameStateChange::Swap(Box::new(State::default()));
    }
}

#[derive(Default)]
struct State {
    skeleton: Option<BudgetedSpineSkeleton>,
    movement: CardinalInputCombinator,
    lod0: InputActionRef,
    lod1: InputActionRef,
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        // Load Spine skeleton LODs assets.
        let asset_lod0 = context
            .assets
            .find("spine://robot-lod0.zip")
            .unwrap()
            .access::<&SpineAsset>(context.assets);
        let asset_lod1 = context
            .assets
            .find("spine://robot-lod1.zip")
            .unwrap()
            .access::<&SpineAsset>(context.assets);

        // Create Spine skeleton instances for each LOD.
        let lod0 = SpineSkeleton::new(asset_lod0);
        // Since we start with LOD 0, we need to play animation on this LOD.
        lod0.play_animation("idle", 0, 0.75, true).unwrap();
        let lod1 = SpineSkeleton::new(asset_lod1);

        // Create and setup budgeted Spine skeleton.
        self.skeleton = Some(
            BudgetedSpineSkeleton::default()
                .lod_switch_strategy(BudgetedSpineSkeletonLodSwitchStrategy {
                    // Since skeleton is playing animations, we need to transfer
                    // just root bone transform to make new LOD be at the exact
                    // place as old LOD was.
                    transfer_root_bone_transform: true,
                    // Make sure that when LODs are switched, same animation is
                    // running on new LOD as it was on old LOD.
                    synchronize_animations: true,
                    ..Default::default()
                })
                // High quality skeleton with IK and physics animations.
                .with_lod(LodSpineSkeleton {
                    skeleton: lod0,
                    refresh_delay: 0.0,
                })
                // Low quality skeleton with simple bone transform animations to
                // make animation process faster.
                .with_lod(LodSpineSkeleton {
                    skeleton: lod1,
                    // We also run it at lower frequency.
                    refresh_delay: 0.05,
                }),
        );

        // Setup inputs for moving the skeleton and switching LODs.
        let move_left = InputActionRef::default();
        let move_right = InputActionRef::default();
        let move_up = InputActionRef::default();
        let move_down = InputActionRef::default();
        self.lod0 = InputActionRef::default();
        self.lod1 = InputActionRef::default();
        self.movement = CardinalInputCombinator::new(
            move_left.clone(),
            move_right.clone(),
            move_up.clone(),
            move_down.clone(),
        );
        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::A),
                    move_left.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::D),
                    move_right.clone(),
                )
                .action(VirtualAction::KeyButton(VirtualKeyCode::W), move_up.clone())
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::S),
                    move_down.clone(),
                )
                .action(VirtualAction::KeyButton(VirtualKeyCode::Left), move_left)
                .action(VirtualAction::KeyButton(VirtualKeyCode::Right), move_right)
                .action(VirtualAction::KeyButton(VirtualKeyCode::Up), move_up)
                .action(VirtualAction::KeyButton(VirtualKeyCode::Down), move_down)
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Key1),
                    self.lod0.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Key2),
                    self.lod1.clone(),
                ),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, _: GameContext, delta_time: f32) {
        let Some(budgeted_skeleton) = self.skeleton.as_mut() else {
            return;
        };

        // Switch LODs if user trigger input actions.
        if self.lod0.get().is_pressed() {
            budgeted_skeleton.set_lod(0);
        } else if self.lod1.get().is_pressed() {
            budgeted_skeleton.set_lod(1);
        }

        // Update skeleton root bone transform based on user movement input.
        if let Some(skeleton) = budgeted_skeleton.lod_skeleton_mut() {
            let movement = Vec2::<f32>::from(self.movement.get());
            skeleton
                .skeleton
                .update_local_transform(None, false, |position, _, _| {
                    position.x += movement.x * SPEED * delta_time;
                    position.y += movement.y * SPEED * delta_time;
                });
        };
        // Update skeleton state based on its refresh frequency.
        budgeted_skeleton.try_refresh(delta_time);
    }

    fn draw(&mut self, context: GameContext) {
        let Some(skeleton) = self.skeleton.as_ref() else {
            return;
        };
        skeleton.draw(context.draw, context.graphics);
    }
}
