use crate::{
    assets::texture::TextureAsset,
    map::{
        LdtkMapBuilder, Map,
        ldtk::{EntityInstance, LayerInstance, Ldtk, Level},
    },
};
use anput::world::World;
use keket::{
    database::{
        handle::{AssetDependency, AssetHandle},
        path::AssetPathStatic,
    },
    protocol::AssetProtocol,
};
use std::{
    collections::HashMap,
    error::Error,
    io::{Cursor, Read},
};
use vek::{Rect, Vec2};
use zip::ZipArchive;

#[derive(Debug)]
pub struct LdtkAsset {
    pub world: Ldtk,
    pub tilesets: HashMap<String, AssetPathStatic>,
}

impl LdtkAsset {
    pub fn build_map<'a>(&'a self, mut builder: LdtkMapBuilder<'a>) -> Map {
        if builder.tileset_reference_extractor.is_none() {
            builder.tileset_reference_extractor = Some(Box::new(|name| {
                self.tilesets
                    .get(name)
                    .map(|path| path.path())
                    .unwrap_or(name)
                    .to_owned()
            }));
        }
        builder.build(&self.world)
    }

    pub fn layers(&self) -> impl Iterator<Item = (&Level, &LayerInstance)> {
        self.world.levels.iter().flat_map(|level| {
            level
                .layer_instances
                .iter()
                .flatten()
                .map(move |layer| (level, layer))
        })
    }

    pub fn entities(
        &self,
        only_levels: Option<&[&str]>,
        only_layers: Option<&[&str]>,
    ) -> impl Iterator<Item = (&Level, &LayerInstance, &EntityInstance)> {
        self.world
            .levels
            .iter()
            .filter(move |level| {
                only_levels
                    .as_ref()
                    .is_none_or(|only_levels| only_levels.contains(&level.identifier.as_str()))
            })
            .flat_map(move |level| {
                level
                    .layer_instances
                    .iter()
                    .flatten()
                    .filter(move |layer| {
                        only_layers.as_ref().is_none_or(|only_layers| {
                            only_layers.contains(&layer.identifier.as_str())
                        })
                    })
                    .flat_map(move |layer| {
                        layer
                            .entity_instances
                            .iter()
                            .map(move |entity| (level, layer, entity))
                    })
            })
    }

    pub fn extract_entities<R>(
        &self,
        only_levels: Option<&[&str]>,
        only_layers: Option<&[&str]>,
        extractor: &dyn LdtkEntityExtractor<Entity = R>,
    ) -> impl Iterator<Item = R> {
        self.entities(only_levels, only_layers)
            .filter_map(move |(_, _, entity)| extractor.extract(entity))
    }

    #[allow(clippy::type_complexity)]
    pub fn tiles(
        &self,
        fetch_value_ids: bool,
        only_levels: Option<&[&str]>,
        only_layers: Option<&[&str]>,
    ) -> impl Iterator<
        Item = (
            &Level,
            &LayerInstance,
            Vec2<i64>,
            Rect<i64, i64>,
            i64,
            Option<&str>,
        ),
    > {
        self.world
            .levels
            .iter()
            .filter(move |level| {
                only_levels
                    .as_ref()
                    .is_none_or(|only_levels| only_levels.contains(&level.identifier.as_str()))
            })
            .flat_map(move |level| {
                level
                    .layer_instances
                    .iter()
                    .flatten()
                    .filter(move |layer| {
                        only_layers.as_ref().is_none_or(|only_layers| {
                            only_layers.contains(&layer.identifier.as_str())
                        })
                    })
                    .flat_map(move |layer| {
                        let layer_definition = fetch_value_ids
                            .then(|| {
                                self.world
                                    .defs
                                    .layers
                                    .iter()
                                    .find(|definition| definition.uid == layer.layer_def_uid)
                            })
                            .flatten();
                        layer
                            .int_grid_csv
                            .iter()
                            .enumerate()
                            .map(move |(index, value)| {
                                let index = index as i64;
                                let value_id = layer_definition.and_then(|layer_definition| {
                                    layer_definition
                                        .int_grid_values
                                        .iter()
                                        .find(|v| v.value == *value)
                                        .and_then(|v| v.identifier.as_deref())
                                });
                                let col = index % layer.c_wid;
                                let row = index / layer.c_wid;
                                let rectangle = Rect {
                                    x: (col * layer.grid_size + layer.px_total_offset_x),
                                    y: (row * layer.grid_size + layer.px_total_offset_y),
                                    w: layer.grid_size,
                                    h: layer.grid_size,
                                };
                                (
                                    level,
                                    layer,
                                    Vec2::new(col, row),
                                    rectangle,
                                    *value,
                                    value_id,
                                )
                            })
                    })
            })
    }

    pub fn extract_tiles<T>(
        &self,
        fetch_value_ids: bool,
        only_levels: Option<&[&str]>,
        only_layers: Option<&[&str]>,
        extractor: &dyn LdtkTileExtractor<Tile = T>,
    ) -> impl Iterator<Item = T> {
        self.tiles(fetch_value_ids, only_levels, only_layers)
            .filter_map(
                move |(level, layer, grid_position, rectangle, value, value_id)| {
                    extractor.extract(level, layer, grid_position, rectangle, value, value_id)
                },
            )
    }
}

