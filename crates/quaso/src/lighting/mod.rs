pub mod material;
mod shaders;

use self::material::{
    EMISSION_TEXTURE, FORWARD_SHADER, GBUFFER_NORMAL_TEXTURE, GBUFFER_SHADER, GBUFFER_TEXT_SHADER,
    LIGHT_MAP_TEXTURE, SCENE_COLOR_TEXTURE, SHADOW_SHADER, SHADOW_TEXTURE, WATER_SHADER,
};
use crate::third_party::time::Instant;
use spitfire_draw::{
    canvas::Canvas,
    context::DrawContext,
    sprite::{Sprite, SpriteTexture},
    text::Text,
    utils::{Drawable, ShaderRef, Vertex},
};
use spitfire_glow::{
    graphics::{CameraScaling, Graphics, Shader},
    renderer::{GlowBlending, GlowTextureFiltering, GlowTextureFormat, GlowUniformValue},
};
use std::{borrow::Cow, collections::HashMap};
use vek::{Rgb, Rgba, Vec2, Vec4};

const LUMINANCE_SIZE: u32 = 128;

const GBUFFER_ALBEDO: usize = 0;
const GBUFFER_EMISSION: usize = 1;
const GBUFFER_ABSORPTION: usize = 2;
const GBUFFER_NORMAL: usize = 3;
const GBUFFER_FORMATS: [GlowTextureFormat; 4] = [
    GlowTextureFormat::Rgba,
    GlowTextureFormat::Data16,
    GlowTextureFormat::Data16,
    GlowTextureFormat::Rgba,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightingConfig {
    pub resolution_scale: f32,
    pub base_interval: f32,
    pub ambient: Rgb<f32>,
    pub sky: Rgb<f32>,
    pub sun: Rgb<f32>,
    pub sun_direction: Vec2<f32>,
    pub sun_sharpness: f32,
    pub light_height: f32,
    pub exposure: f32,
    pub margin: f32,
    pub probe_spacing: u32,
    pub eye_adaptation: Option<EyeAdaptation>,
    pub bloom: Option<Bloom>,
    /// The world distance that a shadow moves away from the sun for each unit of caster height.
    /// A low sun makes long shadows, so this value grows near dawn and dusk.
    pub shadow_length: f32,
    /// The part of the sky light that a shadow caster blocks.
    pub shadow_sky: f32,
    /// The blur radius of every shadow edge, in shadow mask pixels.
    /// Each caster adds its own `ShadowMaterial::softness` on top.
    pub shadow_softness: f32,
    /// The weight of the previous frame in the nearest cascade, from 0 to 1.
    /// It hides the probe flicker of moving lights, but a fast light trails a bit.
    pub temporal_smoothing: f32,
}

/// The exposure becomes `key / average luminance`, clamped to the exposure range.
/// The clamp keeps a night dark and stops a bright noon from going dull.
/// With a `focus` point in world space, the average weights the screen by a Gaussian
/// of `focus_radius` around that point, so a static camera still adapts to the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyeAdaptation {
    pub key: f32,
    pub speed: f32,
    pub min_exposure: f32,
    pub max_exposure: f32,
    pub focus: Option<Vec2<f32>>,
    pub focus_radius: f32,
}

impl Default for EyeAdaptation {
    fn default() -> Self {
        Self {
            key: 0.4,
            speed: 1.5,
            min_exposure: 0.25,
            max_exposure: 2.0,
            focus: None,
            focus_radius: 150.0,
        }
    }
}

/// Bloom blurs the HDR image through a chain of half size levels and mixes the blur into it.
/// The mix takes `strength` of the image away and gives the same amount of blur back,
/// so bloom moves light around but does not add light.
/// More `levels` make a wider glow.
/// `spread` sets how the glow splits between the levels, from 0 to 0.95. Each level keeps
/// `1 - spread` of its own blur and takes `spread` from the next, larger level.
/// A high spread gives most of the weight to the large levels, so the halo is wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bloom {
    pub strength: f32,
    pub levels: usize,
    pub spread: f32,
}

impl Default for Bloom {
    fn default() -> Self {
        Self {
            strength: 0.06,
            levels: 6,
            spread: 0.75,
        }
    }
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self {
            resolution_scale: 0.5,
            base_interval: 1.0,
            ambient: Rgb::zero(),
            sky: Rgb::zero(),
            sun: Rgb::zero(),
            sun_direction: Vec2::new(0.0, -1.0),
            sun_sharpness: 8.0,
            light_height: 32.0,
            exposure: 1.0,
            margin: 0.1,
            probe_spacing: 2,
            eye_adaptation: Some(EyeAdaptation::default()),
            bloom: Some(Bloom::default()),
            shadow_length: 1.0,
            shadow_sky: 0.0,
            shadow_softness: 1.5,
            temporal_smoothing: 0.75,
        }
    }
}

/// A style changes how the lighting looks with GLSL snippets, for example cel shading.
/// Each snippet is put into a fixed shader, so it can declare its own uniforms and helper
/// functions. `Lighting::uniforms` sets the values of those uniforms.
///
/// - `shade` defines `vec3 shade(Surface surface)`. It returns the light that the surface
///   reflects, without emission. The default is `surface.albedo * surface.light`.
///   The composite and the forward pass call it. `Surface` has `albedo` (not premultiplied),
///   `normal`, `light` (with shadow and ambient), `ambient`, `shadow` (0 is open, 1 is full
///   shadow), `scene_uv` and `screen_uv`. Forward sprites get no shadow and a flat normal.
/// - `grade` defines `vec3 grade(vec3 color, vec2 screen_uv)`. It maps the exposed HDR
///   color, with bloom, to the output color. The default is the ACES curve, `aces(color)`.
/// - Each of `passes` defines `vec4 pass_color(vec2 screen_uv, vec2 scene_uv)`. A pass runs
///   on the HDR image after the forward pass, before eye adaptation and bloom. It can read
///   `source_color(screen_uv)`, `scene_albedo(scene_uv)`, `scene_normal(scene_uv)`,
///   `scene_emission(scene_uv)`, `scene_opacity(scene_uv)`, `scene_light(scene_uv)` and
///   `shadow_mask(scene_uv)`. `scene_pixel()` is the size of one screen pixel in scene UV.
///   The passes run in order, and each one reads the result of the one before.
/// - `water` defines `vec4 water_color(WaterSurface water)`. It returns the color and the
///   coverage of a water pixel. `WaterSurface` has the parts that the default mixes:
///   `bed` (the refracted scene with caustics), `body` (the bed through the water),
///   `reflection`, `glint`, `halo`, `light`, `normal` (of the ripples), `fresnel`, `depth`,
///   `foam`, `shadow`, `shadow_dim`, `shoreline`, `ripple_uv`, `scene_uv` and `screen_uv`.
///   The snippet can also call `ripple_height(position)` and `gradient_noise(position)`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LightingStyle {
    pub shade: Option<Cow<'static, str>>,
    pub grade: Option<Cow<'static, str>>,
    pub water: Option<Cow<'static, str>>,
    pub passes: Vec<Cow<'static, str>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LightingDebugView {
    #[default]
    None,
    Albedo,
    Emission,
    Absorption,
    Normal,
    LightMap,
    /// The projected height shadows, white where the sun is open.
    Shadows,
    /// The light that reaches each pixel, without the albedo.
    Lighting,
    /// The distance to the nearest occluder, in bands of 8 scene texels.
    Distance,
    /// The water normal in red and green, the water depth in blue.
    Water,
    /// The bloom blur alone, before the mix with the image.
    Bloom,
    /// The final image in the middle, with the other views in tiles above and below it.
    Overview,
}

