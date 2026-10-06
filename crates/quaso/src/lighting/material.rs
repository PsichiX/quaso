use spitfire_draw::{
    context::DrawContext,
    nine_slice_sprite::NineSliceSprite,
    particles::ParticleEmitter,
    primitives::PrimitivesEmitter,
    sprite::{Sprite, SpriteTexture},
    text::Text,
    tiles::TileSet,
    utils::{Drawable, ShaderRef, TextureRef, Vertex},
};
use spitfire_glow::{
    graphics::GraphicsTarget,
    renderer::{GlowBlending, GlowTextureFiltering, GlowUniformValue},
};
use std::{borrow::Cow, collections::HashMap, ops::DerefMut};
use vek::{Rgb, Vec2};

pub const GBUFFER_SHADER: &str = "lighting/gbuffer";
pub const GBUFFER_TEXT_SHADER: &str = "lighting/gbuffer-text";
pub const FORWARD_SHADER: &str = "lighting/forward";
pub const LIGHT_MAP_TEXTURE: &str = "lighting/light-map";
pub const WATER_SHADER: &str = "lighting/water";
pub const SCENE_COLOR_TEXTURE: &str = "lighting/scene-color";
pub const SHADOW_TEXTURE: &str = "lighting/shadow";
pub const EMISSION_TEXTURE: &str = "lighting/emission";
pub const GBUFFER_NORMAL_TEXTURE: &str = "lighting/gbuffer-normal";
pub const SHADOW_SHADER: &str = "lighting/shadow";

const MAP_NORMAL: i32 = 1;
const MAP_EMISSION: i32 = 2;
const MAP_NO_ALBEDO: i32 = 4;
const MAP_ABSORPTION: i32 = 8;
const MAP_VERTEX_EMISSION: i32 = 16;

#[derive(Debug, Clone)]
pub struct LightMaterial {
    pub emission: Rgb<f32>,
    pub absorption: Rgb<f32>,
    pub normal_map: Option<SpriteTexture>,
    pub emission_map: Option<SpriteTexture>,
    pub absorption_map: Option<SpriteTexture>,
    pub surface: SurfaceLighting,
    /// Water draws only over submerged surfaces, so objects above the water stay visible.
    pub submerged: bool,
    /// The vertex color RGB scales the emission and does not tint the albedo.
    /// A particle emitter then gives each particle its own glow and still draws all of them
    /// in one batch.
    pub vertex_emission: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceLighting {
    Bevel,
    #[default]
    Smooth,
}

impl Default for LightMaterial {
    fn default() -> Self {
        Self {
            emission: Rgb::zero(),
            absorption: Rgb::zero(),
            normal_map: None,
            emission_map: None,
            absorption_map: None,
            surface: SurfaceLighting::default(),
            submerged: false,
            vertex_emission: false,
        }
    }
}

impl LightMaterial {
    pub const OPAQUE_ABSORPTION: f32 = 1000.0;

    pub fn opaque() -> Self {
        Self::default().absorption(Rgb::broadcast(Self::OPAQUE_ABSORPTION))
    }

    pub fn emitter(color: Rgb<f32>, intensity: f32) -> Self {
        Self::opaque().emission(color, intensity)
    }

    pub fn filter(tint: Rgb<f32>, thickness: f32) -> Self {
        Self::default().absorption(Self::absorption_from_tint(tint, thickness))
    }

    pub fn absorption_from_tint(tint: Rgb<f32>, thickness: f32) -> Rgb<f32> {
        let thickness = thickness.max(f32::EPSILON);
        tint.map(|channel| -channel.clamp(1.0e-6, 1.0).ln() / thickness)
    }

    pub fn emission(mut self, color: Rgb<f32>, intensity: f32) -> Self {
        self.emission = color * intensity;
        self
    }

    pub fn absorption(mut self, value: Rgb<f32>) -> Self {
        self.absorption = value;
        self
    }

    pub fn normal_map(mut self, texture: SpriteTexture) -> Self {
        self.normal_map = Some(texture);
        self
    }

    pub fn emission_map(mut self, texture: SpriteTexture) -> Self {
        self.emission_map = Some(texture);
        self
    }

