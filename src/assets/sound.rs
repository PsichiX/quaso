use crate::{assets::name_from_path, context::GameContext, game::GameSubsystem};
use anput::world::World;
use keket::{
    database::{handle::AssetHandle, path::AssetPathStatic},
    protocol::AssetProtocol,
};
use kira::sound::static_sound::StaticSoundData;
use std::{any::Any, error::Error, io::Cursor};

pub struct SoundAsset {
    pub data: StaticSoundData,
}

pub struct SoundAssetSubsystem;

impl GameSubsystem for SoundAssetSubsystem {
    fn update(&mut self, context: GameContext, _: f32) {
        for entity in context.assets.storage.added().iter_of::<SoundAsset>() {
            if let Some((path, asset)) = context
                .assets
                .storage
                .lookup_one::<true, (&AssetPathStatic, &SoundAsset)>(entity)
            {
                context
                    .audio
                    .sounds
                    .insert(name_from_path(&path).to_owned(), asset.data.clone());
            }
        }
        for entity in context.assets.storage.removed().iter_of::<SoundAsset>() {
            if let Some(path) = context
                .assets
                .storage
                .lookup_one::<true, &AssetPathStatic>(entity)
            {
                context.audio.sounds.remove(name_from_path(&path));
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

pub struct SoundAssetProtocol;

impl AssetProtocol for SoundAssetProtocol {
    fn name(&self) -> &str {
        "sound"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let path = storage.component::<true, AssetPathStatic>(handle.entity())?;
        let data = StaticSoundData::from_cursor(Cursor::new(bytes))
            .map_err(|_| format!("Failed to load sound: {:?}", path.path()))?;
        drop(path);

        storage.insert(handle.entity(), (SoundAsset { data },))?;

        Ok(())
    }
}
