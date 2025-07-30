use crate::{assets::name_from_path, context::GameContext, game::GameSubsystem};
use anput::world::World;
use fontdue::Font;
use keket::{
    database::{handle::AssetHandle, path::AssetPathStatic},
    protocol::AssetProtocol,
};
use std::error::Error;

pub struct FontAsset {
    pub font: Font,
}

pub struct FontAssetSubsystem;

impl GameSubsystem for FontAssetSubsystem {
    fn run(&mut self, context: GameContext, _: f32) {
        for entity in context.assets.storage.added().iter_of::<FontAsset>() {
            if let Some((path, asset)) = context
                .assets
                .storage
                .lookup_one::<true, (&AssetPathStatic, &FontAsset)>(entity)
            {
                context
                    .draw
                    .fonts
                    .insert(name_from_path(&path).to_owned(), asset.font.clone());
            }
        }
        for entity in context.assets.storage.removed().iter_of::<FontAsset>() {
            if let Some(path) = context
                .assets
                .storage
                .lookup_one::<true, &AssetPathStatic>(entity)
            {
                context.draw.fonts.remove(name_from_path(&path));
            }
        }
    }
}

pub struct FontAssetProtocol;

impl AssetProtocol for FontAssetProtocol {
    fn name(&self) -> &str {
        "font"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let path = storage.component::<true, AssetPathStatic>(handle.entity())?;
        let font = Font::from_bytes(bytes, Default::default())
            .map_err(|_| format!("Failed to load font: {:?}", path.path()))?;
        drop(path);

        storage.insert(handle.entity(), (FontAsset { font },))?;

        Ok(())
    }
}