    pub fn absorption_map(mut self, texture: SpriteTexture) -> Self {
        self.absorption_map = Some(texture);
        self
    }

    pub fn surface(mut self, surface: SurfaceLighting) -> Self {
        self.surface = surface;
        self
    }

    pub fn submerged(mut self) -> Self {
        self.submerged = true;
        self
    }

    pub fn vertex_emission(mut self) -> Self {
        self.vertex_emission = true;
        self
    }

    /// The normal target blue channel keeps two flags: smooth surface (2) and submerged (1).
    /// The value is divided by 4, so it survives the 8 bit target and the coverage division.
    fn surface_mode(&self) -> f32 {
        let smooth = if self.surface == SurfaceLighting::Smooth {
            2
        } else {
            0
        };
        (smooth + self.submerged as u8) as f32 / 4.0
    }

    fn apply<T: Extend<SpriteTexture> + DerefMut<Target = [SpriteTexture]>>(
        &self,
        shader_name: &'static str,
        mode: LightMode,
        shader: &mut Option<ShaderRef>,
        uniforms: &mut HashMap<Cow<'static, str>, GlowUniformValue>,
        blending: &mut Option<GlowBlending>,
        textures: Option<&mut T>,
    ) {
        let mut maps = if self.vertex_emission {
            MAP_VERTEX_EMISSION
        } else {
            0
        };
        if let Some(textures) = textures {
            match textures.first_mut() {
                Some(albedo) => albedo.sampler = "u_image".into(),
                None => maps |= MAP_NO_ALBEDO,
            }
            if let Some(texture) = &self.normal_map
                && mode == LightMode::Deferred
            {
                textures.extend([SpriteTexture {
                    sampler: "u_normal_map".into(),
                    ..texture.clone()
                }]);
                maps |= MAP_NORMAL;
            }
            if let Some(texture) = &self.emission_map {
                textures.extend([SpriteTexture {
                    sampler: "u_emission_map".into(),
                    ..texture.clone()
                }]);
                maps |= MAP_EMISSION;
            }
            if let Some(texture) = &self.absorption_map
                && mode == LightMode::Deferred
            {
                textures.extend([SpriteTexture {
                    sampler: "u_absorption_map".into(),
                    ..texture.clone()
                }]);
                maps |= MAP_ABSORPTION;
            }
            if mode == LightMode::Forward {
                textures.extend([SpriteTexture {
                    sampler: "u_light_map".into(),
                    texture: TextureRef::name(LIGHT_MAP_TEXTURE),
                    filtering: GlowTextureFiltering::Linear,
                }]);
            }
        }
        *shader = Some(ShaderRef::name(shader_name));
        *blending = Some(match mode {
            LightMode::Deferred => GlowBlending::PremultipliedAlpha,
            LightMode::Forward => GlowBlending::Alpha,
        });
        uniforms.insert("u_maps".into(), GlowUniformValue::I1(maps));
        uniforms.insert(
            "u_emission".into(),
            GlowUniformValue::F3(self.emission.into_array()),
        );
        uniforms.insert(
            "u_absorption".into(),
            GlowUniformValue::F3(self.absorption.into_array()),
        );
        uniforms.insert(
            "u_surface_mode".into(),
            GlowUniformValue::F1(self.surface_mode()),
        );
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LightMode {
    #[default]
    Deferred,
    Forward,
}

pub trait LitTarget {
    fn apply_light_material(&mut self, material: &LightMaterial, mode: LightMode);
}

macro_rules! impl_textured_lit_target {
    ($($type:ty),+) => {
        $(
            impl LitTarget for $type {
                fn apply_light_material(&mut self, material: &LightMaterial, mode: LightMode) {
                    let shader_name = match mode {
                        LightMode::Deferred => GBUFFER_SHADER,
                        LightMode::Forward => FORWARD_SHADER,
                    };
                    material.apply(
                        shader_name,
                        mode,
                        &mut self.shader,
                        &mut self.uniforms,
                        &mut self.blending,
                        Some(&mut self.textures),
                    );
                }
            }
        )+
    };
}

impl_textured_lit_target!(
    Sprite,
    NineSliceSprite,
    PrimitivesEmitter,
    ParticleEmitter,
    TileSet
);

impl LitTarget for Text {
    fn apply_light_material(&mut self, material: &LightMaterial, mode: LightMode) {
        if mode == LightMode::Forward {
            return;
        }
        material.apply(
            GBUFFER_TEXT_SHADER,
            mode,
            &mut self.shader,
            &mut self.uniforms,
            &mut self.blending,
            None::<&mut Vec<SpriteTexture>>,
        );
    }
}

#[derive(Debug, Clone)]
pub struct Lit<T> {
    object: T,
    material: LightMaterial,
    mode: LightMode,
}

impl<T: LitTarget> Lit<T> {
    pub fn new(object: T, material: LightMaterial) -> Self {
        Self::with_mode(object, material, LightMode::Deferred)
    }

    pub fn forward(object: T, material: LightMaterial) -> Self {
        Self::with_mode(object, material, LightMode::Forward)
    }

    pub fn with_mode(mut object: T, material: LightMaterial, mode: LightMode) -> Self {
        object.apply_light_material(&material, mode);
        Self {
            object,
            material,
            mode,
        }
    }

    pub fn object(&self) -> &T {
        &self.object
    }

    pub fn object_mut(&mut self) -> &mut T {
        &mut self.object
    }

    pub fn material(&self) -> &LightMaterial {
        &self.material
    }

    pub fn mode(&self) -> LightMode {
        self.mode
    }

    pub fn into_inner(self) -> T {
        self.object
    }
}

impl<T: Drawable> Drawable for Lit<T> {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
        self.object.draw(context, graphics);
    }
}

/// Water reads the lit scene, so it draws after `Lighting::resolve`, before other forward objects.
/// The albedo alpha is the relative depth: 0 at the shore, 1 at the full depth.
#[derive(Debug, Clone)]
pub struct WaterMaterial {
    /// The color that passes through the full depth.
    pub transmittance: Rgb<f32>,
    /// The color that the water body scatters back from the light.
    pub scatter: Rgb<f32>,
    /// The bed offset in world units at the full depth and the full ripple slope.
    pub refraction: f32,
    /// The reflected part when the camera looks straight down.
    pub reflectivity: f32,
    /// How far, in world units, the water mirrors the bank and the lights.
    pub reach: f32,
    pub roughness: f32,
    /// The strength of light streaks and local light glints.
    pub glints: f32,
    pub shininess: f32,
    /// The white foam strength along the shore, where the depth is low.
    pub foam: f32,
    /// The strength of the bright light lines on the bed.
    pub caustics: f32,
    /// The strength of the tiny sun flashes on the ripples.
    pub sparkles: f32,
    /// The world direction toward the reflected bank. Up on the screen is `(0, -1)`.
    pub direction: Vec2<f32>,
    /// The ripple count across the object UV.
    pub ripple_scale: Vec2<f32>,
    pub ripple_strength: f32,
    pub time: f32,
}

impl Default for WaterMaterial {
    fn default() -> Self {
        Self {
            transmittance: Rgb::new(0.3, 0.6, 0.65),
            scatter: Rgb::new(0.03, 0.12, 0.15),
            refraction: 10.0,
            reflectivity: 0.3,
            reach: 120.0,
            roughness: 1.0,
            glints: 1.0,
            shininess: 60.0,
            foam: 0.0,
            caustics: 0.6,
            sparkles: 30.0,
            direction: Vec2::new(0.0, -1.0),
            ripple_scale: Vec2::new(4.0, 4.0),
            ripple_strength: 1.0,
            time: 0.0,
        }
    }
}

impl WaterMaterial {
    fn apply<T: Extend<SpriteTexture> + DerefMut<Target = [SpriteTexture]>>(
        &self,
        shader: &mut Option<ShaderRef>,
        uniforms: &mut HashMap<Cow<'static, str>, GlowUniformValue>,
        blending: &mut Option<GlowBlending>,
        textures: &mut T,
    ) {
        let maps = match textures.first_mut() {
            Some(albedo) => {
                albedo.sampler = "u_image".into();
                0
            }
            None => MAP_NO_ALBEDO,
        };
        textures.extend(
            [
                (
                    "u_scene_color",
                    SCENE_COLOR_TEXTURE,
                    GlowTextureFiltering::LinearMipmap,
                ),
                (
                    "u_emission",
                    EMISSION_TEXTURE,
                    GlowTextureFiltering::LinearMipmap,
                ),
                (
                    "u_gbuffer_normal",
                    GBUFFER_NORMAL_TEXTURE,
                    GlowTextureFiltering::Nearest,
                ),
                (
                    "u_light_map",
                    LIGHT_MAP_TEXTURE,
                    GlowTextureFiltering::LinearMipmap,
                ),
                (
                    "u_shadow",
                    SHADOW_TEXTURE,
                    GlowTextureFiltering::LinearMipmap,
                ),
            ]
            .map(|(sampler, texture, filtering)| SpriteTexture {
                sampler: sampler.into(),
                texture: TextureRef::name(texture),
                filtering,
            }),
        );
        *shader = Some(ShaderRef::name(WATER_SHADER));
        *blending = Some(GlowBlending::Alpha);
        let direction = Vec2::new(self.direction.x, -self.direction.y)
            .try_normalized()
            .unwrap_or_default();
        uniforms.extend(
            [
                ("u_maps", GlowUniformValue::I1(maps)),
                (
                    "u_transmittance",
                    GlowUniformValue::F3(self.transmittance.into_array()),
                ),
                ("u_scatter", GlowUniformValue::F3(self.scatter.into_array())),
                ("u_refraction", GlowUniformValue::F1(self.refraction)),
                ("u_reflectivity", GlowUniformValue::F1(self.reflectivity)),
                ("u_reach", GlowUniformValue::F1(self.reach)),
                ("u_roughness", GlowUniformValue::F1(self.roughness)),
                ("u_glints", GlowUniformValue::F1(self.glints)),
                ("u_shininess", GlowUniformValue::F1(self.shininess)),
                ("u_foam", GlowUniformValue::F1(self.foam)),
                ("u_caustics", GlowUniformValue::F1(self.caustics)),
                ("u_sparkles", GlowUniformValue::F1(self.sparkles)),
                ("u_direction", GlowUniformValue::F2(direction.into_array())),
                (
                    "u_ripple_scale",
                    GlowUniformValue::F2(self.ripple_scale.into_array()),
                ),
                (
                    "u_ripple_strength",
                    GlowUniformValue::F1(self.ripple_strength),
                ),
                ("u_time", GlowUniformValue::F1(self.time)),
            ]
            .map(|(name, value)| (Cow::Borrowed(name), value)),
        );
    }
}

pub trait WaterTarget {
    fn apply_water_material(&mut self, material: &WaterMaterial);
}

macro_rules! impl_water_target {
    ($($type:ty),+) => {
        $(
            impl WaterTarget for $type {
                fn apply_water_material(&mut self, material: &WaterMaterial) {
                    material.apply(
                        &mut self.shader,
                        &mut self.uniforms,
                        &mut self.blending,
                        &mut self.textures,
                    );
                }
            }
        )+
    };
}

impl_water_target!(Sprite, NineSliceSprite, PrimitivesEmitter, TileSet);

#[derive(Debug, Clone)]
pub struct Water<T> {
    object: T,
    material: WaterMaterial,
}

impl<T: WaterTarget> Water<T> {
    pub fn new(mut object: T, material: WaterMaterial) -> Self {
        object.apply_water_material(&material);
        Self { object, material }
    }

