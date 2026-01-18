use crate::assets::name_from_path;
use anput::bundle::DynamicBundle;
use keket::{
    database::{
        handle::{AssetDependency, AssetHandle},
        path::AssetPathStatic,
    },
    fetch::AssetAwaitsResolution,
    protocol::future::{FutureAssetProtocol, FutureStorageAccess},
};
use serde::{Deserialize, Serialize};
use spitfire_draw::{
    sprite::{Sprite, SpriteTexture},
    utils::TextureRef,
};
use std::{borrow::Cow, collections::HashMap, error::Error};
use vek::{Rect, Vec2};

#[derive(Debug, Clone)]
pub struct AtlasTextureAsset {
    pub texture_name: String,
    pub regions: HashMap<String, Rect<f32, f32>>,
}

impl AtlasTextureAsset {
    pub fn sprite(&self, id: &str, sampler: impl Into<Cow<'static, str>>) -> Option<Sprite> {
        Some(
            Sprite::single(SpriteTexture::new(
                sampler.into(),
                TextureRef::name(self.texture_name.to_owned()),
            ))
            .region_page(*self.regions.get(id)?, 0.0),
        )
    }
}

pub fn make_atlas_texture_asset_protocol() -> FutureAssetProtocol {
    FutureAssetProtocol::new("atlastexture").process(process_bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AtlasTextureAssetFormat {
    pub texture: AssetPathStatic,
    pub size: Vec2<usize>,
    pub regions: HashMap<String, Rect<usize, usize>>,
    #[serde(default)]
    pub padding_pixels: f32,
}

async fn process_bytes(
    handle: AssetHandle,
    access: FutureStorageAccess,
    bytes: Vec<u8>,
) -> Result<DynamicBundle, Box<dyn Error>> {
    let content = serde_json::from_slice::<AtlasTextureAssetFormat>(&bytes)?;
    let entity = access
        .access()?
        .write()
        .unwrap()
        .spawn((content.texture.to_owned(), AssetAwaitsResolution))?;
    access.access()?.write().unwrap().relate::<true, _>(
        AssetDependency,
        handle.entity(),
        entity,
    )?;

    Ok(DynamicBundle::new(AtlasTextureAsset {
        texture_name: name_from_path(&content.texture).to_owned(),
        regions: content
            .regions
            .iter()
            .map(|(id, region)| {
                (
                    id.to_owned(),
                    Rect::new(
                        (region.x as f32 + content.padding_pixels) / content.size.x as f32,
                        (region.y as f32 + content.padding_pixels) / content.size.y as f32,
                        (region.w as f32 - content.padding_pixels * 2.0) / content.size.x as f32,
                        (region.h as f32 - content.padding_pixels * 2.0) / content.size.y as f32,
                    ),
                )
            })
            .collect(),
    })
    .ok()
    .unwrap())
}