pub trait LdtkTileExtractor {
    type Tile;

    fn extract(
        &self,
        level: &Level,
        layer: &LayerInstance,
        grid_position: Vec2<i64>,
        rectangle: Rect<i64, i64>,
        value: i64,
        value_id: Option<&str>,
    ) -> Option<Self::Tile>;
}

impl<F, R> LdtkTileExtractor for F
where
    F: Fn(&Level, &LayerInstance, Vec2<i64>, Rect<i64, i64>, i64, Option<&str>) -> Option<R>,
{
    type Tile = R;

    fn extract(
        &self,
        level: &Level,
        layer: &LayerInstance,
        grid_position: Vec2<i64>,
        rectangle: Rect<i64, i64>,
        value: i64,
        value_id: Option<&str>,
    ) -> Option<Self::Tile> {
        self(level, layer, grid_position, rectangle, value, value_id)
    }
}

pub struct FilteredLdtkTileExtractor<Tile> {
    #[allow(clippy::type_complexity)]
    pub extractors: Vec<(
        Box<dyn Fn(&Level, &LayerInstance, Vec2<i64>, Rect<i64, i64>, i64, Option<&str>) -> bool>,
        Box<dyn LdtkTileExtractor<Tile = Tile>>,
    )>,
}

impl<Tile> Default for FilteredLdtkTileExtractor<Tile> {
    fn default() -> Self {
        Self {
            extractors: Default::default(),
        }
    }
}

impl<Tile> FilteredLdtkTileExtractor<Tile> {
    pub fn with(
        mut self,
        filter: impl Fn(&Level, &LayerInstance, Vec2<i64>, Rect<i64, i64>, i64, Option<&str>) -> bool
        + 'static,
        extractor: impl LdtkTileExtractor<Tile = Tile> + 'static,
    ) -> Self {
        self.extractors
            .push((Box::new(filter), Box::new(extractor)));
        self
    }

    pub fn by_layer_name(
        mut self,
        layer_name: &'static str,
        extractor: impl LdtkTileExtractor<Tile = Tile> + 'static,
    ) -> Self {
        self.extractors.push((
            Box::new(move |_, layer, _, _, _, _| layer.identifier == layer_name),
            Box::new(extractor),
        ));
        self
    }
}

impl<Tile> LdtkTileExtractor for FilteredLdtkTileExtractor<Tile> {
    type Tile = Tile;

    fn extract(
        &self,
        level: &Level,
        layer: &LayerInstance,
        grid_position: Vec2<i64>,
        rectangle: Rect<i64, i64>,
        value: i64,
        value_id: Option<&str>,
    ) -> Option<Self::Tile> {
        self.extractors.iter().find_map(|(filter, extractor)| {
            if filter(level, layer, grid_position, rectangle, value, value_id) {
                extractor.extract(level, layer, grid_position, rectangle, value, value_id)
            } else {
                None
            }
        })
    }
}

