use crate::game::{
    drawables::light::draw_sphere_light,
    enemy::EnemyState,
    item::{Item, ItemKind},
    player::PlayerState,
    states::{
        game_end::{GameEnd, GameEndReason},
        main_menu::MainMenu,
    },
    torch::Torch,
    ui::{health_bar::health_bar, world_to_screen_content_layout},
    utils::{
        events::{Event, Events},
        space::{Space, SpaceObject, SpaceObjectId},
    },
};
use quaso::{
    character::Character,
    context::GameContext,
    game::{GameObject, GameState, GameStateChange},
    gamepad::GamepadManager,
    third_party::{
        kira::sound::static_sound::StaticSoundHandle,
        rand::{thread_rng, Rng},
        raui_core::{
            layout::CoordsMappingScaling,
            widget::{
                component::text_box::TextBoxProps,
                unit::{content::ContentBoxItemLayout, text::TextBoxFont},
                utils::{Color, Rect},
            },
        },
        raui_immediate_widgets::core::text_box,
        spitfire_draw::{
            canvas::Canvas,
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, ShaderRef, TextureRef},
        },
        spitfire_glow::{
            graphics::CameraScaling,
            renderer::{GlowBlending, GlowTextureFiltering, GlowTextureFormat, GlowUniformValue},
        },
        spitfire_input::{InputActionRef, InputConsume, InputMapping, VirtualAction},
        typid::ID,
        windowing::event::VirtualKeyCode,
    },
};
use std::collections::BTreeMap;

pub struct Gameplay {
    map: Sprite,
    player: Character<PlayerState>,
    enemies: BTreeMap<ID<EnemyState>, Character<EnemyState>>,
    items: BTreeMap<ID<Item>, Item>,
    torch: Torch,
    darkness: Option<Canvas>,
    exit: InputActionRef,
    exit_handle: Option<ID<InputMapping>>,
    map_radius: f32,
    music_forest: Option<StaticSoundHandle>,
    music_battle: Option<StaticSoundHandle>,
    gamepads: GamepadManager,
}

impl Default for Gameplay {
    fn default() -> Self {
        let gamepads = GamepadManager::default();

        Self {
            map: Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name("map/level-0"),
                filtering: GlowTextureFiltering::Linear,
            })
            .pivot(0.5.into()),
            player: PlayerState::new_character([0.0, 0.0, 0.0], &gamepads),
            enemies: Default::default(),
            items: Default::default(),
            torch: Torch::new([0.0, 0.0]),
            darkness: None,
            exit: Default::default(),
            exit_handle: None,
            map_radius: 800.0,
            music_forest: None,
            music_battle: None,
            gamepads,
        }
    }
}

impl GameState for Gameplay {
    fn enter(&mut self, mut context: GameContext) {
        context.graphics.state.color = [0.0, 0.3, 0.0, 1.0];
        context.graphics.state.main_camera.screen_alignment = 0.5.into();
        context.graphics.state.main_camera.scaling = CameraScaling::FitVertical(512.0);
        context.gui.coords_map_scaling = CoordsMappingScaling::FitVertical(1024.0);

        self.music_forest = {
            let mut handle = context.audio.play("forest").unwrap();
            handle.set_volume(0.0, Default::default());
            handle.set_loop_region(..);
            Some(handle)
        };

        self.music_battle = {
            let mut handle = context.audio.play("battle").unwrap();
            handle.set_volume(0.0, Default::default());
            handle.set_loop_region(..);
            Some(handle)
        };

        self.exit_handle = Some(context.input.push_mapping(
            InputMapping::default().consume(InputConsume::Hit).action(
                VirtualAction::KeyButton(VirtualKeyCode::Escape),
                self.exit.clone(),
            ),
        ));

        self.player.activate(&mut context);

        for _ in 0..6 {
            let position = [
                thread_rng().gen_range((-self.map_radius)..=self.map_radius),
                thread_rng().gen_range((-self.map_radius)..=self.map_radius),
                0.0,
            ];
            self.enemies.insert(
                ID::new(),
                EnemyState::new_character(position).activated(&mut context),
            );
        }

        for _ in 0..20 {
            let position = [
                thread_rng().gen_range((-self.map_radius)..=self.map_radius),
                thread_rng().gen_range((-self.map_radius)..=self.map_radius),
            ];
            self.items
                .insert(ID::new(), Item::new(ItemKind::random(), position));
        }

        self.darkness = Some(
            Canvas::from_screen(vec![GlowTextureFormat::Monochromatic], context.graphics)
                .unwrap()
                .color([0.0, 0.0, 0.0, 0.0]),
        );
    }

