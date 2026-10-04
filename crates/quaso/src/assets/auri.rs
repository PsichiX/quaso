use crate::{assets::name_from_path, context::GameContext, game::GameSubsystem};
use ankha::{parser::AnkhaContentParser, script::AnkhaFile};
use anput::world::World;
use keket::{
    database::{handle::AssetHandle, path::AssetPathStatic},
    protocol::AssetProtocol,
};
use std::{any::Any, error::Error};

pub struct AuriAsset {
    pub file: AnkhaFile,
}

pub struct AuriAssetSubsystem;

impl GameSubsystem for AuriAssetSubsystem {
    fn update(&mut self, mut context: GameContext, _: f32) {
        let added = context
            .assets
            .storage
            .added()
            .iter_of::<AuriAsset>()
            .filter_map(|entity| {
                let (path, asset) = context
                    .assets
                    .storage
                    .lookup_one::<true, (&AssetPathStatic, &AuriAsset)>(entity)?;
                Some((name_from_path(&path).to_owned(), asset.file.to_owned()))
            })
            .collect::<Vec<_>>();
        let removed = context
            .assets
            .storage
            .removed()
            .iter_of::<AuriAsset>()
            .filter_map(|entity| {
                let path = context
                    .assets
                    .storage
                    .lookup_one::<true, &AssetPathStatic>(entity)?;
                Some(name_from_path(&path).to_owned())
            })
            .collect::<Vec<_>>();
        if added.is_empty() && removed.is_empty() {
            return;
        }
        if let Some(scripting) = context.scripting.as_mut() {
            for name in removed {
                scripting.remove_file(&name);
            }
            for (name, file) in added {
                scripting.add_file(name, file);
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct AuriAssetProtocol;

impl AssetProtocol for AuriAssetProtocol {
    fn name(&self) -> &str {
        "auri"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let path = storage.component::<true, AssetPathStatic>(handle.entity())?;
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| format!("Auri script: `{}` is not valid UTF-8!", path.path()))?;
        let file = AnkhaContentParser::default()
            .with_setup(ankha_auri::install)
            .parse_file_content(source)
            .map_err(|error| format!("Auri script: `{}` failed to parse! {error}", path.path()))?;
        drop(path);

        storage.insert(handle.entity(), (AuriAsset { file },))?;

        Ok(())
    }
}