impl LightingDebugView {
    /// The overview tiles from the top left, in reading order.
    const OVERVIEW_TILES: [Self; 9] = [
        Self::Albedo,
        Self::Normal,
        Self::Emission,
        Self::Absorption,
        Self::LightMap,
        Self::Lighting,
        Self::Shadows,
        Self::Distance,
        Self::Bloom,
    ];

    fn index(self) -> i32 {
        match self {
            Self::None | Self::Overview | Self::Bloom => 0,
            Self::Albedo => 1,
            Self::Emission => 2,
            Self::Absorption => 3,
            Self::Normal => 4,
            Self::LightMap => 5,
            Self::Shadows => 6,
            Self::Lighting => 7,
            Self::Distance => 8,
            Self::Water => 9,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Final",
            Self::Albedo => "Albedo",
            Self::Emission => "Emission",
            Self::Absorption => "Absorption",
            Self::Normal => "Normal",
            Self::LightMap => "Light map",
            Self::Shadows => "Shadows",
            Self::Lighting => "Lighting",
            Self::Distance => "Distance",
            Self::Water => "Water",
            Self::Bloom => "Bloom",
            Self::Overview => "Overview",
        }
    }

    fn tonemapped(self) -> bool {
        matches!(
            self,
            Self::None | Self::Lighting | Self::Overview | Self::Bloom
        )
    }
}

struct LightingShaders {
    medium: Shader,
    seed: Shader,
    jump_flood: Shader,
    cascade: Shader,
    light_map: Shader,
    composite: Shader,
    temporal: Shader,
    tonemap: Shader,
    luminance: Shader,
    adaptation: Shader,
    forward: Shader,
    water: Shader,
    shadow: Shader,
    copy: Shader,
    text: Shader,
    passes: Vec<Shader>,
    bloom_down: Shader,
    bloom_up: Shader,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Stage {
    #[default]
    Idle,
    Scene,
    Shadows,
    Forward,
}

#[derive(Default)]
pub struct Lighting {
    pub config: LightingConfig,
    pub debug_view: LightingDebugView,
    /// The name of a loaded font for the overview tile labels. Without it the tiles have no labels.
    pub debug_font: Option<Cow<'static, str>>,
    /// The uniforms of the style snippets. The composite, forward, tonemap and pass shaders get them.
    pub uniforms: HashMap<Cow<'static, str>, GlowUniformValue>,
    style: LightingStyle,
    shaders: Option<LightingShaders>,
    gbuffer: Option<Canvas>,
    seeds: Option<[Canvas; 2]>,
    surfaces: Option<[Canvas; 2]>,
    cascades: Option<[Canvas; 2]>,
    history: Option<[Canvas; 2]>,
    history_index: usize,
    history_valid: bool,
    history_shift: Vec2<f32>,
    scene_origin: Option<Vec2<f32>>,
    light_map: Option<Canvas>,
    medium: Option<Canvas>,
    hdr: Option<Canvas>,
    scene_color: Option<Canvas>,
    overview: Option<Canvas>,
    bloom: Vec<Canvas>,
    shadows: Option<Canvas>,
    luminance: Option<Canvas>,
    adapted: Option<[Canvas; 2]>,
    adapted_index: usize,
    adapted_time: Option<Instant>,
    stage: Stage,
    scene_size: Vec2<u32>,
    cascade_size: Vec2<u32>,
    probe_spacing: u32,
    cascade_count: usize,
    texel_world: f32,
    view_rect: Vec4<f32>,
    scene_camera: (CameraScaling, Vec2<f32>),
    saved_camera: Option<(CameraScaling, Vec2<f32>)>,
}

impl Lighting {
    pub fn with_style(mut self, style: LightingStyle) -> Self {
        self.set_style(style);
        self
    }

    pub fn style(&self) -> &LightingStyle {
        &self.style
    }

    /// The shaders build again on the next `begin`. A snippet that does not compile logs an error,
    /// and the lighting draws nothing until the style is fixed.
    pub fn set_style(&mut self, style: LightingStyle) {
        if self.style != style {
            self.style = style;
            self.shaders = None;
        }
    }

    pub fn with_uniform(
        mut self,
        name: impl Into<Cow<'static, str>>,
        value: GlowUniformValue,
    ) -> Self {
        self.uniforms.insert(name.into(), value);
        self
    }

    pub fn with_debug_font(mut self, font: impl Into<Cow<'static, str>>) -> Self {
        self.debug_font = Some(font.into());
        self
    }

