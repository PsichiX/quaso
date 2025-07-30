use crate::assets::texture::TextureAsset;
use anput::world::World;
use keket::{
    database::{
        handle::{AssetDependency, AssetHandle},
        path::AssetPathStatic,
    },
    protocol::AssetProtocol,
};
use rusty_spine::{Atlas, SkeletonData, SkeletonJson};
use std::{
    collections::HashMap,
    error::Error,
    io::{Cursor, Read},
    sync::Arc,
};
use zip::ZipArchive;

#[derive(Debug)]
pub struct SpineAsset {
    pub atlas: Arc<Atlas>,
    pub skeleton_data: Arc<SkeletonData>,
    pub textures: HashMap<String, AssetPathStatic>,
}

pub struct SpineAssetProtocol;

impl AssetProtocol for SpineAssetProtocol {
    fn name(&self) -> &str {
        "spine"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        let mut atlas = None;
        let mut skeleton_data = None;
        let mut atlas_page_names = Vec::new();
        for file_name in archive.file_names() {
            if file_name.ends_with(".atlas") {
                atlas = Some(file_name.to_string());
            } else if file_name.ends_with(".json") {
                skeleton_data = Some(file_name.to_string());
            } else if file_name.ends_with(".png") {
                atlas_page_names.push(file_name.to_string());
            }
        }
        let Some(atlas_name) = atlas else {
            return Err("No atlas file found in Spine package".into());
        };
        let Some(skeleton_data_name) = skeleton_data else {
            return Err("No skeleton data file found in Spine package".into());
        };
        let path_part = storage
            .component::<true, AssetPathStatic>(handle.entity())?
            .path()
            .to_owned();

        let mut bytes = vec![];
        archive.by_name(&atlas_name)?.read_to_end(&mut bytes)?;
        let atlas = Arc::new(Atlas::new(&bytes, "")?);

        bytes.clear();
        archive
            .by_name(&skeleton_data_name)?
            .read_to_end(&mut bytes)?;
        let skeleton_data = Arc::new(SkeletonJson::new(atlas.clone()).read_skeleton_data(&bytes)?);

        bytes.clear();
        let mut textures = HashMap::new();
        for atlas_page_name in atlas_page_names {
            let mut bytes = vec![];
            archive.by_name(&atlas_page_name)?.read_to_end(&mut bytes)?;
            let image = image::load_from_memory(&bytes)?.into_rgba8();
            let path = AssetPathStatic::new(format!("texture://{path_part}/{atlas_page_name}"));
            let asset = TextureAsset {
                image,
                cols: 1,
                rows: 1,
            };
            let entity = storage.spawn((path.clone(), asset))?;
            textures.insert(atlas_page_name, path);
            storage.relate::<true, _>(AssetDependency, handle.entity(), entity)?;
        }

        storage.insert(
            handle.entity(),
            (SpineAsset {
                atlas,
                skeleton_data,
                textures,
            },),
        )?;

        Ok(())
    }
}
