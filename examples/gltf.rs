use quaso::{
    GameLauncher,
    animation::gltf::{
        GltfAnimationBlendSpace, GltfAnimationBlendSpacePoint, GltfAnimationTarget,
        GltfRenderablesOptions, GltfSceneAnimation, GltfSceneAttribute, GltfSceneInstance,
        GltfSceneTemplate, GltfSceneTransform,
    },
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    coroutine::{async_game_context, async_wait_for_asset},
    game::{GameInstance, GameState, GameStateChange},
    third_party::{
        nodio::query::Related,
        spitfire_draw::{
            utils::Drawable,
            {sprite::Sprite, utils::ShaderRef},
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::GlowBlending,
        },
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
            VirtualKeyCode,
        },
        vek::{Rgba, Vec2, Vec3},
    },
};
use std::{error::Error, pin::Pin};

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("GLTF")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

#[derive(Default)]
struct Preloader;

impl GameState for Preloader {
    fn timeline(
        &mut self,
        context: GameContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
        context.graphics.state.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.state.main_camera.screen_alignment = [0.5, 0.8].into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(1.5);
        context.graphics.state.main_camera.transform.scale = Vec3::new(1.0, -1.0, 1.0);

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

        let handle = context
            .assets
            .ensure("gltf://stickerman.glb?binary")
            .unwrap();

        Box::pin(async move {
            println!("Waiting for GLTF asset to load...");
            async_wait_for_asset(handle).await;
            println!("GLTF asset loaded.");

            {
                let context = async_game_context().await.unwrap();
                for handle in handle.dependencies(context.assets) {
                    println!(
                        "Dependency: {}",
                        handle.path(context.assets).unwrap().content()
                    );
                }
            }

            {
                let context = async_game_context().await.unwrap();
                let scene = context
                    .assets
                    .find("gltf-scene://stickerman.glb/Scene")
                    .unwrap();

                let instance = scene
                    .access::<&GltfSceneTemplate>(context.assets)
                    .instantiate(context.assets)
                    .with_animation(
                        "idle",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/TPose")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_animation(
                        "walk",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/Walk")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_animation(
                        "run",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/Run")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_animation(
                        "run",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/Run")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_animation(
                        "crouch",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/Crouch")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_animation(
                        "crouch-walk",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/CrouchWalk")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_animation(
                        "jump",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/Jump")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_animation(
                        "falling",
                        GltfSceneAnimation::new(
                            context
                                .assets
                                .find("gltf-anim://stickerman.glb/Falling")
                                .unwrap(),
                            context.assets,
                        )
                        .unwrap()
                        .weight(0.0)
                        .playing(true)
                        .looped(true),
                    )
                    .with_parameter("move-x", Default::default())
                    .with_parameter("move-y", Default::default())
                    .with_animation_node(
                        GltfAnimationBlendSpace::new(["move-y".into()])
                            .point(GltfAnimationBlendSpacePoint::new(
                                [-1.0],
                                GltfAnimationTarget::new("falling"),
                            ))
                            .point(GltfAnimationBlendSpacePoint::new(
                                [0.0],
                                GltfAnimationBlendSpace::new(["move-x".into()])
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [0.0],
                                        GltfAnimationTarget::new("idle"),
                                    ))
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [-1.0],
                                        GltfAnimationTarget::new("walk"),
                                    ))
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [-2.0],
                                        GltfAnimationTarget::new("run"),
                                    ))
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [1.0],
                                        GltfAnimationTarget::new("walk"),
                                    ))
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [2.0],
                                        GltfAnimationTarget::new("run"),
                                    )),
                            ))
                            .point(GltfAnimationBlendSpacePoint::new(
                                [1.0],
                                GltfAnimationBlendSpace::new(["move-x".into()])
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [0.0],
                                        GltfAnimationTarget::new("crouch"),
                                    ))
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [-1.0],
                                        GltfAnimationTarget::new("crouch-walk"),
                                    ))
                                    .point(GltfAnimationBlendSpacePoint::new(
                                        [1.0],
                                        GltfAnimationTarget::new("crouch-walk"),
                                    )),
                            )),
                    );

                instance.visit_tree(&mut |level, index, id, name, transform, mesh, skin, bone| {
                    println!(
                        "{}Node {} | id: {} | name: {} | transform: {} | mesh: {} | skin: {} | bone: {}",
                        "  ".repeat(level),
                        index,
                        id,
                        name.map(|n| n.as_str()).unwrap_or("<unnamed>"),
                        transform.is_some(),
                        mesh.is_some(),
                        skin.is_some(),
                        bone.is_some(),
                    );
                    true
                });
                *context.state_change = GameStateChange::Swap(Box::new(State {
                    instance,
                    movement: Default::default(),
                    delayed_movement: Default::default(),
                    sprint: InputActionRef::default(),
                }));
            }
        })
    }
}

struct State {
    instance: GltfSceneInstance,
    movement: CardinalInputCombinator,
    sprint: InputActionRef,
    delayed_movement: [f32; 2],
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        let left = InputActionRef::default();
        let right = InputActionRef::default();
        let up = InputActionRef::default();
        let down = InputActionRef::default();
        self.movement =
            CardinalInputCombinator::new(left.clone(), right.clone(), up.clone(), down.clone());

        context.input.push_mapping(
            InputMapping::default()
                .consume(InputConsume::Hit)
                .action(VirtualAction::KeyButton(VirtualKeyCode::W), up)
                .action(VirtualAction::KeyButton(VirtualKeyCode::S), down)
                .action(VirtualAction::KeyButton(VirtualKeyCode::A), left)
                .action(VirtualAction::KeyButton(VirtualKeyCode::D), right)
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::LShift),
                    self.sprint.clone(),
                ),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, context: GameContext, delta_time: f32) {
        let mut movement = self.movement.get();
        let sprint = if self.sprint.get().is_hold() {
            2.0
        } else {
            1.0
        };
        movement[0] *= sprint;
        self.delayed_movement =
            std::array::from_fn(|i| self.delayed_movement[i] * 0.9 + movement[i] * 0.1);

        self.instance
            .parameter("move-x")
            .unwrap()
            .set(self.delayed_movement[0]);
        self.instance
            .parameter("move-y")
            .unwrap()
            .set(self.delayed_movement[1]);
        self.instance
            .update_and_apply_animations(delta_time, context.assets);
    }

    fn draw(&mut self, context: GameContext) {
        let renderables = self
            .instance
            .build_renderables(
                context.assets,
                &GltfRenderablesOptions::default().sort_triangles_by_max_positive_z(),
            )
            .unwrap();
        renderables.draw(context.draw, context.graphics);

        if let Some(transform) = self
            .instance
            .query_bone_by_name::<Related<GltfSceneAttribute, &GltfSceneTransform>>("Bone.012")
        {
            Sprite::default()
                .shader(ShaderRef::name("color"))
                .position(transform.world_matrix().mul_point(Vec2::zero()))
                .pivot(0.5.into())
                .size(0.1.into())
                .tint(Rgba::new(1.0, 0.0, 0.0, 0.5))
                .blending(GlowBlending::Alpha)
                .draw(context.draw, context.graphics);
        }
    }
}