    pub fn new(config: LightingConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    pub fn cascade_count(&self) -> usize {
        self.cascade_count
    }

    pub fn scene_size(&self) -> Vec2<u32> {
        self.scene_size
    }

    pub fn light_map(&self, sampler: impl Into<Cow<'static, str>>) -> Option<SpriteTexture> {
        self.light_map
            .as_ref()?
            .sprite_texture(0, sampler.into(), GlowTextureFiltering::Linear)
    }

    pub fn begin(&mut self, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        if let Err(error) = self.prepare(draw, graphics) {
            tracing::error!("Lighting could not prepare its resources: {error}");
            return;
        }
        if let Some(shadows) = &self.shadows {
            shadows.with(draw, graphics, true, |_, _| {});
        }
        if let Some(gbuffer) = &self.gbuffer {
            gbuffer.activate(draw, graphics, true);
            let camera = &mut graphics.state.main_camera;
            self.saved_camera = Some((camera.scaling, camera.screen_alignment));
            (camera.scaling, camera.screen_alignment) = self.scene_camera;
            self.stage = Stage::Scene;
        }
    }

    /// Shadow casters draw between this call and `end_shadows`, inside the scene stage.
    pub fn begin_shadows(&mut self, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        if self.stage != Stage::Scene {
            return;
        }
        let (Some(shaders), Some(shadows)) = (&mut self.shaders, &self.shadows) else {
            return;
        };
        let offset = -self
            .config
            .sun_direction
            .try_normalized()
            .unwrap_or_default()
            * self.config.shadow_length;
        shaders
            .shadow
            .set_shared_uniform("u_shadow_offset", GlowUniformValue::F2(offset.into_array()));
        draw.shaders
            .insert(SHADOW_SHADER.into(), shaders.shadow.clone());
        Canvas::deactivate(draw, graphics);
        shadows.activate(draw, graphics, false);
        self.stage = Stage::Shadows;
    }

    pub fn end_shadows(&mut self, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        if self.stage != Stage::Shadows {
            return;
        }
        Canvas::deactivate(draw, graphics);
        if let Some(gbuffer) = &self.gbuffer {
            gbuffer.activate(draw, graphics, false);
        }
        self.stage = Stage::Scene;
    }

    pub fn resolve(&mut self, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        self.end_shadows(draw, graphics);
        if self.stage != Stage::Scene {
            return;
        }
        let (
            Some(shaders),
            Some(gbuffer),
            Some(seeds),
            Some(surfaces),
            Some(cascades),
            Some(light_map),
            Some(medium),
            Some(hdr),
            Some(scene_color),
            Some(shadows),
        ) = (
            &self.shaders,
            &self.gbuffer,
            &self.seeds,
            &self.surfaces,
            &self.cascades,
            &self.light_map,
            &self.medium,
            &self.hdr,
            &self.scene_color,
            &self.shadows,
        )
        else {
            return;
        };
        Canvas::deactivate(draw, graphics);
        if let Some((scaling, alignment)) = self.saved_camera.take() {
            let camera = &mut graphics.state.main_camera;
            camera.scaling = scaling;
            camera.screen_alignment = alignment;
        }
        self.stage = Stage::Forward;

        let scene_size = self.scene_size.as_::<f32>();
        let cascade_size = self.cascade_size.as_::<f32>();
        let target_size = Vec2::new(hdr.surface().width(), hdr.surface().height()).as_::<f32>();
        let gbuffer_texture = |index: usize, sampler: &'static str| {
            gbuffer.sprite_texture(index, sampler.into(), GlowTextureFiltering::Linear)
        };
        let probe_spacing = self.probe_spacing as f32;
        let gbuffer_scale = gbuffer.surface().width() as f32 / scene_size.x;

        let scene_sources = [
            gbuffer_texture(GBUFFER_EMISSION, "u_emission"),
            gbuffer_texture(GBUFFER_ABSORPTION, "u_absorption"),
        ];
        let seeds_texture = shaders
            .jump_flood(
                draw,
                graphics,
                seeds,
                scene_sources.iter().flatten().cloned(),
                self.scene_size,
                false,
            )
            .sprite_texture(0, "u_seeds".into(), GlowTextureFiltering::Nearest);
        let surface_texture = shaders
            .jump_flood(
                draw,
                graphics,
                surfaces,
                scene_sources.iter().flatten().cloned(),
                self.scene_size,
                true,
            )
            .sprite_texture(0, "u_surface".into(), GlowTextureFiltering::Nearest);

        medium.with(draw, graphics, false, |draw, graphics| {
            full_screen_pass(
                draw,
                graphics,
                &shaders.medium,
                Vec2::new(gbuffer.surface().width(), gbuffer.surface().height()).as_::<f32>(),
                scene_sources.iter().flatten().cloned(),
                [("u_texel_world", GlowUniformValue::F1(self.texel_world))],
                GlowBlending::None,
            );
        });
        for attachment in medium.surface().attachments() {
            attachment.texture.generate_mipmaps();
        }

        let sun = self.config.sun / sun_lobe_mean(self.config.sun_sharpness);
        let sun_direction = Vec2::new(self.config.sun_direction.x, -self.config.sun_direction.y)
            .try_normalized()
            .unwrap_or_default();
        let light_height = self.config.light_height.max(0.0) / self.texel_world;
        for cascade in (0..self.cascade_count).rev() {
            let target = &cascades[cascade % 2];
            let upper = &cascades[(cascade + 1) % 2];
            let textures = [
                medium.sprite_texture(0, "u_opacity".into(), GlowTextureFiltering::LinearMipmap),
                medium.sprite_texture(1, "u_emission".into(), GlowTextureFiltering::LinearMipmap),
                upper.sprite_texture(0, "u_upper".into(), GlowTextureFiltering::Linear),
                seeds_texture.clone(),
            ];
            target.with(draw, graphics, false, |draw, graphics| {
                full_screen_pass(
                    draw,
                    graphics,
                    &shaders.cascade,
                    cascade_size,
                    textures.iter().flatten().cloned(),
                    [
                        ("u_cascade", GlowUniformValue::I1(cascade as _)),
                        (
                            "u_cascade_count",
                            GlowUniformValue::I1(self.cascade_count as _),
                        ),
                        ("u_size", GlowUniformValue::F2(cascade_size.into_array())),
                        (
                            "u_scene_size",
                            GlowUniformValue::F2(scene_size.into_array()),
                        ),
                        (
                            "u_base_interval",
                            GlowUniformValue::F1(self.config.base_interval),
                        ),
                        ("u_probe_spacing", GlowUniformValue::F1(probe_spacing)),
                        ("u_gbuffer_scale", GlowUniformValue::F1(gbuffer_scale)),
                        ("u_sky", GlowUniformValue::F3(self.config.sky.into_array())),
                        ("u_light_height", GlowUniformValue::F1(light_height)),
                        ("u_sun", GlowUniformValue::F3(sun.into_array())),
                        (
                            "u_sun_direction",
                            GlowUniformValue::F2(sun_direction.into_array()),
                        ),
                        (
                            "u_sun_sharpness",
                            GlowUniformValue::F1(self.config.sun_sharpness),
                        ),
                    ],
                    GlowBlending::None,
                );
            });
        }

        let Some(history) = &self.history else {
            return;
        };
        let previous = &history[self.history_index];
        let nearest = &history[1 - self.history_index];
        let history_weight = if self.history_valid {
            self.config.temporal_smoothing.clamp(0.0, 0.99)
        } else {
            0.0
        };
        nearest.with(draw, graphics, false, |draw, graphics| {
            full_screen_pass(
                draw,
                graphics,
                &shaders.temporal,
                cascade_size,
                [
                    cascades[0].sprite_texture(
                        0,
                        "u_current".into(),
                        GlowTextureFiltering::Nearest,
                    ),
                    previous.sprite_texture(0, "u_previous".into(), GlowTextureFiltering::Nearest),
                ]
                .into_iter()
                .flatten(),
                [
                    ("u_size", GlowUniformValue::F2(cascade_size.into_array())),
                    (
                        "u_shift",
                        GlowUniformValue::F2(self.history_shift.into_array()),
                    ),
                    ("u_history", GlowUniformValue::F1(history_weight)),
                ],
                GlowBlending::None,
            );
        });
        self.history_index = 1 - self.history_index;
        self.history_valid = true;

        light_map.with(draw, graphics, false, |draw, graphics| {
            full_screen_pass(
                draw,
                graphics,
                &shaders.light_map,
                scene_size,
                [
                    nearest.sprite_texture(0, "u_cascade".into(), GlowTextureFiltering::Linear),
                    gbuffer_texture(GBUFFER_ABSORPTION, "u_absorption"),
                    surface_texture.clone(),
                ]
                .into_iter()
                .flatten(),
                [
                    ("u_size", GlowUniformValue::F2(cascade_size.into_array())),
                    (
                        "u_scene_size",
                        GlowUniformValue::F2(scene_size.into_array()),
                    ),
                    ("u_probe_spacing", GlowUniformValue::F1(probe_spacing)),
                    ("u_texel_world", GlowUniformValue::F1(self.texel_world)),
                ],
                GlowBlending::None,
            );
        });

        for attachment in light_map.surface().attachments() {
            attachment.texture.generate_mipmaps();
        }
        draw.textures.insert(
            LIGHT_MAP_TEXTURE.into(),
            light_map.surface().attachments()[0].texture.clone(),
        );
        draw.textures.insert(
            EMISSION_TEXTURE.into(),
            medium.surface().attachments()[1].texture.clone(),
        );
        draw.textures.insert(
            GBUFFER_NORMAL_TEXTURE.into(),
            gbuffer.surface().attachments()[GBUFFER_NORMAL]
                .texture
                .clone(),
        );
        draw.textures.insert(
            SHADOW_TEXTURE.into(),
            shadows.surface().attachments()[0].texture.clone(),
        );
        let shadow_light = self.config.sun + self.config.sky * self.config.shadow_sky;
        let shadow_open = self.config.sun + self.config.sky;
        let mut water = shaders.water.clone();
        for (name, value) in [
            (
                "u_shadow_light",
                GlowUniformValue::F3(shadow_light.into_array()),
            ),
            (
                "u_shadow_open",
                GlowUniformValue::F3(shadow_open.into_array()),
            ),
            (
                "u_shadow_softness",
                GlowUniformValue::F1(self.config.shadow_softness),
            ),
            (
                "u_target_size",
                GlowUniformValue::F2(target_size.into_array()),
            ),
            (
                "u_scene_size",
                GlowUniformValue::F2(scene_size.into_array()),
            ),
            (
                "u_view_rect",
                GlowUniformValue::F4(self.view_rect.into_array()),
            ),
            ("u_texel_world", GlowUniformValue::F1(self.texel_world)),
            ("u_sky", GlowUniformValue::F3(self.config.sky.into_array())),
            ("u_sun", GlowUniformValue::F3(self.config.sun.into_array())),
            (
                "u_sun_direction",
                GlowUniformValue::F2(sun_direction.into_array()),
            ),
            (
                "u_ambient",
                GlowUniformValue::F3(self.config.ambient.into_array()),
            ),
            ("u_debug", GlowUniformValue::I1(self.debug_view.index())),
        ] {
            water.set_shared_uniform(name, value);
        }
        for (name, value) in &self.uniforms {
            water.set_shared_uniform(name.clone(), *value);
        }
        let mut forward = shaders.forward.clone();
        forward.set_shared_uniform(
            "u_target_size",
            GlowUniformValue::F2(target_size.into_array()),
        );
        forward.set_shared_uniform("u_debug", GlowUniformValue::I1(self.debug_view.index()));
        for (name, value) in &self.uniforms {
            forward.set_shared_uniform(name.clone(), *value);
        }
        forward.set_shared_uniform(
            "u_view_rect",
            GlowUniformValue::F4(self.view_rect.into_array()),
        );
        forward.set_shared_uniform(
            "u_ambient",
            GlowUniformValue::F3(self.config.ambient.into_array()),
        );

        let textures = [
            gbuffer_texture(GBUFFER_ALBEDO, "u_albedo"),
            gbuffer_texture(GBUFFER_EMISSION, "u_emission"),
            gbuffer_texture(GBUFFER_ABSORPTION, "u_absorption"),
            gbuffer_texture(GBUFFER_NORMAL, "u_normal"),
            light_map.sprite_texture(0, "u_light_map".into(), GlowTextureFiltering::LinearMipmap),
            nearest.sprite_texture(0, "u_cascade".into(), GlowTextureFiltering::Linear),
            surface_texture,
            shadows.sprite_texture(0, "u_shadow".into(), GlowTextureFiltering::LinearMipmap),
            seeds_texture,
        ];
        for attachment in shadows.surface().attachments() {
            attachment.texture.generate_mipmaps();
        }
        let composite_uniforms = [
            (
                "u_target_size",
                GlowUniformValue::F2(target_size.into_array()),
            ),
            (
                "u_scene_size",
                GlowUniformValue::F2(scene_size.into_array()),
            ),
            (
                "u_cascade_size",
                GlowUniformValue::F2(cascade_size.into_array()),
            ),
            (
                "u_ambient",
                GlowUniformValue::F3(self.config.ambient.into_array()),
            ),
            ("u_texel_world", GlowUniformValue::F1(self.texel_world)),
            (
                "u_view_rect",
                GlowUniformValue::F4(self.view_rect.into_array()),
            ),
            ("u_probe_spacing", GlowUniformValue::F1(probe_spacing)),
            (
                "u_shadow_light",
                GlowUniformValue::F3(shadow_light.into_array()),
            ),
            (
                "u_shadow_open",
                GlowUniformValue::F3(shadow_open.into_array()),
            ),
            (
                "u_shadow_softness",
                GlowUniformValue::F1(self.config.shadow_softness),
            ),
        ];
        if let Some(overview) = self.overview.as_ref() {
            overview.with(draw, graphics, true, |draw, graphics| {
                for (index, view) in LightingDebugView::OVERVIEW_TILES.into_iter().enumerate() {
                    if view == LightingDebugView::Bloom {
                        continue;
                    }
                    let tile = overview_tile(index, target_size);
                    full_screen_pass(
                        draw,
                        graphics,
                        &shaders.composite,
                        target_size,
                        textures.iter().flatten().cloned(),
                        composite_uniforms
                            .iter()
                            .cloned()
                            .chain([
                                ("u_debug", GlowUniformValue::I1(view.index())),
                                ("u_tile", GlowUniformValue::F4(tile.into_array())),
                            ])
                            .map(named)
                            .chain(style_uniforms(&self.uniforms)),
                        GlowBlending::None,
                    );
                }
            });
        }
        hdr.activate(draw, graphics, true);
        full_screen_pass(
            draw,
            graphics,
            &shaders.composite,
            target_size,
            textures.iter().flatten().cloned(),
            composite_uniforms
                .into_iter()
                .chain([
                    ("u_debug", GlowUniformValue::I1(self.debug_view.index())),
                    ("u_tile", GlowUniformValue::F4([0.0; 4])),
                ])
                .map(named)
                .chain(style_uniforms(&self.uniforms)),
            GlowBlending::None,
        );

        scene_color.with(draw, graphics, false, |draw, graphics| {
            full_screen_pass(
                draw,
                graphics,
                &shaders.copy,
                target_size,
                hdr.sprite_texture(0, "u_source".into(), GlowTextureFiltering::Nearest),
                [(
                    "u_target_size",
                    GlowUniformValue::F2(target_size.into_array()),
                )],
                GlowBlending::None,
            );
        });
        for attachment in scene_color.surface().attachments() {
            attachment.texture.generate_mipmaps();
        }
        draw.textures.insert(
            SCENE_COLOR_TEXTURE.into(),
            scene_color.surface().attachments()[0].texture.clone(),
        );
    }