    fn exit(&mut self, mut context: GameContext) {
        self.player.deactivate(&mut context);

        for (_, mut enemy) in std::mem::take(&mut self.enemies) {
            enemy.deactivate(&mut context);
        }

        if let Some(id) = self.exit_handle {
            context.input.remove_mapping(id);
            self.exit_handle = None;
        }

        if let Some(handle) = self.music_forest.as_mut() {
            handle.stop(Default::default());
        }
        if let Some(handle) = self.music_battle.as_mut() {
            handle.stop(Default::default());
        }
    }

    fn fixed_update(&mut self, mut context: GameContext, delta_time: f32) {
        self.maintain(delta_time);

        if self.exit.get().is_down() {
            *context.state_change = GameStateChange::Swap(Box::new(MainMenu));
        }

        self.process_game_objects(&mut context, delta_time);

        self.resolve_collisions();

        self.execute_events(&mut context);

        self.update_ambient_music();
    }

    fn draw(&mut self, mut context: GameContext) {
        self.map.draw(context.draw, context.graphics);

        self.torch.draw(&mut context);

        for item in self.items.values_mut() {
            item.draw(&mut context);
        }

        for enemy in self.enemies.values_mut() {
            enemy.draw(&mut context);
        }

        self.player.draw(&mut context);

        if let Some(canvas) = &mut self.darkness {
            let _ = canvas.match_to_screen(context.graphics);

            canvas.with(context.draw, context.graphics, true, |draw, graphics| {
                draw_sphere_light(
                    self.player
                        .state
                        .read()
                        .unwrap()
                        .sprite
                        .transform
                        .position
                        .xy(),
                    200.0,
                    0.0..=1.0,
                    1.0,
                    draw,
                    graphics,
                );

                draw_sphere_light(
                    self.torch.sprite.transform.position.xy(),
                    350.0,
                    0.0..=1.0,
                    1.0,
                    draw,
                    graphics,
                );
            });

            Sprite::single(
                canvas
                    .sprite_texture(0, "u_image".into(), GlowTextureFiltering::Linear)
                    .unwrap(),
            )
            .pivot([0.0, 1.0].into())
            .scale([1.0, -1.0].into())
            .shader(ShaderRef::name("lighting"))
            .blending(GlowBlending::Alpha)
            .screen_space(true)
            .uniform(
                "u_dark_color".into(),
                GlowUniformValue::F4([0.0, 0.0, 0.0, 1.0]),
            )
            .uniform(
                "u_light_color".into(),
                GlowUniformValue::F4([0.0, 0.0, 0.0, 0.0]),
            )
            .draw(context.draw, context.graphics);
        }
    }

    fn draw_gui(&mut self, context: GameContext) {
        let health_bar_rectangle = Rect {
            left: -50.0,
            right: 50.0,
            top: -60.0,
            bottom: -40.0,
        };

        {
            let state = self.player.state.read().unwrap();
            let layout = world_to_screen_content_layout(
                state.sprite.transform.position.xy(),
                health_bar_rectangle,
                &context,
            );

            health_bar(layout, state.health);
        }

        for enemy in self.enemies.values() {
            let state = enemy.state.read().unwrap();
            let layout = world_to_screen_content_layout(
                state.sprite.transform.position.xy(),
                health_bar_rectangle,
                &context,
            );

            health_bar(layout, state.health);
        }

        text_box((
            ContentBoxItemLayout {
                margin: 40.0.into(),
                ..Default::default()
            },
            TextBoxProps {
                text: format!(
                    "Weapon: {:?}\nEnemies: {}\nItems: {}",
                    self.player.state.read().unwrap().weapon,
                    self.enemies.len(),
                    self.items.len(),
                ),
                font: TextBoxFont {
                    name: "roboto".to_owned(),
                    size: 28.0,
                },
                color: Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
                ..Default::default()
            },
        ));
    }
}

