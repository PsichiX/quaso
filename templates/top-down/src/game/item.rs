use quaso::{
    context::GameContext,
    game::GameObject,
    third_party::{
        rand::random,
        spitfire_draw::{
            sprite::{Sprite, SpriteTexture},
            utils::{Drawable, TextureRef},
        },
        spitfire_glow::renderer::GlowTextureFiltering,
        vek::Vec2,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Apple,
    Orange,
    Banana,
}

impl ItemKind {
    pub fn random() -> Self {
        let index = random::<u64>() as usize % 3;
        match index {
            0 => Self::Apple,
            1 => Self::Orange,
            2 => Self::Banana,
            _ => unreachable!(),
        }
    }

    pub fn texture(self) -> &'static str {
        match self {
            Self::Apple => "item/apple",
            Self::Orange => "item/orange",
            Self::Banana => "item/banana",
        }
    }

    pub fn health(self) -> usize {
        match self {
            Self::Apple => 10,
            Self::Orange => 0,
            Self::Banana => 5,
        }
    }

    pub fn attack(self) -> usize {
        match self {
            Self::Apple => 0,
            Self::Orange => 10,
            Self::Banana => 5,
        }
    }
}

pub struct Item {
    pub sprite: Sprite,
    pub health: usize,
    pub attack: usize,
}

impl Item {
    pub fn new(kind: ItemKind, position: impl Into<Vec2<f32>>) -> Self {
        Self {
            sprite: Sprite::single(SpriteTexture {
                sampler: "u_image".into(),
                texture: TextureRef::name(kind.texture()),
                filtering: GlowTextureFiltering::Linear,
            })
            .position(position.into()),
            health: kind.health(),
            attack: kind.attack(),
        }
    }
}

impl GameObject for Item {
    fn draw(&mut self, context: &mut GameContext) {
        self.sprite.draw(context.draw, context.graphics);
    }
}