    pub fn end(&mut self, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        self.resolve(draw, graphics);
        if self.stage != Stage::Forward {
            return;
        }
        self.stage = Stage::Idle;
        self.style_passes(draw, graphics);
        let adaptation = self.adapt(draw, graphics);
        let bloom = self.bloom(draw, graphics);
        let (Some(shaders), Some(hdr)) = (&self.shaders, &self.hdr) else {
            return;
        };
        Canvas::deactivate(draw, graphics);
        let target_size = Vec2::new(hdr.surface().width(), hdr.surface().height()).as_::<f32>();
        let adapted = self.adapted.as_ref().and_then(|adapted| {
            adapted[self.adapted_index].sprite_texture(
                0,
                "u_adapted".into(),
                GlowTextureFiltering::Nearest,
            )
        });
        let adaptation_key = adaptation.map(|adaptation| adaptation.key).unwrap_or(1.0);
        let adaptation_range = adaptation
            .map(|adaptation| [adaptation.min_exposure, adaptation.max_exposure])
            .unwrap_or([1.0; 2]);
        let bloom_strength = self
            .config
            .bloom
            .map(|bloom| bloom.strength.clamp(0.0, 1.0))
            .unwrap_or_default();
        let textures = [
            hdr.sprite_texture(0, "u_hdr".into(), GlowTextureFiltering::Linear),
            adapted,
            bloom,
        ];
        let tonemap_uniforms = [
            (
                "u_target_size",
                GlowUniformValue::F2(target_size.into_array()),
            ),
            ("u_exposure", GlowUniformValue::F1(self.config.exposure)),
            (
                "u_adaptation",
                GlowUniformValue::I1(adaptation.is_some() as _),
            ),
            ("u_adaptation_key", GlowUniformValue::F1(adaptation_key)),
            ("u_adaptation_range", GlowUniformValue::F2(adaptation_range)),
            (
                "u_tonemap",
                GlowUniformValue::I1(self.debug_view.tonemapped() as _),
            ),
            ("u_bloom_strength", GlowUniformValue::F1(bloom_strength)),
            (
                "u_bloom_ready",
                GlowUniformValue::I1((!self.bloom.is_empty()) as _),
            ),
        ];
        full_screen_pass(
            draw,
            graphics,
            &shaders.tonemap,
            target_size,
            textures.iter().flatten().cloned(),
            tonemap_uniforms
                .iter()
                .cloned()
                .chain([
                    (
                        "u_bloom_view",
                        GlowUniformValue::I1((self.debug_view == LightingDebugView::Bloom) as _),
                    ),
                    ("u_tile", GlowUniformValue::F4([0.0; 4])),
                ])
                .map(named)
                .chain(style_uniforms(&self.uniforms)),
            GlowBlending::PremultipliedAlpha,
        );
        if let Some(overview) = &self.overview {
            full_screen_pass(
                draw,
                graphics,
                &shaders.copy,
                target_size,
                overview.sprite_texture(0, "u_source".into(), GlowTextureFiltering::Nearest),
                [(
                    "u_target_size",
                    GlowUniformValue::F2(target_size.into_array()),
                )],
                GlowBlending::PremultipliedAlpha,
            );
            let bloom_index = LightingDebugView::OVERVIEW_TILES
                .iter()
                .position(|view| *view == LightingDebugView::Bloom)
                .unwrap_or_default();
            full_screen_pass(
                draw,
                graphics,
                &shaders.tonemap,
                target_size,
                textures.iter().flatten().cloned(),
                tonemap_uniforms
                    .into_iter()
                    .chain([
                        ("u_bloom_view", GlowUniformValue::I1(1)),
                        (
                            "u_tile",
                            GlowUniformValue::F4(
                                overview_tile(bloom_index, target_size).into_array(),
                            ),
                        ),
                    ])
                    .map(named)
                    .chain(style_uniforms(&self.uniforms)),
                GlowBlending::None,
            );
            if let Some(font) = &self.debug_font {
                draw_overview_labels(draw, graphics, &shaders.text, font, target_size);
            }
        }
    }

