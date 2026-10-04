use spitfire_draw::{sprite::Sprite, utils::TextureRef};
use std::{
    collections::{BTreeMap, HashSet},
    ops::Range,
};
use vek::Rect;

#[derive(Debug, Clone, PartialEq)]
pub struct FrameAnimation {
    /// [(image index, frame duration)]
    frames: Vec<(usize, f32)>,
    /// (frame index, accumulator)?
    current: Option<(usize, f32)>,
    /// {frame index: [event ids]}
    events: BTreeMap<usize, HashSet<String>>,
    pub speed: f32,
    pub is_playing: bool,
    pub looping: bool,
}

impl Default for FrameAnimation {
    fn default() -> Self {
        Self {
            frames: Vec::new(),
            current: None,
            events: Default::default(),
            speed: 30.0,
            is_playing: false,
            looping: false,
        }
    }
}

impl FrameAnimation {
    pub fn new(images: Range<usize>) -> Self {
        let mut result = Self::default();
        for index in images {
            result.frames.push((index, 1.0));
        }
        result
    }

    pub fn add_frame(&mut self, image_index: usize, duration: f32) {
        self.frames.push((image_index, duration));
    }

    pub fn frame(mut self, image_index: usize, duration: f32) -> Self {
        self.add_frame(image_index, duration);
        self
    }

    pub fn add_event(&mut self, frame: usize, id: impl ToString) {
        self.events.entry(frame).or_default().insert(id.to_string());
    }

    pub fn event(mut self, frame: usize, id: impl ToString) -> Self {
        self.add_event(frame, id);
        self
    }

    pub fn speed(mut self, value: f32) -> Self {
        self.speed = value;
        self
    }

    pub fn playing(mut self) -> Self {
        self.play();
        self
    }

    pub fn looping(mut self) -> Self {
        self.looping = true;
        self
    }

    pub fn play(&mut self) {
        if self.frames.is_empty() {
            return;
        }
        self.is_playing = true;
        self.current = Some((0, 0.0));
    }

    pub fn stop(&mut self) {
        self.is_playing = false;
        self.current = None;
    }

    pub fn update(&mut self, delta_time: f32) -> HashSet<&str> {
        if self.frames.is_empty() || !self.is_playing {
            return Default::default();
        }
        let Some((mut current_index, mut accumulator)) = self.current else {
            return Default::default();
        };
        let mut result = HashSet::default();
        accumulator += (delta_time * self.speed).max(0.0);
        while accumulator >= self.frames[current_index].1 {
            accumulator -= self.frames[current_index].1;
            if let Some(events) = self.events.get(&current_index) {
                result.extend(events.iter().map(|id| id.as_str()));
            }
            current_index += 1;
            if current_index >= self.frames.len() {
                if self.looping {
                    self.current = Some((0, accumulator));
                    current_index = 0;
                } else {
                    self.is_playing = false;
                    self.current = None;
                    return result;
                }
            }
        }
        self.current = Some((current_index, accumulator));
        result
    }

    pub fn current_image(&self) -> Option<usize> {
        self.current
            .and_then(|(index, _)| self.frames.get(index))
            .map(|(image_index, _)| *image_index)
    }
}

#[derive(Debug, Clone)]
pub struct NamedFrameAnimation {
    pub animation: FrameAnimation,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct SpriteAnimationImage {
    pub texture: TextureRef,
    pub region: Rect<f32, f32>,
    pub page: f32,
}

#[derive(Debug, Default, Clone)]
pub struct SpriteFrameAnimation {
    pub animation: FrameAnimation,
    pub images: BTreeMap<usize, SpriteAnimationImage>,
}

impl SpriteFrameAnimation {
    pub fn event(mut self, frame: usize, id: impl ToString) -> Self {
        self.animation = self.animation.event(frame, id);
        self
    }

    pub fn speed(mut self, value: f32) -> Self {
        self.animation = self.animation.speed(value);
        self
    }

    pub fn playing(mut self) -> Self {
        self.animation = self.animation.playing();
        self
    }

    pub fn looping(mut self) -> Self {
        self.animation = self.animation.looping();
        self
    }

    pub fn current_image(&self) -> Option<&SpriteAnimationImage> {
        self.images.get(&self.animation.current_image()?)
    }

    pub fn apply_to_sprite(&self, sprite: &mut Sprite, texture_index: usize) {
        if let Some(image) = self.current_image()
            && let Some(texture) = sprite.textures.get_mut(texture_index)
        {
            texture.texture = image.texture.clone();
            sprite.region = image.region;
            sprite.page = image.page;
        }
    }
}