pub trait LdtkEntityExtractor {
    type Entity;

    fn extract(&self, entity: &EntityInstance) -> Option<Self::Entity>;
}

impl<F, R> LdtkEntityExtractor for F
where
    F: Fn(&EntityInstance) -> Option<R>,
{
    type Entity = R;

    fn extract(&self, entity: &EntityInstance) -> Option<Self::Entity> {
        self(entity)
    }
}

pub struct FilteredLdtkEntityExtractor<Entity> {
    #[allow(clippy::type_complexity)]
    pub extractors: Vec<(
        Box<dyn Fn(&EntityInstance) -> bool>,
        Box<dyn LdtkEntityExtractor<Entity = Entity>>,
    )>,
}

impl<Entity> Default for FilteredLdtkEntityExtractor<Entity> {
    fn default() -> Self {
        Self {
            extractors: Default::default(),
        }
    }
}

impl<Entity> FilteredLdtkEntityExtractor<Entity> {
    pub fn with(
        mut self,
        filter: impl Fn(&EntityInstance) -> bool + 'static,
        extractor: impl LdtkEntityExtractor<Entity = Entity> + 'static,
    ) -> Self {
        self.extractors
            .push((Box::new(filter), Box::new(extractor)));
        self
    }

    pub fn by_identifier(
        mut self,
        identifier: &'static str,
        extractor: impl LdtkEntityExtractor<Entity = Entity> + 'static,
    ) -> Self {
        self.extractors.push((
            Box::new(move |entity| entity.identifier == identifier),
            Box::new(extractor),
        ));
        self
    }

    pub fn by_tags(
        mut self,
        tags: &'static [&'static str],
        extractor: impl LdtkEntityExtractor<Entity = Entity> + 'static,
    ) -> Self {
        self.extractors.push((
            Box::new(|entity| entity.tags.iter().all(|tag| tags.contains(&tag.as_str()))),
            Box::new(extractor),
        ));
        self
    }
}

impl<Entity> LdtkEntityExtractor for FilteredLdtkEntityExtractor<Entity> {
    type Entity = Entity;

    fn extract(&self, entity: &EntityInstance) -> Option<Self::Entity> {
        self.extractors.iter().find_map(|(filter, extractor)| {
            if filter(entity) {
                extractor.extract(entity)
            } else {
                None
            }
        })
    }
}

pub struct LdtkAssetProtocol;

impl AssetProtocol for LdtkAssetProtocol {
    fn name(&self) -> &str {
        "ldtk"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        let mut world_name = None;
        let mut tileset_names = Vec::new();
        for file_name in archive.file_names() {
            if file_name.ends_with(".ldtk") {
                world_name = Some(file_name.to_string());
            } else if file_name.ends_with(".png") {
                tileset_names.push(file_name.to_string());
            }
        }
        let Some(world) = world_name else {
            return Err("No world file found in LDTK package".into());
        };
        let path_part = storage
            .component::<true, AssetPathStatic>(handle.entity())?
            .path()
            .to_owned();

        let mut bytes = vec![];
        archive.by_name(&world)?.read_to_end(&mut bytes)?;
        let world = serde_json::from_slice::<Ldtk>(&bytes)?;

        bytes.clear();
        let mut tilesets = HashMap::new();
        for tileset_name in tileset_names {
            let mut bytes = vec![];
            archive.by_name(&tileset_name)?.read_to_end(&mut bytes)?;
            let image = image::load_from_memory(&bytes)?.into_rgba8();
            let path = AssetPathStatic::new(format!("texture://{path_part}/{tileset_name}"));
            let asset = TextureAsset {
                image,
                cols: 1,
                rows: 1,
            };
            let entity = storage.spawn((path.clone(), asset))?;
            tilesets.insert(tileset_name, path);
            storage.relate::<true, _>(AssetDependency, handle.entity(), entity)?;
        }

        storage.insert(handle.entity(), (LdtkAsset { world, tilesets },))?;

        Ok(())
    }
}
