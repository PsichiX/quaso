use crate::{
    animation::frame::{SpriteAnimationImage, SpriteFrameAnimation},
    assets::name_from_path,
    context::GameContext,
    game::GameSubsystem,
};
use anput::bundle::DynamicBundle;
use image::{
    AnimationDecoder, ImageFormat, Rgba, RgbaImage,
    codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder},
};
use keket::{
    database::{handle::AssetHandle, path::AssetPathStatic},
    protocol::future::{FutureAssetProtocol, FutureStorageAccess},
};
use moirai::coroutine::yield_now;
use send_wrapper::SendWrapper;
use spitfire_draw::utils::TextureRef;
use spitfire_glow::renderer::GlowTextureFormat;
use std::{error::Error, io::Cursor};
use vek::Rect;

pub struct AnimTextureFrame {
    pub image: RgbaImage,
    pub duration: f32,
}

pub struct AnimTextureAsset {
    pub frames: Vec<AnimTextureFrame>,
}

impl AnimTextureAsset {
    pub fn total_duration(&self) -> f32 {
        self.frames.iter().map(|frame| frame.duration).sum()
    }

    pub fn build_animation(&self, texture: TextureRef) -> SpriteFrameAnimation {
        let mut result = SpriteFrameAnimation::default();
        for (index, frame) in self.frames.iter().enumerate() {
            result.images.insert(
                index,
                SpriteAnimationImage {
                    texture: texture.clone(),
                    region: Rect::new(0.0, 0.0, 1.0, 1.0),
                    page: index as f32,
                },
            );
            result.animation.add_frame(index, frame.duration);
        }
        result
    }
}

pub struct AnimTextureAssetSubsystem;

impl GameSubsystem for AnimTextureAssetSubsystem {
    fn run(&mut self, context: GameContext, _: f32) {
        for entity in context.assets.storage.added().iter_of::<AnimTextureAsset>() {
            if let Some((path, asset)) = context
                .assets
                .storage
                .lookup_one::<true, (&AssetPathStatic, &AnimTextureAsset)>(entity)
            {
                if asset.frames.is_empty() {
                    continue;
                }
                let buffer = asset
                    .frames
                    .iter()
                    .flat_map(|frame| frame.image.as_raw().to_owned())
                    .collect::<Vec<u8>>();
                context.draw.textures.insert(
                    name_from_path(&path).to_owned().into(),
                    context
                        .graphics
                        .texture(
                            asset.frames[0].image.width(),
                            asset.frames[0].image.height(),
                            asset.frames.len() as u32,
                            GlowTextureFormat::Rgba,
                            Some(buffer.as_slice()),
                        )
                        .unwrap(),
                );
            }
        }
        for entity in context
            .assets
            .storage
            .removed()
            .iter_of::<AnimTextureAsset>()
        {
            if let Some(path) = context
                .assets
                .storage
                .lookup_one::<true, &AssetPathStatic>(entity)
            {
                context.draw.textures.remove(name_from_path(&path));
            }
        }
    }
}

pub fn make_anim_texture_asset_protocol() -> FutureAssetProtocol {
    FutureAssetProtocol::new("animtexture").process(process_bytes)
}

async fn process_bytes(
    handle: AssetHandle,
    access: FutureStorageAccess,
    bytes: Vec<u8>,
) -> Result<DynamicBundle, Box<dyn Error>> {
    let access = access.access()?;
    let access = access.read().unwrap();
    let path = access.component::<true, AssetPathStatic>(handle.entity())?;

    let format = image::guess_format(&bytes)
        .map_err(|_| format!("Failed to read texture format: {:?}", path.path()))?;

    let mut frames = Vec::new();

    match format {
        ImageFormat::Gif => {
            let decoder = GifDecoder::new(Cursor::new(bytes))
                .map_err(|_| "Failed to decode GIF anim texture")?;
            let mut iter = SendWrapper::new(decoder.into_frames());
            #[allow(clippy::while_let_on_iterator)]
            while let Some(frame) = iter.next() {
                let frame = frame?;
                let (numer, denom) = frame.delay().numer_denom_ms();
                frames.push(AnimTextureFrame {
                    image: frame.into_buffer(),
                    duration: (numer as f32 / denom as f32) * 0.001,
                });
                yield_now().await;
            }
        }
        ImageFormat::Png => {
            let decoder = PngDecoder::new(Cursor::new(bytes))
                .map_err(|_| "Failed to decode APNG anim texture")?;
            let apng = decoder
                .apng()
                .map_err(|_| "Failed to decode APNG anim texture")?;
            let mut iter = SendWrapper::new(apng.into_frames());
            #[allow(clippy::while_let_on_iterator)]
            while let Some(frame) = iter.next() {
                let frame = frame?;
                let (numer, denom) = frame.delay().numer_denom_ms();
                frames.push(AnimTextureFrame {
                    image: frame.into_buffer(),
                    duration: (numer as f32 / denom as f32) * 0.001,
                });
                yield_now().await;
            }
        }
        ImageFormat::WebP => {
            let mut decoder = WebPDecoder::new(Cursor::new(bytes))
                .map_err(|_| "Failed to decode WebP anim texture")?;
            decoder.set_background_color(Rgba([0, 0, 0, 0])).unwrap();
            let mut iter = SendWrapper::new(decoder.into_frames());
            #[allow(clippy::while_let_on_iterator)]
            while let Some(frame) = iter.next() {
                let frame = frame?;
                let (numer, denom) = frame.delay().numer_denom_ms();
                frames.push(AnimTextureFrame {
                    image: frame.into_buffer(),
                    duration: (numer as f32 / denom as f32) * 0.001,
                });
                yield_now().await;
            }
        }
        _ => return Err(format!("Unsupported anim texture format: {:?}", format).into()),
    }

    Ok(DynamicBundle::new(AnimTextureAsset { frames })
        .ok()
        .unwrap())
}
