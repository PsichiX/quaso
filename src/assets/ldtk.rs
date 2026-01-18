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

    pub fn extract_entities<R>(
        &self,
        extractor: &dyn LdtkEntityExtractor<Entity = R>,
    ) -> impl Iterator<Item = R> {
        self.world.levels.iter().flat_map(move |level| {
            level
                .layer_instances
                .iter()
                .flatten()
                .flat_map(move |layer| {
                    layer
                        .entity_instances
                        .iter()
                        .filter_map(|entity| extractor.extract(entity))
                })
        })
    }

    pub fn entities(&self) -> impl Iterator<Item = (&Level, &LayerInstance, &EntityInstance)> {
        self.world.levels.iter().flat_map(|level| {
            level
                .layer_instances
                .iter()
                .flatten()
                .flat_map(move |layer| {
                    layer
                        .entity_instances
                        .iter()
                        .map(move |entity| (level, layer, entity))
                })
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
