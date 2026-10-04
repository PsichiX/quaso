use quaso::{
    GameLauncher,
    assets::{make_directory_database, shader::ShaderAsset},
    config::Config,
    context::GameContext,
    game::{GameInstance, GameObject, GameState, GameStateChange},
    gc::Gc,
    third_party::{
        spitfire_draw::{
            sprite::Sprite,
            utils::{Drawable, ShaderRef},
        },
        spitfire_glow::graphics::{CameraScaling, Shader},
        spitfire_input::{
            CardinalInputCombinator, InputActionRef, InputConsume, InputMapping, VirtualAction,
            VirtualKeyCode,
        },
        vek::{Rgba, Vec2},
    },
};
use std::error::Error;

// Example demonstrating basic usage of garbage-collected objects.
// It's important to note that `Gc` type is not a typical garbage-collected
// pointer. Think of it as GC with ownership flavor - newly created `Gc`
// instance owns the object memory and all other instances created via
// `reference` method are just weak references. That means, when owning `Gc`
// instance is dropped, the object memory is freed, and all other weak
// references become invalid. This approach allows to have cyclic references
// as well as self-references between `Gc` objects without memory leaks.
fn main() -> Result<(), Box<dyn Error>> {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_directory_database("./resources/").unwrap();
    }))
    .title("GC")
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
    }

    fn update(&mut self, context: GameContext, _: f32) {
        if !context.assets.is_busy() {
            *context.state_change = GameStateChange::Swap(Box::new(State::default()));
        }
    }
}

#[derive(Default)]
struct State {
    // Player controller is owned by the state.
    player_controller: Option<Gc<PlayerController>>,
    // Actors are also owned by the state.
    actors: Vec<Gc<Actor>>,
    switch: InputActionRef,
    exit: InputActionRef,
    spawn: InputActionRef,
    destroy: InputActionRef,
}

impl GameState for State {
    fn enter(&mut self, context: GameContext) {
        let move_left = InputActionRef::default();
        let move_right = InputActionRef::default();
        let move_up = InputActionRef::default();
        let move_down = InputActionRef::default();

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
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Left),
                    move_left.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Right),
                    move_right.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Up),
                    move_up.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Down),
                    move_down.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Space),
                    self.switch.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Insert),
                    self.spawn.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Delete),
                    self.destroy.clone(),
                )
                .action(
                    VirtualAction::KeyButton(VirtualKeyCode::Escape),
                    self.exit.clone(),
                ),
        );

        // Create the player controller pointer owned by the state.
        self.player_controller = Some(Gc::new(PlayerController {
            movement_input: CardinalInputCombinator::new(move_left, move_right, move_up, move_down),
            controls: None,
        }));

        self.spawn_actor(Vec2::new(-100.0, 0.0), 150.0);
        self.spawn_actor(Vec2::new(100.0, 0.0), 100.0);
    }

    fn exit(&mut self, context: GameContext) {
        context.input.pop_mapping();
    }

    fn fixed_update(&mut self, mut context: GameContext, delta_time: f32) {
        if let Some(controller) = &mut self.player_controller {
            // Process the player controller using writable access to GC pointer.
            controller.write().process(&mut context, delta_time);
        }

        for actor in &mut self.actors {
            actor.write().process(&mut context, delta_time);
        }

        if self.switch.get().is_pressed() {
            self.switch_to_next();
        } else if self.spawn.get().is_pressed() {
            self.spawn_actor(Default::default(), 100.0);
        } else if self.destroy.get().is_pressed() {
            self.destroy_current();
        }

        if self.exit.get().is_pressed() {
            *context.state_change = GameStateChange::Pop;
        }
    }

    fn draw(&mut self, mut context: GameContext) {
        for actor in &mut self.actors {
            actor.write().draw(&mut context);
        }

        if let Some(controller) = &mut self.player_controller {
            controller.write().draw(&mut context);
        }
    }
}

impl State {
    fn switch_to_next(&mut self) {
        if !self.actors.is_empty()
            && let Some(controller) = &mut self.player_controller
        {
            let controller = &mut *controller.write();
            if let Some(current) = &controller.controls {
                // We can tell if two GC pointers are pointing to same object.
                if let Some(index) = self.actors.iter().position(|a| Gc::ptr_eq(a, current)) {
                    let next = self.actors[(index + 1) % self.actors.len()].reference();
                    controller.controls = Some(next);
                } else {
                    controller.controls = Some(self.actors[0].reference());
                }
            }
        }
    }

    fn spawn_actor(&mut self, position: Vec2<f32>, speed: f32) {
        let actor = Gc::new(Actor {
            sprite: Sprite::default()
                .shader(ShaderRef::name("color"))
                .position(position)
                .size(50.0.into())
                .pivot(0.5.into())
                .tint(Rgba::red()),
            speed,
        });
        if let Some(controller) = &mut self.player_controller {
            controller.write().controls = Some(actor.reference());
        }
        self.actors.push(actor);
    }

    fn destroy_current(&mut self) {
        let switch = if let Some(controller) = &mut self.player_controller {
            let controller = &mut *controller.write();
            if let Some(current) = &controller.controls {
                // Remove the current actor from the list, dropping the owning
                // GC pointer, which will free the object memory and invalidate
                // all other weak references pointing to it.
                self.actors.retain(|actor| !Gc::ptr_eq(actor, current));
                true
            } else {
                false
            }
        } else {
            false
        };
        if switch {
            self.switch_to_next();
        }
    }
}

struct Actor {
    sprite: Sprite,
    speed: f32,
}

impl GameObject for Actor {
    fn draw(&mut self, context: &mut GameContext) {
        self.sprite.draw(context.draw, context.graphics);
    }
}

struct PlayerController {
    pub movement_input: CardinalInputCombinator,
    pub controls: Option<Gc<Actor>>,
}

impl GameObject for PlayerController {
    fn process(&mut self, _: &mut GameContext, delta_time: f32) {
        if let Some(actor) = &mut self.controls {
            let mut actor = actor.write();
            let movement = Vec2::from(self.movement_input.get()) * actor.speed * delta_time;
            actor.sprite.transform.position += movement;
        }
    }

    fn draw(&mut self, context: &mut GameContext) {
        if let Some(actor) = &mut self.controls {
            Sprite::default()
                .shader(ShaderRef::name("color"))
                .position(actor.read().sprite.transform.position.into())
                .size(10.0.into())
                .pivot(0.5.into())
                .tint(Rgba::blue())
                .draw(context.draw, context.graphics);
        }
    }
}