    pub fn object(&self) -> &T {
        &self.object
    }

    pub fn material(&self) -> &WaterMaterial {
        &self.material
    }

    pub fn into_inner(self) -> T {
        self.object
    }
}

impl<T: Drawable> Drawable for Water<T> {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
        self.object.draw(context, graphics);
    }
}

/// A shadow caster draws between `Lighting::begin_shadows` and `Lighting::end_shadows`.
/// The silhouette moves away from the sun by `height * LightingConfig::shadow_length`.
/// The ground under the silhouette loses the sun and a part of the sky light, but keeps the
/// light of local lights, so an object above a torch does not block the torch.
/// `softness` is the blur radius of the shadow edge in world units. Leaves and cloth cast soft
/// shadows, poles and walls cast hard shadows.
/// An eraser removes the shadows that are already in the mask under its own shape.
/// Draw a low object first as a caster, then as an eraser, and then draw the higher casters.
/// The low object then shadows the ground around it but not its own top.
/// With `vertex_height`, the red channel of the vertex color scales `height`. A particle
/// emitter then gives each particle its own height and still draws all of them in one batch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowMaterial {
    pub height: f32,
    pub opacity: f32,
    pub softness: f32,
    pub erase: bool,
    pub vertex_height: bool,
}

impl Default for ShadowMaterial {
    fn default() -> Self {
        Self {
            height: 1.0,
            opacity: 1.0,
            softness: 0.0,
            erase: false,
            vertex_height: false,
        }
    }
}

