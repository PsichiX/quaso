use crate::{assets::name_from_path, context::GameContext, game::GameSubsystem};
use anput::bundle::DynamicBundle;
use base64::{Engine, prelude::BASE64_STANDARD};
use gltf::{
    Glb, Gltf, Texture,
    buffer::{Source as BufferSource, View as BufferView},
    image::Source as ImageSource,
};
use keket::{
    database::{
        handle::{AssetDependency, AssetHandle},
        path::AssetPathStatic,
    },
    fetch::{AssetAwaitsResolution, AssetBytesAreReadyToProcess},
    protocol::{
        future::{FutureAssetProtocol, FutureStorageAccess},
        group::GroupAsset,
    },
};
use moirai::coroutine::yield_now;
use std::{collections::HashSet, error::Error};

pub enum GltfAssetInstantiateSceneRoots {
    None,
    All,
    Named(HashSet<String>),
}

pub struct GltfAsset {
    pub gltf: Gltf,
    // pub instantiate_scene_roots: GltfAssetInstantiateSceneRoots,
}

pub struct GltfAssetSubsystem;

impl GameSubsystem for GltfAssetSubsystem {
    fn run(&mut self, _context: GameContext, _: f32) {}
}

pub fn make_gltf_asset_protocol() -> FutureAssetProtocol {
    FutureAssetProtocol::new("gltf").process(process_bytes)
}

async fn process_bytes(
    handle: AssetHandle,
    access: FutureStorageAccess,
    bytes: Vec<u8>,
) -> Result<DynamicBundle, Box<dyn Error>> {
    let path = access
        .access()?
        .read()
        .unwrap()
        .component::<true, AssetPathStatic>(handle.entity())?
        .clone();

    let mut binary = false;
    // let mut instantiate_scene_roots = GltfAssetInstantiateSceneRoots::None;
    for (key, _value) in path.meta_items() {
        if key == "binary" || key == "b" {
            binary = true;
        }
        // else if key == "scene" || key == "s" {
        //     if value == "*" {
        //         instantiate_scene_roots = GltfAssetInstantiateSceneRoots::All;
        //     }
        //     match &mut instantiate_scene_roots {
        //         GltfAssetInstantiateSceneRoots::None => {
        //             instantiate_scene_roots = GltfAssetInstantiateSceneRoots::Named(
        //                 [value.to_owned()].into_iter().collect(),
        //             );
        //         }
        //         GltfAssetInstantiateSceneRoots::Named(set) => {
        //             set.insert(value.to_owned());
        //         }
        //         GltfAssetInstantiateSceneRoots::All => {}
        //     }
        // }
    }

    let gltf = if binary {
        let glb = Glb::from_slice(&bytes)?;
        let gltf = Gltf::from_slice(&glb.json)?;
        process_gltf(handle, access, path, gltf, glb.bin.as_deref()).await?
    } else {
        let gltf = Gltf::from_slice(&bytes)?;
        process_gltf(handle, access, path, gltf, None).await?
    };

    Ok(DynamicBundle::new(GltfAsset {
        gltf,
        // instantiate_scene_roots,
    })
    .ok()
    .unwrap()
    .with_component(GroupAsset)
    .ok()
    .unwrap())
}

async fn process_gltf(
    handle: AssetHandle,
    access: FutureStorageAccess,
    path: AssetPathStatic,
    gltf: Gltf,
    bin: Option<&[u8]>,
) -> Result<Gltf, Box<dyn Error>> {
    for texture in gltf.textures() {
        process_texture(texture, &path, handle, &access, bin)?;
        yield_now().await;
    }

    Ok(gltf)
}

fn process_texture(
    texture: Texture,
    path: &AssetPathStatic,
    handle: AssetHandle,
    access: &FutureStorageAccess,
    bin: Option<&[u8]>,
) -> Result<(), Box<dyn Error>> {
    let name = texture
        .name()
        .map(|v| v.to_owned())
        .unwrap_or_else(|| texture.index().to_string());
    let source = match texture.source().source() {
        ImageSource::Uri { uri, .. } => bytes_from_uri_source(uri)?,
        ImageSource::View { view, .. } => {
            if let Some(bin) = bin {
                bytes_from_buffer_view(view, bin)?
            } else {
                return Err(
                    "GLTF image references buffer view but GLB binary chunk is missing".into(),
                );
            }
        }
    };
    let entity = match source {
        BytesSource::Data(bytes) => {
            let path = AssetPathStatic::new(format!("texture://{}@{}", name_from_path(path), name));
            access
                .access()?
                .write()
                .unwrap()
                .spawn((path, AssetBytesAreReadyToProcess(bytes)))?
        }
        BytesSource::External(path) => {
            let path = AssetPathStatic::new(format!("texture://{}", path));
            access
                .access()?
                .write()
                .unwrap()
                .spawn((path, AssetAwaitsResolution))?
        }
    };
    access.access()?.write().unwrap().relate::<true, _>(
        AssetDependency,
        handle.entity(),
        entity,
    )?;
    Ok(())
}

enum BytesSource {
    Data(Vec<u8>),
    External(String),
}

fn bytes_from_buffer_view(
    view: BufferView,
    glb_binary_chunk: &[u8],
) -> Result<BytesSource, Box<dyn Error>> {
    let buffer = view.buffer();
    match buffer.source() {
        BufferSource::Bin => {
            let start = view.offset();
            let end = start + view.length();
            if start <= end && start <= glb_binary_chunk.len() && end <= glb_binary_chunk.len() {
                Ok(BytesSource::Data(glb_binary_chunk[start..end].to_vec()))
            } else {
                Err("Buffer view range is out of bounds of the GLB binary chunk".into())
            }
        }
        BufferSource::Uri(uri) => bytes_from_uri_source(uri),
    }
}

fn bytes_from_uri_source(uri: &str) -> Result<BytesSource, Box<dyn Error>> {
    if uri.starts_with("data:")
        && let Some(comma_index) = uri.find(',')
    {
        let encoded_data = &uri[comma_index + ','.len_utf8()..];
        return Ok(BytesSource::Data(BASE64_STANDARD.decode(encoded_data)?));
    }
    Ok(BytesSource::External(uri.to_owned()))
}