    /// A pass cannot read the canvas it writes, so each pass first copies the HDR canvas to
    /// the scene color canvas. The water already read the scene color canvas in this frame.
    fn style_passes(&self, draw: &mut DrawContext, graphics: &mut Graphics<Vertex>) {
        let (
            Some(shaders),
            Some(hdr),
            Some(scratch),
            Some(gbuffer),
            Some(light_map),
            Some(shadows),
        ) = (
            &self.shaders,
            &self.hdr,
            &self.scene_color,
            &self.gbuffer,
            &self.light_map,
            &self.shadows,
        )
        else {
            return;
        };
        if shaders.passes.is_empty() || !self.debug_view.tonemapped() {
            return;
        }
        let target_size = Vec2::new(hdr.surface().width(), hdr.surface().height()).as_::<f32>();
        let scene_size = self.scene_size.as_::<f32>();
        let shadow_light = self.config.sun + self.config.sky * self.config.shadow_sky;
        let shadow_open = self.config.sun + self.config.sky;
        let gbuffer_texture = |index: usize, sampler: &'static str| {
            gbuffer.sprite_texture(index, sampler.into(), GlowTextureFiltering::Linear)
        };
        for pass in &shaders.passes {
            scratch.with(draw, graphics, false, |draw, graphics| {
                full_screen_pass(
                    draw,
                    graphics,
                    &shaders.copy,
                    target_size,
                    hdr.sprite_texture(0, "u_source".into(), GlowTextureFiltering::Nearest),
                    [(
                        "u_target_size",
                        GlowUniformValue::F2(target_size.into_array()),
                    )],
                    GlowBlending::None,
                );
            });
            let textures = [
                scratch.sprite_texture(0, "u_source".into(), GlowTextureFiltering::Linear),
                gbuffer_texture(GBUFFER_ALBEDO, "u_albedo"),
                gbuffer_texture(GBUFFER_EMISSION, "u_emission"),
                gbuffer_texture(GBUFFER_ABSORPTION, "u_absorption"),
                gbuffer_texture(GBUFFER_NORMAL, "u_normal"),
                light_map.sprite_texture(
                    0,
                    "u_light_map".into(),
                    GlowTextureFiltering::LinearMipmap,
                ),
                shadows.sprite_texture(0, "u_shadow".into(), GlowTextureFiltering::LinearMipmap),
            ];
            hdr.with(draw, graphics, false, |draw, graphics| {
                full_screen_pass(
                    draw,
                    graphics,
                    pass,
                    target_size,
                    textures.iter().flatten().cloned(),
                    [
                        (
                            "u_target_size",
                            GlowUniformValue::F2(target_size.into_array()),
                        ),
                        (
                            "u_scene_size",
                            GlowUniformValue::F2(scene_size.into_array()),
                        ),
                        (
                            "u_view_rect",
                            GlowUniformValue::F4(self.view_rect.into_array()),
                        ),
                        ("u_texel_world", GlowUniformValue::F1(self.texel_world)),
                        (
                            "u_shadow_softness",
                            GlowUniformValue::F1(self.config.shadow_softness),
                        ),
                        (
                            "u_shadow_light",
                            GlowUniformValue::F3(shadow_light.into_array()),
                        ),
                        (
                            "u_shadow_open",
                            GlowUniformValue::F3(shadow_open.into_array()),
                        ),
                    ]
                    .map(named)
                    .into_iter()
                    .chain(style_uniforms(&self.uniforms)),
                    GlowBlending::None,
                );
            });
        }
    }

