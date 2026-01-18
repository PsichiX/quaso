pub mod grid_world;
/// 1.5.3
/// Converted LDTK JSON schema to Rust using QuickType generator.
/// https://ldtk.io/files/MINIMAL_JSON_SCHEMA.json
#[allow(clippy::all)]
pub mod ldtk;

use crate::map::ldtk::Ldtk;
use spitfire_draw::{
    context::DrawContext,
    sprite::SpriteTexture,
    utils::{Drawable, ShaderRef, TextureRef, Vertex, transform_to_matrix},
};
use spitfire_glow::{
    graphics::{GraphicsBatch, GraphicsTarget},
    renderer::{GlowBlending, GlowTextureFiltering, GlowUniformValue},
};
use std::{borrow::Cow, collections::HashMap};
use vek::{Quaternion, Rect, Rgba, Transform, Vec2, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LdtkMapColliderResult {
    Ignore,
    ClearArea,
    AggregateMask(u32),
    UniqueAreaMask(u32),
}

impl LdtkMapColliderResult {
    pub fn does_clear_area(&self) -> bool {
        matches!(self, Self::ClearArea | Self::UniqueAreaMask(_))
    }

    pub fn mask(&self) -> Option<u32> {
        match self {
            Self::AggregateMask(mask) | Self::UniqueAreaMask(mask) => Some(*mask),
            _ => None,
        }
    }
}

pub struct LdtkMapBuilder<'a> {
    pub pixel_world_scale: f32,
    pub image_shader: Option<ShaderRef>,
    pub sampler: Cow<'static, str>,
    pub texture_filtering: GlowTextureFiltering,
    #[allow(clippy::type_complexity)]
    pub tileset_reference_extractor: Option<Box<dyn Fn(&str) -> String + 'a>>,
    #[allow(clippy::type_complexity)]
    pub int_grid_collision_extractor: Box<dyn Fn(&str) -> LdtkMapColliderResult + 'a>,
}

impl Default for LdtkMapBuilder<'_> {
    fn default() -> Self {
        Self {
            pixel_world_scale: 1.0,
            image_shader: None,
            sampler: "u_image".into(),
            texture_filtering: Default::default(),
            tileset_reference_extractor: None,
            int_grid_collision_extractor: Box::new(|_| LdtkMapColliderResult::Ignore),
        }
    }
}

impl<'a> LdtkMapBuilder<'a> {
    pub fn pixel_world_scale(mut self, scale: f32) -> Self {
        self.pixel_world_scale = scale;
        self
    }

    pub fn image_shader(mut self, shader: ShaderRef) -> Self {
        self.image_shader = Some(shader);
        self
    }

    pub fn sampler(mut self, sampler: impl Into<Cow<'static, str>>) -> Self {
        self.sampler = sampler.into();
        self
    }

    pub fn tileset_reference_extractor(mut self, extractor: impl Fn(&str) -> String + 'a) -> Self {
        self.tileset_reference_extractor = Some(Box::new(extractor));
        self
    }

    pub fn int_grid_collision_extractor(
        mut self,
        extractor: impl Fn(&str) -> LdtkMapColliderResult + 'a,
    ) -> Self {
        self.int_grid_collision_extractor = Box::new(extractor);
        self
    }

