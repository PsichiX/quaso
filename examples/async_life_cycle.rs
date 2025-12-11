use quaso::{
    GameLauncher,
    animation::frame::SpriteFrameAnimation,
    assets::{anim_texture::AnimTextureAsset, make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    coroutine::{
        async_delta_time, async_game_context, async_heartbeat_bound, async_next_frame,
        async_wait_for_assets,
    },
    game::{GameInstance, GameState, GameStateChange},
    third_party::{
        moirai::jobs::JobLocation,
        raui_core::{
            layout::CoordsMappingScaling,
            widget::{
                component::text_box::TextBoxProps,
                unit::text::{TextBoxFont, TextBoxHorizontalAlign, TextBoxVerticalAlign},
                utils::Color,
            },
        },
        raui_immediate_widgets::core::text_box,
        send_wrapper::SendWrapper,
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, TextureRef},
        },
        spitfire_glow::{
            graphics::{CameraScaling, Shader},
            renderer::GlowTextureFiltering,
        },
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
        },
        vek::Vec2,
        windowing::event::VirtualKeyCode,
    },
    value::Val,
};
use std::{error::Error, pin::Pin};

const SPEED: f32 = 100.0;

fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("Async life-cycle!")
    .config(Config::load_from_file("./resources/GameConfig.toml")?)
    .run();
    Ok(())
}

#[derive(Default)]
struct Preloader;

impl GameState for Preloader {
    // Timelines allow to define entire state lifetime as a coroutine, which is
    // automatically canceled when state exits.
    fn timeline(
        &mut self,
        context: GameContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
        context.graphics.state.color = [0.2, 0.2, 0.2, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(500.0);
        context.gui.coords_map_scaling = CoordsMappingScaling::FitVertical(500.0);

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

        let font = context.assets.schedule("font://roboto.ttf").unwrap();

        let ferris = context
            .assets
            .schedule("animtexture://ferris-bongo.gif")
            .unwrap();

        // Return future that waits for scheduled assets to load and then swaps state.
        Box::pin(async move {
            async_wait_for_assets([font, ferris]).await;

            let context = async_game_context().await.unwrap();
            *context.state_change = GameStateChange::Swap(Box::new(State::default()));
        })
    }
}

struct State {
    // Val is a thread-safe mutable owned value container that can be shared
    // between async tasks. We wrap Sprite and SpriteFrameAnimation in SendWrapper
    // because they do not implement Send+Sync traits by default and we need to
    // mark them as safe to be used only on the origin thread - this works because
    // coroutines and async life-cycle futures run only on main thread.
    ferris: Val<SendWrapper<Sprite>>,
    ferris_anim: Val<SendWrapper<SpriteFrameAnimation>>,
    movement: Val<CardinalInputCombinator>,
    exit: InputActionRef,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ferris: Val::new(SendWrapper::new(Default::default())),
            ferris_anim: Val::new(SendWrapper::new(Default::default())),
            movement: Val::new(Default::default()),
            exit: Default::default(),
        }
    }
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        *self.ferris.write() = SendWrapper::new(
            Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name(""),
                filtering: GlowTextureFiltering::Linear,
            })
            .pivot(0.5.into()),
        );

        *self.ferris_anim.write() = SendWrapper::new(
            context
                .assets
                .ensure("animtexture://ferris-bongo.gif")
                .unwrap()
                .access::<&AnimTextureAsset>(context.assets)
                .build_animation(TextureRef::name("ferris-bongo.gif")),
        );
        self.ferris_anim.write().animation.speed = 0.5;
        self.ferris_anim.write().animation.looping = true;
        self.ferris_anim.write().animation.play();

        let move_left = InputActionRef::default();
        let move_right = InputActionRef::default();
        let move_up = InputActionRef::default();
        let move_down = InputActionRef::default();
        *self.movement.write() = CardinalInputCombinator::new(
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
                    VirtualAction::KeyButton(VirtualKeyCode::Escape),
                    self.exit.clone(),
                ),
        );

        // To be able to access state data from async tasks, we need to create
        // pointers to values containers - pointers are special async-safe
        // lazily accessed references that can be sent between tasks.
        let ferris = self.ferris.pointer();
        let ferris_anim = self.ferris_anim.pointer();
        let movement = self.movement.pointer();
        let exit = self.exit.clone();

        // We spawn state heartbeat-bound async tasks into various job queues.
        // Heartbeat-bound tasks are automatically canceled when state exits.
        // If we would not bind tasks to heartbeat, they would continue running
        // even after state exit, as their main use case is to run across states.
        // This pattern is useful for long-running tasks like frame loops.
        context.fixed_update_queue.spawn(
            JobLocation::Local,
            async_heartbeat_bound([context.state_heartbeat.clone()], async move {
                loop {
                    let delta_time = async_delta_time().await;
                    let context = async_game_context().await.unwrap();

                    ferris_anim.write().animation.update(delta_time);
                    ferris_anim.write().apply_to_sprite(&mut ferris.write(), 0);

                    let movement = Vec2::<f32>::from(movement.read().get());
                    ferris.write().transform.position += movement * SPEED * delta_time;

                    if exit.get().is_pressed() {
                        *context.state_change = GameStateChange::Pop;
                    }

                    async_next_frame().await;
                }
            }),
        );

        let ferris = self.ferris.pointer();
        context.draw_queue.spawn(
            JobLocation::Local,
            async_heartbeat_bound([context.state_heartbeat.clone()], async move {
                loop {
                    let context = async_game_context().await.unwrap();
                    ferris.read().draw(context.draw, context.graphics);

                    async_next_frame().await;
                }
            }),
        );

        context.draw_gui_queue.spawn(
            JobLocation::Local,
            async_heartbeat_bound([context.state_heartbeat.clone()], async move {
                loop {
                    text_box(TextBoxProps {
                        text: "Hello, World!".to_owned(),
                        horizontal_align: TextBoxHorizontalAlign::Center,
                        vertical_align: TextBoxVerticalAlign::Bottom,
                        font: TextBoxFont {
                            name: "roboto.ttf".to_owned(),
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

                    async_next_frame().await;
                }
            }),
        );
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }
}