    /// Each level stores `1 - spread` of its blur, so the next downsample scales the level back.
    /// The upsample then adds `spread` of the larger level, and the weights of all levels sum to 1.
    /// The first level reads the HDR canvas with a Karis average, which weights each group of
    /// taps by the inverse of its luminance. Without it a single very bright pixel, like a water
    /// sparkle, makes a large glow that blinks.
    fn bloom(
        &self,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) -> Option<SpriteTexture> {
        let (Some(shaders), Some(hdr)) = (&self.shaders, &self.hdr) else {
            return None;
        };
        let spread = self.config.bloom?.spread.clamp(0.0, 0.95);
        let last = self.bloom.len().saturating_sub(1);
        let canvas_size =
            |canvas: &Canvas| Vec2::new(canvas.surface().width(), canvas.surface().height()).as_();
        let mut source = hdr;
        for (index, level) in self.bloom.iter().enumerate() {
            let source_size: Vec2<f32> = canvas_size(source);
            let target_size: Vec2<f32> = canvas_size(level);
            level.with(draw, graphics, false, |draw, graphics| {
                full_screen_pass(
                    draw,
                    graphics,
                    &shaders.bloom_down,
                    target_size,
                    source.sprite_texture(0, "u_source".into(), GlowTextureFiltering::Linear),
                    [
                        (
                            "u_source_size",
                            GlowUniformValue::F2(source_size.into_array()),
                        ),
                        (
                            "u_target_size",
                            GlowUniformValue::F2(target_size.into_array()),
                        ),
                        ("u_first", GlowUniformValue::I1((index == 0) as _)),
                        (
                            "u_input_scale",
                            GlowUniformValue::F1(if index == 0 {
                                1.0
                            } else {
                                1.0 / (1.0 - spread)
                            }),
                        ),
                        (
                            "u_output_scale",
                            GlowUniformValue::F1(if index == last { 1.0 } else { 1.0 - spread }),
                        ),
                    ],
                    GlowBlending::None,
                );
            });
            source = level;
        }
        for index in (1..self.bloom.len()).rev() {
            let source = &self.bloom[index];
            let target = &self.bloom[index - 1];
            let source_size: Vec2<f32> = canvas_size(source);
            let target_size: Vec2<f32> = canvas_size(target);
            target.with(draw, graphics, false, |draw, graphics| {
                full_screen_pass(
                    draw,
                    graphics,
                    &shaders.bloom_up,
                    target_size,
                    source.sprite_texture(0, "u_source".into(), GlowTextureFiltering::Linear),
                    [
                        (
                            "u_source_size",
                            GlowUniformValue::F2(source_size.into_array()),
                        ),
                        (
                            "u_target_size",
                            GlowUniformValue::F2(target_size.into_array()),
                        ),
                        ("u_scale", GlowUniformValue::F1(spread)),
                    ],
                    GlowBlending::Additive,
                );
            });
        }
        self.bloom
            .first()?
            .sprite_texture(0, "u_bloom".into(), GlowTextureFiltering::Linear)
    }

    /// Writes the log average luminance of the HDR canvas, eased over time, to `adapted`.
    fn adapt(
        &mut self,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) -> Option<EyeAdaptation> {
        let Some(adaptation) = self.config.eye_adaptation else {
            self.adapted_time = None;
            return None;
        };
        let (Some(shaders), Some(hdr), Some(luminance), Some(adapted)) =
            (&self.shaders, &self.hdr, &self.luminance, &self.adapted)
        else {
            return None;
        };
        let now = Instant::now();
        let blend = match self.adapted_time {
            Some(time) => 1.0 - (-(now - time).as_secs_f32() * adaptation.speed.max(0.0)).exp(),
            None => 1.0,
        };
        self.adapted_time = Some(now);
        let luminance_size = Vec2::broadcast(LUMINANCE_SIZE as f32);
        let camera = &graphics.state.main_camera;
        let screen_size = camera.screen_size;
        let focus = adaptation
            .focus
            .map(|focus| {
                let point = camera.world_to_screen_point(focus);
                let radius = adaptation.focus_radius * screen_size.y / camera.world_size().y;
                [point.x, screen_size.y - point.y, radius.max(1.0)]
            })
            .unwrap_or([0.0; 3]);
        luminance.with(draw, graphics, false, |draw, graphics| {
            full_screen_pass(
                draw,
                graphics,
                &shaders.luminance,
                luminance_size,
                hdr.sprite_texture(0, "u_hdr".into(), GlowTextureFiltering::Linear),
                [
                    ("u_size", GlowUniformValue::F2(luminance_size.into_array())),
                    (
                        "u_screen_size",
                        GlowUniformValue::F2(screen_size.into_array()),
                    ),
                    ("u_focus", GlowUniformValue::F3(focus)),
                ],
                GlowBlending::None,
            );
        });
        for attachment in luminance.surface().attachments() {
            attachment.texture.generate_mipmaps();
        }
        let previous = &adapted[self.adapted_index];
        self.adapted_index = 1 - self.adapted_index;
        let textures = [
            luminance.sprite_texture(0, "u_luminance".into(), GlowTextureFiltering::LinearMipmap),
            previous.sprite_texture(0, "u_previous".into(), GlowTextureFiltering::Nearest),
        ];
        adapted[self.adapted_index].with(draw, graphics, false, |draw, graphics| {
            full_screen_pass(
                draw,
                graphics,
                &shaders.adaptation,
                Vec2::one(),
                textures.iter().flatten().cloned(),
                [
                    ("u_lod", GlowUniformValue::F1(LUMINANCE_SIZE.ilog2() as f32)),
                    ("u_blend", GlowUniformValue::F1(blend)),
                ],
                GlowBlending::None,
            );
        });
        Some(adaptation)
    }