    pub fn build(&self, ldtk: &Ldtk) -> Map {
        let levels = ldtk
            .levels
            .iter()
            .map(|level| {
                let layers = level
                    .layer_instances
                    .as_ref()
                    .into_iter()
                    .flatten()
                    .rev()
                    .filter_map(|layer| {
                        let tileset_uid = layer.tileset_def_uid?;
                        if layer.auto_layer_tiles.is_empty() {
                            return None;
                        }
                        let tileset_definition = ldtk
                            .defs
                            .tilesets
                            .iter()
                            .find(|definition| definition.uid == tileset_uid)
                            .unwrap();
                        let texture_reference = tileset_definition.rel_path.as_deref()?;
                        let texture_reference = self
                            .tileset_reference_extractor
                            .as_ref()
                            .map(|extractor| extractor(texture_reference))
                            .unwrap_or_else(|| texture_reference.to_owned());
                        let texture_reference = TextureRef::name(texture_reference);
                        let tiles = layer
                            .auto_layer_tiles
                            .iter()
                            .map(|tile| MapTile {
                                visible: true,
                                rectangle: Rect {
                                    x: tile.px[0] as f32 * self.pixel_world_scale,
                                    y: tile.px[1] as f32 * self.pixel_world_scale,
                                    w: layer.grid_size as f32 * self.pixel_world_scale,
                                    h: layer.grid_size as f32 * self.pixel_world_scale,
                                },
                                region: Rect {
                                    x: tile.src[0] as f32 / tileset_definition.px_wid as f32,
                                    y: tile.src[1] as f32 / tileset_definition.px_hei as f32,
                                    w: tileset_definition.tile_grid_size as f32
                                        / tileset_definition.px_wid as f32,
                                    h: tileset_definition.tile_grid_size as f32
                                        / tileset_definition.px_hei as f32,
                                },
                                page: 0.0,
                                color: Rgba::white(),
                            })
                            .collect::<Vec<_>>();
                        let visibility_region = tiles
                            .iter()
                            .map(|tile| tile.rectangle)
                            .reduce(|current, item| current.union(item));
                        Some(MapLayer {
                            visible: layer.visible,
                            visibility_region,
                            shader: self.image_shader.clone(),
                            textures: vec![SpriteTexture {
                                sampler: self.sampler.clone(),
                                texture: texture_reference,
                                filtering: self.texture_filtering,
                            }],
                            uniforms: Default::default(),
                            blending: None,
                            transform: Transform {
                                position: Vec3::new(
                                    layer.px_total_offset_x as f32 * self.pixel_world_scale,
                                    layer.px_total_offset_y as f32 * self.pixel_world_scale,
                                    0.0,
                                ),
                                orientation: Quaternion::identity(),
                                scale: Vec3::one(),
                            },
                            tiles,
                        })
                    })
                    .collect();
                let mut colliders = vec![];
                for layer in level.layer_instances.as_ref().into_iter().flatten().rev() {
                    let layer_definition = ldtk
                        .defs
                        .layers
                        .iter()
                        .find(|definition| definition.uid == layer.layer_def_uid)
                        .unwrap();
                    for (index, value) in layer.int_grid_csv.iter().enumerate() {
                        let index = index as i64;
                        let Some(value_definition) = layer_definition
                            .int_grid_values
                            .iter()
                            .find(|v| v.value == *value)
                        else {
                            continue;
                        };
                        let Some(value_id) = value_definition.identifier.as_deref() else {
                            continue;
                        };
                        let col = index % layer.c_wid;
                        let row = index / layer.c_wid;
                        let rectangle = Rect {
                            x: (col * layer.grid_size + layer.px_total_offset_x) as f32
                                * self.pixel_world_scale,
                            y: (row * layer.grid_size + layer.px_total_offset_y) as f32
                                * self.pixel_world_scale,
                            w: layer.grid_size as f32 * self.pixel_world_scale,
                            h: layer.grid_size as f32 * self.pixel_world_scale,
                        };
                        let result = self.int_grid_collision_extractor.as_ref()(value_id);
                        if result.does_clear_area() {
                            colliders.retain(|collider: &MapCollider| {
                                !collider.rectangle.collides_with_rect(rectangle)
                            });
                        }
                        if let Some(mask) = result.mask() {
                            colliders.push(MapCollider {
                                enabled: true,
                                rectangle,
                                mask,
                            });
                        }
                    }
                }
                MapLevel {
                    visible: true,
                    layers,
                    colliders,
                    transform: Transform {
                        position: Vec3::new(
                            level.world_x as f32 * self.pixel_world_scale,
                            level.world_y as f32 * self.pixel_world_scale,
                            0.0,
                        ),
                        orientation: Quaternion::identity(),
                        scale: Vec3::one(),
                    },
                }
            })
            .collect();
        Map {
            levels,
            transform: Default::default(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Map {
    pub levels: Vec<MapLevel>,
    pub transform: Transform<f32, f32, f32>,
}

impl Map {
    pub fn new(levels: impl IntoIterator<Item = MapLevel>) -> Self {
        Self {
            levels: levels.into_iter().collect(),
            transform: Default::default(),
        }
    }

    pub fn level(mut self, level: MapLevel) -> Self {
        self.levels.push(level);
        self
    }

    pub fn levels(mut self, levels: impl IntoIterator<Item = MapLevel>) -> Self {
        self.levels.extend(levels);
        self
    }

    pub fn transform(mut self, transform: Transform<f32, f32, f32>) -> Self {
        self.transform = transform;
        self
    }

    pub fn position(mut self, position: Vec3<f32>) -> Self {
        self.transform.position = position;
        self
    }

    pub fn rotation(mut self, rotation: Quaternion<f32>) -> Self {
        self.transform.orientation = rotation;
        self
    }

    pub fn scale(mut self, scale: Vec3<f32>) -> Self {
        self.transform.scale = scale;
        self
    }

    pub fn collides_with_point(&self, point: Vec2<f32>, mask: u32) -> bool {
        let point = transform_to_matrix(self.transform)
            .inverted()
            .mul_point(point);
        self.levels
            .iter()
            .any(|level| level.collides_with_point(point, mask))
    }

    pub fn draw<'a>(&'a self) -> MapRenderer<'a> {
        MapRenderer {
            clip_region: None,
            clip_each_tile: false,
            show_colliders: None,
            map: self,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MapLevel {
    pub visible: bool,
    pub layers: Vec<MapLayer>,
    pub colliders: Vec<MapCollider>,
    pub transform: Transform<f32, f32, f32>,
}

impl Default for MapLevel {
    fn default() -> Self {
        Self {
            visible: true,
            layers: Default::default(),
            colliders: Default::default(),
            transform: Default::default(),
        }
    }
}

impl MapLevel {
    pub fn visibility(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn layer(mut self, layer: MapLayer) -> Self {
        self.layers.push(layer);
        self
    }

    pub fn layers(mut self, layers: impl IntoIterator<Item = MapLayer>) -> Self {
        self.layers.extend(layers);
        self
    }

    pub fn collider(mut self, collider: MapCollider) -> Self {
        self.colliders.push(collider);
        self
    }

    pub fn colliders(mut self, colliders: impl IntoIterator<Item = MapCollider>) -> Self {
        self.colliders.extend(colliders);
        self
    }

    pub fn transform(mut self, transform: Transform<f32, f32, f32>) -> Self {
        self.transform = transform;
        self
    }

    pub fn position(mut self, position: Vec3<f32>) -> Self {
        self.transform.position = position;
        self
    }

    pub fn rotation(mut self, rotation: Quaternion<f32>) -> Self {
        self.transform.orientation = rotation;
        self
    }

    pub fn scale(mut self, scale: Vec3<f32>) -> Self {
        self.transform.scale = scale;
        self
    }

    pub fn collides_with_point(&self, point: Vec2<f32>, mask: u32) -> bool {
        let point = transform_to_matrix(self.transform)
            .inverted()
            .mul_point(point);
        self.visible
            && self
                .colliders
                .iter()
                .any(|collider| collider.collides_with_point(point, mask))
    }
}

#[derive(Debug, Clone)]
pub struct MapLayer {
    pub visible: bool,
    pub visibility_region: Option<Rect<f32, f32>>,
    pub shader: Option<ShaderRef>,
    pub textures: Vec<SpriteTexture>,
    pub uniforms: HashMap<Cow<'static, str>, GlowUniformValue>,
    pub blending: Option<GlowBlending>,
    pub transform: Transform<f32, f32, f32>,
    pub tiles: Vec<MapTile>,
}

impl Default for MapLayer {
    fn default() -> Self {
        Self {
            visible: true,
            visibility_region: None,
            shader: None,
            textures: Default::default(),
            uniforms: Default::default(),
            blending: None,
            transform: Default::default(),
            tiles: Default::default(),
        }
    }
}

impl MapLayer {
    pub fn single(texture: SpriteTexture) -> Self {
        Self {
            textures: vec![texture],
            ..Default::default()
        }
    }

    pub fn visibility(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn visibility_region(mut self, region: Rect<f32, f32>) -> Self {
        self.visibility_region = Some(region);
        self
    }

    pub fn shader(mut self, shader: ShaderRef) -> Self {
        self.shader = Some(shader);
        self
    }

    pub fn texture(mut self, texture: SpriteTexture) -> Self {
        self.textures.push(texture);
        self
    }

    pub fn uniform(mut self, key: Cow<'static, str>, value: GlowUniformValue) -> Self {
        self.uniforms.insert(key, value);
        self
    }

    pub fn blending(mut self, value: GlowBlending) -> Self {
        self.blending = Some(value);
        self
    }

    pub fn transform(mut self, value: Transform<f32, f32, f32>) -> Self {
        self.transform = value;
        self
    }

    pub fn position(mut self, position: Vec3<f32>) -> Self {
        self.transform.position = position;
        self
    }

    pub fn rotation(mut self, rotation: Quaternion<f32>) -> Self {
        self.transform.orientation = rotation;
        self
    }

    pub fn scale(mut self, scale: Vec3<f32>) -> Self {
        self.transform.scale = scale;
        self
    }

    pub fn tile(mut self, tile: MapTile) -> Self {
        self.tiles.push(tile);
        self
    }

    pub fn tiles(mut self, tiles: impl IntoIterator<Item = MapTile>) -> Self {
        self.tiles.extend(tiles);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapTile {
    pub visible: bool,
    pub rectangle: Rect<f32, f32>,
    pub region: Rect<f32, f32>,
    pub page: f32,
    pub color: Rgba<f32>,
}

impl MapTile {
    pub fn new(rectangle: Rect<f32, f32>, region: Rect<f32, f32>, page: f32) -> Self {
        Self {
            visible: true,
            rectangle,
            region,
            page,
            color: Rgba::white(),
        }
    }

    pub fn visibility(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn color(mut self, color: Rgba<f32>) -> Self {
        self.color = color;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapCollider {
    pub enabled: bool,
    pub rectangle: Rect<f32, f32>,
    pub mask: u32,
}

impl MapCollider {
    pub fn new(rectangle: Rect<f32, f32>, mask: u32) -> Self {
        Self {
            enabled: true,
            rectangle,
            mask,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn mask(mut self, mask: u32) -> Self {
        self.mask = mask;
        self
    }

    pub fn collides_with_point(&self, point: Vec2<f32>, mask: u32) -> bool {
        self.enabled && self.mask & mask != 0 && self.rectangle.contains_point(point)
    }
}

pub struct MapRenderer<'a> {
    pub clip_region: Option<Rect<f32, f32>>,
    pub clip_each_tile: bool,
    pub show_colliders: Option<(ShaderRef, Rgba<f32>, GlowBlending)>,
    map: &'a Map,
}

impl MapRenderer<'_> {
    pub fn clip_region(mut self, region: Rect<f32, f32>) -> Self {
        self.clip_region = Some(region);
        self
    }

    pub fn clip_each_tile(mut self, clip: bool) -> Self {
        self.clip_each_tile = clip;
        self
    }

    pub fn show_colliders(
        mut self,
        shader: ShaderRef,
        color: Rgba<f32>,
        blending: GlowBlending,
    ) -> Self {
        self.show_colliders = Some((shader, color, blending));
        self
    }
}

impl Drawable for MapRenderer<'_> {
    fn draw(&self, context: &mut DrawContext, graphics: &mut dyn GraphicsTarget<Vertex>) {
        for level in &self.map.levels {
            if !level.visible {
                continue;
            }
            for layer in &level.layers {
                if !layer.visible || layer.tiles.is_empty() {
                    continue;
                }
                if let Some(visibility_region) = layer.visibility_region
                    && let Some(clip_region) = self.clip_region
                    && !visibility_region.collides_with_rect(clip_region)
                {
                    continue;
                }
                let batch = GraphicsBatch {
                    shader: context.shader(layer.shader.as_ref()),
                    uniforms: layer
                        .uniforms
                        .iter()
                        .map(|(k, v)| (k.clone(), v.to_owned()))
                        .chain(std::iter::once((
                            "u_projection_view".into(),
                            GlowUniformValue::M4(
                                graphics.state().main_camera.world_matrix().into_col_array(),
                            ),
                        )))
                        .chain(layer.textures.iter().enumerate().map(|(index, texture)| {
                            (texture.sampler.clone(), GlowUniformValue::I1(index as _))
                        }))
                        .collect(),
                    textures: layer
                        .textures
                        .iter()
                        .filter_map(|texture| {
                            Some((context.texture(Some(&texture.texture))?, texture.filtering))
                        })
                        .collect(),
                    blending: layer.blending.unwrap_or_else(|| context.top_blending()),
                    scissor: None,
                    wireframe: context.wireframe,
                };
                graphics.state_mut().stream.batch_optimized(batch);
                let transform = context.top_transform()
                    * transform_to_matrix(self.map.transform)
                    * transform_to_matrix(level.transform)
                    * transform_to_matrix(layer.transform);
                graphics.state_mut().stream.transformed(
                    move |stream| {
                        for tile in &layer.tiles {
                            if !tile.visible {
                                continue;
                            }
                            if self.clip_each_tile
                                && let Some(clip_region) = self.clip_region
                                && !tile.rectangle.collides_with_rect(clip_region)
                            {
                                continue;
                            }
                            let offset = tile.rectangle.position();
                            let size = tile.rectangle.extent();
                            let color = tile.color.into_array();
                            stream.quad([
                                Vertex {
                                    position: [offset.x, offset.y],
                                    uv: [tile.region.x, tile.region.y, tile.page],
                                    color,
                                },
                                Vertex {
                                    position: [offset.x + size.w, offset.y],
                                    uv: [tile.region.x + tile.region.w, tile.region.y, tile.page],
                                    color,
                                },
                                Vertex {
                                    position: [offset.x + size.w, offset.y + size.h],
                                    uv: [
                                        tile.region.x + tile.region.w,
                                        tile.region.y + tile.region.h,
                                        tile.page,
                                    ],
                                    color,
                                },
                                Vertex {
                                    position: [offset.x, offset.y + size.h],
                                    uv: [tile.region.x, tile.region.y + tile.region.h, tile.page],
                                    color,
                                },
                            ]);
                        }
                    },
                    |vertex| {
                        let point = transform.mul_point(Vec2::from(vertex.position));
                        vertex.position[0] = point.x;
                        vertex.position[1] = point.y;
                    },
                );
            }
            let Some((shader, color, blending)) = &self.show_colliders else {
                continue;
            };
            let color = color.into_array();
            let batch = GraphicsBatch {
                shader: context.shader(Some(shader)),
                uniforms: std::iter::once((
                    "u_projection_view".into(),
                    GlowUniformValue::M4(
                        graphics.state().main_camera.world_matrix().into_col_array(),
                    ),
                ))
                .collect(),
                textures: Default::default(),
                blending: *blending,
                scissor: None,
                wireframe: context.wireframe,
            };
            graphics.state_mut().stream.batch_optimized(batch);
            let transform = context.top_transform()
                * transform_to_matrix(self.map.transform)
                * transform_to_matrix(level.transform);
            graphics.state_mut().stream.transformed(
                move |stream| {
                    for collider in &level.colliders {
                        if !collider.enabled {
                            continue;
                        }
                        if self.clip_each_tile
                            && let Some(clip_region) = self.clip_region
                            && !collider.rectangle.collides_with_rect(clip_region)
                        {
                            continue;
                        }
                        let offset = collider.rectangle.position();
                        let size = collider.rectangle.extent();
                        stream.quad([
                            Vertex {
                                position: [offset.x, offset.y],
                                uv: [0.0, 0.0, 0.0],
                                color,
                            },
                            Vertex {
                                position: [offset.x + size.w, offset.y],
                                uv: [0.0, 0.0, 0.0],
                                color,
                            },
                            Vertex {
                                position: [offset.x + size.w, offset.y + size.h],
                                uv: [0.0, 0.0, 0.0],
                                color,
                            },
                            Vertex {
                                position: [offset.x, offset.y + size.h],
                                uv: [0.0, 0.0, 0.0],
                                color,
                            },
                        ]);
                    }
                },
                |vertex| {
                    let point = transform.mul_point(Vec2::from(vertex.position));
                    vertex.position[0] = point.x;
                    vertex.position[1] = point.y;
                },
            );
        }
    }
}