impl ShadowMaterial {
    pub fn new(height: f32) -> Self {
        Self {
            height,
            ..Default::default()
        }
    }

    pub fn eraser() -> Self {
        Self {
            height: 0.0,
            erase: true,
            ..Default::default()
        }
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    pub fn softness(mut self, softness: f32) -> Self {
        self.softness = softness;
        self
    }

    pub fn vertex_height(mut self) -> Self {
        self.vertex_height = true;
        self
    }

    fn apply<T: Extend<SpriteTexture> + DerefMut<Target = [SpriteTexture]>>(
        &self,
        shader: &mut Option<ShaderRef>,
        uniforms: &mut HashMap<Cow<'static, str>, GlowUniformValue>,
        blending: &mut Option<GlowBlending>,
        textures: &mut T,
    ) {
        let maps = match textures.first_mut() {
            Some(albedo) => {
                albedo.sampler = "u_image".into();
                0
            }
            None => MAP_NO_ALBEDO,
        };
        *shader = Some(ShaderRef::name(SHADOW_SHADER));
        *blending = Some(if self.erase {
            GlowBlending::Multiply
        } else {
            GlowBlending::Alpha
        });
        uniforms.extend(
            [
                ("u_maps", GlowUniformValue::I1(maps)),
                ("u_height", GlowUniformValue::F1(self.height)),
                ("u_opacity", GlowUniformValue::F1(self.opacity)),
                ("u_softness", GlowUniformValue::F1(self.softness)),
                ("u_erase", GlowUniformValue::I1(self.erase as i32)),
                (
                    "u_vertex_height",
                    GlowUniformValue::I1(self.vertex_height as i32),
                ),
            ]
            .map(|(name, value)| (Cow::Borrowed(name), value)),
        );
    }
}

pub trait ShadowTarget {
    fn apply_shadow_material(&mut self, material: &ShadowMaterial);
}

macro_rules! impl_shadow_target {
    ($($type:ty),+) => {
        $(
            impl ShadowTarget for $type {
                fn apply_shadow_material(&mut self, material: &ShadowMaterial) {
                    material.apply(
                        &mut self.shader,
                        &mut self.uniforms,
                        &mut self.blending,
                        &mut self.textures,
                    );
                }
            }
        )+
    };
}

impl_shadow_target!(
    Sprite,
    NineSliceSprite,
    PrimitivesEmitter,
    ParticleEmitter,
    TileSet
);

#[derive(Debug, Clone)]
pub struct Shadow<T> {
    object: T,
    material: ShadowMaterial,
}

impl<T: ShadowTarget> Shadow<T> {
    pub fn new(mut object: T, material: ShadowMaterial) -> Self {
        object.apply_shadow_material(&material);
        Self { object, material }
    }

    pub fn object(&self) -> &T {
        &self.object
    }

    pub fn material(&self) -> &ShadowMaterial {
        &self.material
    }

    pub fn into_inner(self) -> T {
        self.object
    }
}

impl<T: Drawable> Drawable for Shadow<T> {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
        self.object.draw(context, graphics);
    }
}
