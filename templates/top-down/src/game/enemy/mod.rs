pub mod ai;
pub mod conditions;
pub mod tasks;

use crate::game::{
    enemy::{
        ai::{pursue::EnemyAiPursueTask, wait::EnemyAiWaitTask, wander::EnemyAiWanderTask},
        conditions::{
            EnemyHasPlayerInRangeCondition, EnemyIsAttackingCondition, EnemyIsMovingCondition,
            EnemyIsNotInCooldownCondition,
        },
        tasks::{attack::EnemyAttackTask, idle::EnemyIdleTask, run::EnemyRunTask},
    },
    item::Item,
    player::PlayerState,
    utils::events::{Event, Events, Instigator},
};
use quaso::{
    animation::frame::NamedAnimation,
    character::{Character, CharacterController},
    context::GameContext,
    game::GameObject,
    third_party::{
        emergent::{builders::behavior_tree::BehaviorTree, combinators::all::CombinatorAll},
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, ShaderRef, TextureRef},
        },
        spitfire_glow::renderer::GlowUniformValue,
        typid::ID,
        vek::{Vec2, Vec3},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnemyAiBehavior {
    Wait,
    Wander,
    Pursue,
}

#[derive(Debug, Default)]
pub struct EnemyAiState {
    pub direction: Vec2<f32>,
    pub attack: bool,
    pub cooldown_seconds: f32,
    pub target_in_range_distance_threshold: f32,
    pub target_in_range_position: Option<Vec2<f32>>,
    pub target_offset_x: f32,
}

pub struct EnemyState {
    pub sprite: Sprite,
    pub ai: EnemyAiState,
    pub health: usize,
    pub attack: usize,
    pub attack_range: f32,
    pub blink_seconds: f32,
}

impl Default for EnemyState {
    fn default() -> Self {
        Self {
            sprite: Sprite::single(SpriteTexture::new(
                "u_image".into(),
                TextureRef::name("enemy/idle/1"),
            ))
            .shader(ShaderRef::name("character"))
            .pivot([0.25, 0.5].into())
            .uniform(
                "u_fill_color".into(),
                GlowUniformValue::F4([0.0, 0.0, 0.0, 0.0]),
            ),
            ai: Default::default(),
            health: 100,
            attack: 20,
            attack_range: 60.0,
            blink_seconds: 0.0,
        }
    }
}

impl GameObject for EnemyState {
    fn process(&mut self, _: &mut GameContext, delta_time: f32) {
        self.ai.cooldown_seconds = (self.ai.cooldown_seconds - delta_time).max(0.0);
        self.blink_seconds = (self.blink_seconds - delta_time).max(0.0);
    }

    fn draw(&mut self, context: &mut GameContext) {
        if self.blink_seconds > 0.0 {
            self.sprite.uniforms.insert(
                "u_fill_color".into(),
                GlowUniformValue::F4([1.0, 1.0, 1.0, 1.0]),
            );
        } else {
            self.sprite.uniforms.insert(
                "u_fill_color".into(),
                GlowUniformValue::F4([1.0, 1.0, 1.0, 0.0]),
            );
        }
        self.sprite.draw(context.draw, context.graphics);
    }
}

impl EnemyState {
    pub fn new_character(position: impl Into<Vec3<f32>>) -> Character<EnemyState> {
        let mut state = EnemyState::default();
        state.ai.target_in_range_distance_threshold = 100.0;
        state.ai.target_offset_x = 20.0;
        state.sprite.transform.position = position.into();

        let task = BehaviorTree::selector(true)
            .node(BehaviorTree::state(
                EnemyIsAttackingCondition,
                EnemyAttackTask::default(),
            ))
            .node(BehaviorTree::state(
                EnemyIsMovingCondition,
                EnemyRunTask::default(),
            ))
            .node(BehaviorTree::state(true, EnemyIdleTask::default()))
            .build();

        let ai = BehaviorTree::selector(true)
            .node(BehaviorTree::state(
                CombinatorAll::default()
                    .condition(EnemyIsNotInCooldownCondition)
                    .condition(EnemyHasPlayerInRangeCondition),
                EnemyAiPursueTask::default(),
            ))
            .node(BehaviorTree::state(
                EnemyIsNotInCooldownCondition,
                EnemyAiWanderTask::default(),
            ))
            .node(BehaviorTree::state(true, EnemyAiWaitTask))
            .build();

        let controller = CharacterController::ai(ai);
        Character::new(state, task, controller)
    }

    pub fn apply_animation(&mut self, animation: &NamedAnimation) {
        if let Some(frame) = animation.animation.current_frame() {
            self.sprite.textures[0].texture =
                TextureRef::name(format!("{}/{}", animation.id, frame));
        }
    }

    pub fn sense_player(&mut self, player: &PlayerState) {
        let direction = player.sprite.transform.position.xy() - self.sprite.transform.position.xy();

        if direction.magnitude() < self.ai.target_in_range_distance_threshold {
            self.ai.target_in_range_position = Some(player.sprite.transform.position.xy());
        } else {
            self.ai.target_in_range_position = None;
        }
    }

    pub fn execute_events(&mut self, id: ID<EnemyState>, events: &[Event]) {
        for event in events {
            if let Event::Attack {
                position,
                range,
                value,
                instigator,
            } = event
                && *instigator == Instigator::Player
            {
                let distance = position.distance(self.sprite.transform.position.xy());
                if distance <= *range {
                    self.blink_seconds = 0.15;
                    self.health = self.health.saturating_sub(*value);
                    if self.health == 0 {
                        Events::write(Event::KillEnemy { id });
                    }
                }
            }
        }
    }

    pub fn consume_item(&mut self, item: &Item) {
        self.health += item.health;
        self.attack += item.attack;
    }
}
