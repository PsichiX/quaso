use crate::{assets::name_from_path, context::GameContext, game::GameSubsystem};
use anput::world::World;
use image::{GenericImage, GenericImageView, RgbaImage};
use keket::{
    database::{handle::AssetHandle, path::AssetPathStatic},
    protocol::AssetProtocol,
};
use spitfire_glow::renderer::GlowTextureFormat;
use std::{any::Any, error::Error};

pub struct TextureAsset {
    pub image: RgbaImage,
    pub cols: u32,
    pub rows: u32,
}

pub struct TextureAssetSubsystem;

impl GameSubsystem for TextureAssetSubsystem {
    fn update(&mut self, context: GameContext, _: f32) {
        for entity in context.assets.storage.added().iter_of::<TextureAsset>() {
            if let Some((path, asset)) = context
                .assets
                .storage
                .lookup_one::<true, (&AssetPathStatic, &TextureAsset)>(entity)
            {
                let pages = asset.cols * asset.rows;
                context.draw.textures.insert(
                    name_from_path(&path).to_owned().into(),
                    context
                        .graphics
                        .texture(
                            asset.image.width(),
                            asset.image.height() / pages,
                            pages,
                            GlowTextureFormat::Rgba,
                            Some(asset.image.as_raw()),
                        )
                        .unwrap(),
                );
            }
        }
        for entity in context.assets.storage.removed().iter_of::<TextureAsset>() {
            if let Some(path) = context
                .assets
                .storage
                .lookup_one::<true, &AssetPathStatic>(entity)
            {
                context.draw.textures.remove(name_from_path(&path));
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

pub struct TextureAssetProtocol;

impl AssetProtocol for TextureAssetProtocol {
    fn name(&self) -> &str {
        "texture"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let path = storage.component::<true, AssetPathStatic>(handle.entity())?;
        let mut cols = 1;
        let mut rows = 1;
        for (key, value) in path.meta_items() {
            if key == "cols" || key == "c" {
                cols = value.parse().unwrap_or(1);
            } else if key == "rows" || key == "r" {
                rows = value.parse().unwrap_or(1);
            }
        }
        let mut image = image::load_from_memory(&bytes)
            .map_err(|_| format!("Failed to load texture: {:?}", path.path()))?
            .into_rgba8();
        drop(path);
        let pages = cols * rows;
        image = if cols > 1 || rows > 1 {
            let width = image.width() / cols;
            let height = image.height() / rows;
            let mut result = RgbaImage::new(width, height * pages);
            for row in 0..rows {
                for col in 0..cols {
                    let view = image.view(col * width, row * height, width, height);
                    result
                        .copy_from(&*view, 0, (row * cols + col) * height)
                        .unwrap();
                }
            }
            result
        } else {
            image
        };

        storage.insert(handle.entity(), (TextureAsset { image, cols, rows },))?;

        Ok(())
    }
}