    fn prepare(
        &mut self,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
    ) -> Result<(), String> {
        if self.shaders.is_none() {
            let gbuffer =
                graphics.shader(Shader::TEXTURED_VERTEX_2D, &shaders::gbuffer_fragment())?;
            let gbuffer_text =
                graphics.shader(Shader::TEXT_VERTEX, &shaders::gbuffer_text_fragment())?;
            draw.shaders.insert(GBUFFER_SHADER.into(), gbuffer);
            draw.shaders
                .insert(GBUFFER_TEXT_SHADER.into(), gbuffer_text);
            let shade = self.style.shade.as_deref();
            let forward = graphics.shader(
                Shader::TEXTURED_VERTEX_2D,
                &shaders::forward_fragment(shade),
            )?;
            let passes = self
                .style
                .passes
                .iter()
                .map(|pass| {
                    graphics.shader(Shader::TEXTURED_VERTEX_2D, &shaders::pass_fragment(pass))
                })
                .collect::<Result<Vec<_>, _>>()?;
            draw.shaders.insert(FORWARD_SHADER.into(), forward.clone());
            let water = graphics.shader(
                Shader::TEXTURED_VERTEX_2D,
                &shaders::water_fragment(self.style.water.as_deref()),
            )?;
            draw.shaders.insert(WATER_SHADER.into(), water.clone());
            let shadow = graphics.shader(shaders::SHADOW_VERTEX, &shaders::shadow_fragment())?;
            draw.shaders.insert(SHADOW_SHADER.into(), shadow.clone());
            self.shaders = Some(LightingShaders {
                forward,
                water,
                shadow,
                copy: graphics.shader(Shader::TEXTURED_VERTEX_2D, shaders::COPY_FRAGMENT)?,
                text: graphics.shader(Shader::TEXTURED_VERTEX_2D, Shader::TEXT_FRAGMENT)?,
                passes,
                bloom_down: graphics
                    .shader(Shader::TEXTURED_VERTEX_2D, shaders::BLOOM_DOWN_FRAGMENT)?,
                bloom_up: graphics
                    .shader(Shader::TEXTURED_VERTEX_2D, shaders::BLOOM_UP_FRAGMENT)?,
                tonemap: graphics.shader(
                    Shader::TEXTURED_VERTEX_2D,
                    &shaders::tonemap_fragment(self.style.grade.as_deref()),
                )?,
                luminance: graphics
                    .shader(Shader::TEXTURED_VERTEX_2D, shaders::LUMINANCE_FRAGMENT)?,
                adaptation: graphics
                    .shader(Shader::TEXTURED_VERTEX_2D, shaders::ADAPTATION_FRAGMENT)?,
                seed: graphics.shader(Shader::TEXTURED_VERTEX_2D, shaders::SEED_FRAGMENT)?,
                jump_flood: graphics
                    .shader(Shader::TEXTURED_VERTEX_2D, shaders::JUMP_FLOOD_FRAGMENT)?,
                cascade: graphics.shader(Shader::TEXTURED_VERTEX_2D, shaders::CASCADE_FRAGMENT)?,
                medium: graphics.shader(Shader::TEXTURED_VERTEX_2D, shaders::MEDIUM_FRAGMENT)?,
                light_map: graphics
                    .shader(Shader::TEXTURED_VERTEX_2D, shaders::LIGHT_MAP_FRAGMENT)?,
                composite: graphics.shader(
                    Shader::TEXTURED_VERTEX_2D,
                    &shaders::composite_fragment(shade),
                )?,
                temporal: graphics
                    .shader(Shader::TEXTURED_VERTEX_2D, shaders::TEMPORAL_FRAGMENT)?,
            });
        }

        let camera = &graphics.state.main_camera;
        let screen_size = camera.screen_size.map(|value| (value as u32).max(1));
        let margin = self.config.margin.max(0.0);
        let scale = self.config.resolution_scale.clamp(0.01, 1.0);
        let base_interval = self.config.base_interval.max(1.0);
        let probe_spacing = self.config.probe_spacing.clamp(1, 8).next_power_of_two();
        let margin_size =
            screen_size.map(|value| (value as f32 * (1.0 + 2.0 * margin)).round() as u32);
        let mut cascade_count = cascade_count_for(margin_size.as_::<f32>() * scale, base_interval);
        let (outer_size, scene_size, snap_texels) = loop {
            let snap_texels = probe_spacing << (cascade_count - 1);
            let outer_size = margin_size + (snap_texels as f32 / scale).ceil() as u32;
            let scene_size = outer_size.map(|value| ((value as f32 * scale).round() as u32).max(1));
            let count = cascade_count_for(scene_size.as_::<f32>(), base_interval);
            if count <= cascade_count {
                break (outer_size, scene_size, snap_texels);
            }
            cascade_count = count;
        };
        let world_size = camera.world_size();
        let world_per_pixel = world_size / screen_size.as_::<f32>();
        let outer_world_size = world_per_pixel * outer_size.as_::<f32>();
        let texel = outer_world_size / scene_size.as_::<f32>();
        let snap_world = texel * snap_texels as f32;
        let position = camera.transform.position.xy();
        let view_origin = position + camera.world_offset();
        let margin_world = world_per_pixel * (margin_size - screen_size).as_::<f32>() * 0.5;
        let scene_origin = ((view_origin - margin_world) / snap_world).floor() * snap_world;
        self.history_shift = match self.scene_origin {
            Some(previous) => {
                let shift = (scene_origin - previous) / (texel * probe_spacing as f32);
                Vec2::new(shift.x, -shift.y).round()
            }
            None => Vec2::zero(),
        };
        self.scene_origin = Some(scene_origin);
        self.scene_camera = (
            CameraScaling::Stretch(outer_world_size),
            (position - scene_origin) / outer_world_size,
        );
        let inner_offset = (view_origin - scene_origin) / outer_world_size;
        let inner_scale = world_size / outer_world_size;
        // Scene textures keep the world bottom at uv 0. A top based offset moves the image
        // with the camera between two grid snaps.
        self.view_rect = Vec4::new(
            inner_offset.x,
            1.0 - inner_offset.y - inner_scale.y,
            inner_scale.x,
            inner_scale.y,
        );
        self.texel_world = texel.y;
        let top_spacing = 1u32 << (cascade_count - 1);
        let cascade_size = scene_size
            .map(|value| value.div_ceil(probe_spacing).div_ceil(top_spacing) * top_spacing * 2);

        match &mut self.gbuffer {
            Some(gbuffer) => gbuffer.match_to_size(graphics, outer_size.x, outer_size.y)?,
            None => {
                let attachments = GBUFFER_FORMATS
                    .iter()
                    .map(|format| {
                        graphics
                            .texture(outer_size.x, outer_size.y, 1, *format, None)
                            .map(Into::into)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.gbuffer =
                    Some(Canvas::from_surface(graphics.surface(attachments)?).color([0.0; 4]));
            }
        }

        match &mut self.medium {
            Some(medium) => medium.match_to_size(graphics, outer_size.x, outer_size.y)?,
            None => {
                let attachments = (0..2)
                    .map(|_| {
                        graphics
                            .texture(
                                outer_size.x,
                                outer_size.y,
                                1,
                                GlowTextureFormat::Data16,
                                None,
                            )
                            .map(Into::into)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.medium = Some(Canvas::from_surface(graphics.surface(attachments)?));
            }
        }

        if self.cascades.is_none() || self.cascade_size != cascade_size {
            let make = || {
                Canvas::simple(
                    cascade_size.x,
                    cascade_size.y,
                    GlowTextureFormat::Data16,
                    graphics,
                )
            };
            self.cascades = Some([make()?, make()?]);
            self.history = Some([make()?, make()?]);
            self.history_valid = false;
        }
        match &mut self.hdr {
            Some(hdr) => hdr.match_to_size(graphics, screen_size.x, screen_size.y)?,
            None => {
                self.hdr = Some(
                    Canvas::simple(
                        screen_size.x,
                        screen_size.y,
                        GlowTextureFormat::Data16,
                        graphics,
                    )?
                    .color([0.0; 4]),
                );
            }
        }
        match &mut self.scene_color {
            Some(scene_color) => {
                scene_color.match_to_size(graphics, screen_size.x, screen_size.y)?
            }
            None => {
                self.scene_color = Some(Canvas::simple(
                    screen_size.x,
                    screen_size.y,
                    GlowTextureFormat::Data16,
                    graphics,
                )?);
            }
        }
        if self.debug_view == LightingDebugView::Overview {
            match &mut self.overview {
                Some(overview) => overview.match_to_size(graphics, screen_size.x, screen_size.y)?,
                None => {
                    self.overview = Some(
                        Canvas::simple(
                            screen_size.x,
                            screen_size.y,
                            GlowTextureFormat::Rgba,
                            graphics,
                        )?
                        .color([0.0; 4]),
                    );
                }
            }
        } else {
            self.overview = None;
        }
        let bloom_sizes = match self.config.bloom {
            Some(bloom) => (1..=bloom.levels.clamp(1, 10))
                .map(|level| screen_size.map(|value| value >> level))
                .take_while(|size| size.x >= 2 && size.y >= 2)
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        if self.bloom.len() == bloom_sizes.len() {
            for (level, size) in self.bloom.iter_mut().zip(&bloom_sizes) {
                level.match_to_size(graphics, size.x, size.y)?;
            }
        } else {
            self.bloom = bloom_sizes
                .iter()
                .map(|size| Canvas::simple(size.x, size.y, GlowTextureFormat::Data16, graphics))
                .collect::<Result<_, _>>()?;
        }
        if self.luminance.is_none() {
            self.luminance = Some(Canvas::simple(
                LUMINANCE_SIZE,
                LUMINANCE_SIZE,
                GlowTextureFormat::Data16,
                graphics,
            )?);
            let make = || Canvas::simple(1, 1, GlowTextureFormat::Data16, graphics);
            self.adapted = Some([make()?, make()?]);
        }
        if self.seeds.is_none() || self.scene_size != scene_size {
            let make = || {
                Canvas::simple(
                    scene_size.x,
                    scene_size.y,
                    GlowTextureFormat::Data32,
                    graphics,
                )
            };
            self.seeds = Some([make()?, make()?]);
            self.surfaces = Some([make()?, make()?]);
        }
        if self.shadows.is_none() || self.scene_size != scene_size {
            self.shadows = Some(
                Canvas::simple(
                    scene_size.x,
                    scene_size.y,
                    GlowTextureFormat::Rgba,
                    graphics,
                )?
                .color([0.0; 4]),
            );
        }
        if self.light_map.is_none() || self.scene_size != scene_size {
            self.light_map = Some(Canvas::simple(
                scene_size.x,
                scene_size.y,
                GlowTextureFormat::Data16,
                graphics,
            )?);
        }

        self.scene_size = scene_size;
        self.cascade_size = cascade_size;
        self.probe_spacing = probe_spacing;
        self.cascade_count = cascade_count;
        Ok(())
    }
}

impl LightingShaders {
    fn jump_flood<'a>(
        &self,
        draw: &mut DrawContext,
        graphics: &mut Graphics<Vertex>,
        canvases: &'a [Canvas; 2],
        sources: impl IntoIterator<Item = SpriteTexture>,
        scene_size: Vec2<u32>,
        invert: bool,
    ) -> &'a Canvas {
        let size = scene_size.as_::<f32>();
        let sources = sources.into_iter().collect::<Vec<_>>();
        canvases[0].with(draw, graphics, false, |draw, graphics| {
            full_screen_pass(
                draw,
                graphics,
                &self.seed,
                size,
                sources.iter().cloned(),
                [
                    ("u_scene_size", GlowUniformValue::F2(size.into_array())),
                    ("u_invert", GlowUniformValue::I1(invert as _)),
                ],
                GlowBlending::None,
            );
        });
        let mut index = 0;
        let mut step = scene_size.reduce_max().next_power_of_two() / 2;
        while step >= 1 {
            let texture =
                canvases[index].sprite_texture(0, "u_seeds".into(), GlowTextureFiltering::Nearest);
            index = 1 - index;
            canvases[index].with(draw, graphics, false, |draw, graphics| {
                full_screen_pass(
                    draw,
                    graphics,
                    &self.jump_flood,
                    size,
                    texture.clone(),
                    [
                        ("u_step", GlowUniformValue::I1(step as _)),
                        ("u_scene_size", GlowUniformValue::F2(size.into_array())),
                    ],
                    GlowBlending::None,
                );
            });
            step /= 2;
        }
        &canvases[index]
    }
}

const OVERVIEW_COLUMNS: f32 = 5.0;

/// The tile rect is in render target pixels, from the bottom left like `gl_FragCoord`.
/// The top row is full, and the shorter bottom row sits in the middle.
fn overview_tile(index: usize, target_size: Vec2<f32>) -> Vec4<f32> {
    let columns = OVERVIEW_COLUMNS as usize;
    let size = target_size / OVERVIEW_COLUMNS;
    let bottom_count = LightingDebugView::OVERVIEW_TILES
        .len()
        .saturating_sub(columns);
    let (column, row) = if index < columns {
        (index as f32, OVERVIEW_COLUMNS - 1.0)
    } else {
        (
            (index - columns) as f32 + (columns - bottom_count) as f32 * 0.5,
            0.0,
        )
    };
    Vec4::new(column * size.x, row * size.y, size.x, size.y)
}

fn draw_overview_labels(
    draw: &mut DrawContext,
    graphics: &mut Graphics<Vertex>,
    shader: &Shader,
    font: &str,
    target_size: Vec2<f32>,
) {
    let font_size = (target_size.y / OVERVIEW_COLUMNS / 9.0).max(10.0);
    let margin = font_size * 0.5;
    for (index, view) in LightingDebugView::OVERVIEW_TILES.into_iter().enumerate() {
        let tile = overview_tile(index, target_size);
        let position = Vec2::new(tile.x, target_size.y - tile.y - tile.w) + margin;
        for (offset, tint) in [
            (
                Vec2::broadcast(font_size * 0.08),
                Rgba::new(0.0, 0.0, 0.0, 0.8),
            ),
            (Vec2::zero(), Rgba::white()),
        ] {
            Text::new(ShaderRef::object(shader.clone()))
                .font(font.to_owned())
                .size(font_size)
                .text(view.label())
                .tint(tint)
                .position(position + offset)
                .screen_space(true)
                .draw(draw, graphics);
        }
    }
}

fn named((name, value): (&'static str, GlowUniformValue)) -> (Cow<'static, str>, GlowUniformValue) {
    (Cow::Borrowed(name), value)
}

fn style_uniforms<'a>(
    uniforms: &'a HashMap<Cow<'static, str>, GlowUniformValue>,
) -> impl Iterator<Item = (Cow<'static, str>, GlowUniformValue)> + 'a {
    uniforms.iter().map(|(name, value)| (name.clone(), *value))
}

fn full_screen_pass<N: Into<Cow<'static, str>>>(
    draw: &mut DrawContext,
    graphics: &mut Graphics<Vertex>,
    shader: &Shader,
    size: Vec2<f32>,
    textures: impl IntoIterator<Item = SpriteTexture>,
    uniforms: impl IntoIterator<Item = (N, GlowUniformValue)>,
    blending: GlowBlending,
) {
    let mut sprite = Sprite {
        shader: Some(ShaderRef::object(shader.clone())),
        size: Some(size),
        blending: Some(blending),
        screen_space: true,
        ..Default::default()
    };
    sprite.textures.extend(textures);
    sprite.uniforms.extend(
        uniforms
            .into_iter()
            .map(|(name, value)| (name.into(), value)),
    );
    sprite.draw(draw, graphics);
}

/// The cascades add `sun * lobe / mean(lobe)` for each escaped ray direction.
/// The division makes `LightingConfig::sun` the light that an unblocked point gets,
/// for any sharpness.
fn cascade_count_for(scene_size: Vec2<f32>, base_interval: f32) -> usize {
    let diagonal = scene_size.magnitude();
    let mut cascade_count = 1;
    while base_interval * (4.0f32.powi(cascade_count as i32) - 1.0) / 3.0 < diagonal {
        cascade_count += 1;
    }
    cascade_count
}

fn sun_lobe_mean(sharpness: f32) -> f32 {
    const SAMPLES: usize = 512;
    let sum = (0..SAMPLES)
        .map(|index| {
            let angle = (index as f32 + 0.5) / SAMPLES as f32 * std::f32::consts::TAU;
            angle.cos().max(0.0).powf(sharpness)
        })
        .sum::<f32>();
    (sum / SAMPLES as f32).max(1.0e-6)
}