impl Gameplay {
    fn maintain(&mut self, delta_time: f32) {
        self.gamepads.maintain();
        Events::maintain(delta_time);

        Space::write().write().unwrap().maintain(
            self.enemies
                .iter()
                .map(|(id, enemy)| SpaceObject {
                    id: SpaceObjectId::Enemy(*id),
                    position: enemy.state.read().unwrap().sprite.transform.position.xy(),
                    collider_radius: 20.0,
                })
                .chain(self.items.iter().map(|(id, item)| SpaceObject {
                    id: SpaceObjectId::Item(*id),
                    position: item.sprite.transform.position.xy(),
                    collider_radius: 10.0,
                }))
                .chain(std::iter::once(SpaceObject {
                    id: SpaceObjectId::Player,
                    position: self
                        .player
                        .state
                        .read()
                        .unwrap()
                        .sprite
                        .transform
                        .position
                        .xy(),
                    collider_radius: 20.0,
                }))
                .collect(),
        );
    }

    fn process_game_objects(&mut self, context: &mut GameContext, delta_time: f32) {
        self.torch.process(context, delta_time);

        self.player.process(context, delta_time);

        for enemy in self.enemies.values_mut() {
            enemy.process(context, delta_time);
            enemy
                .state
                .write()
                .unwrap()
                .sense_player(&self.player.state.read().unwrap());
        }
    }

    fn execute_events(&mut self, context: &mut GameContext) {
        Events::read(|events| {
            self.player.state.write().unwrap().execute_events(events);

            for (id, enemy) in &mut self.enemies {
                enemy.state.write().unwrap().execute_events(*id, events);
            }

            for event in events {
                match event {
                    Event::KillPlayer => {
                        *context.state_change =
                            GameStateChange::Swap(Box::new(GameEnd::new(GameEndReason::Lost)));
                    }
                    Event::KillEnemy { id } => {
                        self.enemies.remove(id);
                        if self.enemies.is_empty() {
                            Events::write_delayed(2.0, Event::WinGame);
                        }
                    }
                    Event::KillItem { id } => {
                        self.items.remove(id);
                    }
                    Event::WinGame => {
                        *context.state_change =
                            GameStateChange::Swap(Box::new(GameEnd::new(GameEndReason::Won)));
                    }
                    Event::PlaySound(id) => {
                        context.audio.play(id.as_ref());
                    }
                    _ => {}
                }
            }
        });
    }

    fn update_ambient_music(&mut self) {
        let player_position = self
            .player
            .state
            .read()
            .unwrap()
            .sprite
            .transform
            .position
            .xy();
        let factor = self
            .enemies
            .values()
            .map(|enemy| {
                enemy
                    .state
                    .read()
                    .unwrap()
                    .sprite
                    .transform
                    .position
                    .xy()
                    .distance(player_position)
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(f32::INFINITY)
            .min(300.0)
            / 300.0;
        if let Some(handle) = self.music_forest.as_mut() {
            handle.set_volume(factor * 2.0, Default::default());
        }
        if let Some(handle) = self.music_battle.as_mut() {
            handle.set_volume((1.0 - factor) * 2.0, Default::default());
        }
    }

    fn resolve_collisions(&mut self) {
        let space = Space::read();
        let space = space.read().unwrap();

        for object_item in space.iter() {
            if let SpaceObjectId::Item(item_id) = object_item.id {
                if let Some(item) = self.items.get(&item_id) {
                    for object in space.collisions(object_item, true) {
                        match object.id {
                            SpaceObjectId::Player => {
                                self.player.state.write().unwrap().consume_item(item);
                                Events::write(Event::KillItem { id: item_id });
                                Events::write(Event::PlaySound("collect".into()));
                            }
                            SpaceObjectId::Enemy(enemy_id) => {
                                if let Some(enemy) = self.enemies.get_mut(&enemy_id) {
                                    enemy.state.write().unwrap().consume_item(item);
                                    Events::write(Event::KillItem { id: item_id });
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
