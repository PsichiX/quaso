use crate::{
    animation::gltf::{
        GltfAnimation, GltfAnimationChannel, GltfAnimationValues, GltfMesh, GltfNode, GltfNodeId,
        GltfPrimitive, GltfSceneTemplate, GltfSkeletonBone, GltfSkin, GltfVertex,
    },
    assets::name_from_path,
    context::GameContext,
    coroutine::async_next_frame,
    game::GameSubsystem,
};
use anput::bundle::DynamicBundle;
use base64::{Engine, prelude::BASE64_STANDARD};
use gltf::{
    Animation, Glb, Gltf, Mesh, Node, Scene, Skin, Texture,
    animation::util::ReadOutputs,
    buffer::{Source as BufferSource, View as BufferView},
    image::Source as ImageSource,
    material::AlphaMode,
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
use send_wrapper::SendWrapper;
use spitfire_core::Triangle;
use spitfire_draw::{sprite::SpriteTexture, utils::TextureRef};
use spitfire_glow::renderer::{GlowBlending, GlowTextureFiltering};
use std::{any::Any, collections::HashMap, error::Error};
use vek::{Mat4, Quaternion, Transform, Vec3};

pub struct GltfAsset {
    pub gltf: Gltf,
}

pub struct GltfAssetSubsystem;

impl GameSubsystem for GltfAssetSubsystem {
    fn update(&mut self, _context: GameContext, _: f32) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub fn make_gltf_asset_protocol() -> FutureAssetProtocol {
    FutureAssetProtocol::new("gltf").process(process_bytes)
}

struct Options {
    pixel_sampling: bool,
    row_major_matrices: bool,
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
    let mut options = Options {
        pixel_sampling: false,
        row_major_matrices: false,
    };
    for (key, _value) in path.meta_items() {
        if key == "binary" || key == "b" {
            binary = true;
        }
        if key == "pixels" || key == "p" {
            options.pixel_sampling = true;
        }
        if key == "rows" || key == "r" {
            options.row_major_matrices = true;
        }
    }

    let gltf = if binary {
        let glb = Glb::from_slice(&bytes)?;
        let gltf = Gltf::from_slice(&glb.json)?;
        process_gltf(handle, access, path, gltf, glb.bin.as_deref(), options).await?
    } else {
        let gltf = Gltf::from_slice(&bytes)?;
        process_gltf(handle, access, path, gltf, None, options).await?
    };

    Ok(DynamicBundle::new(GltfAsset { gltf })
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
    options: Options,
) -> Result<Gltf, Box<dyn Error>> {
    for texture in gltf.textures() {
        process_texture(texture, &path, handle, &access, bin)?;
        async_next_frame().await;
    }

    let mut buffers = Vec::default();
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            BufferSource::Bin => {
                if let Some(bin) = bin {
                    bin.to_vec()
                } else {
                    return Err(
                        "GLTF buffer references BIN section but GLB binary chunk is missing".into(),
                    );
                }
            }
            BufferSource::Uri(uri) => match bytes_from_uri_source(uri)? {
                BytesSource::Data(bytes) => bytes,
                BytesSource::External(path) => {
                    return Err(format!(
                        "GLTF buffer references external URI '{}', which is not supported in this context",
                        path
                    )
                    .into());
                }
            },
        };
        buffers.push(data);
        async_next_frame().await;
    }

    let mut meshes_table = HashMap::default();
    for mesh in gltf.meshes() {
        let index = mesh.index();
        let handle = process_mesh(mesh, &path, handle, &access, &buffers, &options)?;
        meshes_table.insert(index, handle);
        async_next_frame().await;
    }

    let mut skins_table = HashMap::default();
    for skin in gltf.skins() {
        let index = skin.index();
        let handle = process_skin(skin, &path, handle, &access, &buffers, &options)?;
        skins_table.insert(index, handle);
        async_next_frame().await;
    }

    for animation in gltf.animations() {
        process_animation(animation, &path, handle, &access, &buffers)?;
        async_next_frame().await;
    }

    for scene in gltf.scenes() {
        process_scene(scene, &path, handle, &access, &meshes_table, &skins_table)?;
        async_next_frame().await;
    }

    Ok(gltf)
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|c| {
            !c.is_control()
                && !c.is_ascii_control()
                && !c.is_whitespace()
                && !c.is_ascii_whitespace()
        })
        .collect()
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
        .map(sanitize_name)
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
            let asset_path =
                AssetPathStatic::new(format!("texture://{}/{}", name_from_path(path), name));
            access
                .access()?
                .write()
                .unwrap()
                .spawn((asset_path, AssetBytesAreReadyToProcess(bytes)))?
        }
        BytesSource::External(path) => {
            let asset_path = AssetPathStatic::new(format!("texture://{}", path));
            access
                .access()?
                .write()
                .unwrap()
                .spawn((asset_path, AssetAwaitsResolution))?
        }
    };
    access.access()?.write().unwrap().relate::<true, _>(
        AssetDependency,
        handle.entity(),
        entity,
    )?;
    Ok(())
}

fn texture_name(texture: &Texture, path: &AssetPathStatic) -> String {
    let name = texture
        .name()
        .map(sanitize_name)
        .unwrap_or_else(|| texture.index().to_string());
    match texture.source().source() {
        ImageSource::Uri { uri, .. } => {
            if uri.starts_with("data:") {
                format!("{}/{}", name_from_path(path), name)
            } else {
                uri.to_owned()
            }
        }
        ImageSource::View { .. } => {
            format!("{}/{}", name_from_path(path), name)
        }
    }
}

fn process_mesh(
    mesh: Mesh,
    path: &AssetPathStatic,
    handle: AssetHandle,
    access: &FutureStorageAccess,
    buffers: &[Vec<u8>],
    options: &Options,
) -> Result<AssetHandle, Box<dyn Error>> {
    let name = mesh
        .name()
        .map(sanitize_name)
        .unwrap_or_else(|| mesh.index().to_string());
    let asset_path = AssetPathStatic::new(format!("gltf-mesh://{}/{}", path.path(), name));
    let mut primitives = Vec::default();
    for primitive in mesh.primitives() {
        let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|v| v.as_slice()));
        let indices = reader
            .read_indices()
            .map(|v| v.into_u32())
            .ok_or("Mesh primitive is missing indices")?;
        let positions = reader
            .read_positions()
            .ok_or("Mesh primitive is missing positions")?;
        let uvs = reader.read_tex_coords(0).map(|v| v.into_f32());
        let colors = reader.read_colors(0).map(|v| v.into_rgba_f32());
        let joints = reader.read_joints(0).map(|v| v.into_u16());
        let weights = reader.read_weights(0).map(|v| v.into_f32());
        let triangles = TriangleIterator(indices).collect::<Vec<_>>();
        let vertices = VertexIterator {
            positions,
            uvs,
            colors,
            joints,
            weights,
        }
        .collect::<Vec<_>>();
        let main_texture = primitive
            .material()
            .pbr_metallic_roughness()
            .base_color_texture()
            .map(|info| {
                let texture = info.texture();
                let name = texture_name(&texture, path);
                SendWrapper::new(
                    SpriteTexture::new("u_image".into(), TextureRef::name(name)).filtering(
                        if options.pixel_sampling {
                            GlowTextureFiltering::Nearest
                        } else {
                            GlowTextureFiltering::Linear
                        },
                    ),
                )
            });
        let blending = match primitive.material().alpha_mode() {
            AlphaMode::Opaque | AlphaMode::Mask => GlowBlending::None,
            AlphaMode::Blend => GlowBlending::Alpha,
        };
        primitives.push(GltfPrimitive {
            main_texture,
            blending,
            triangles,
            vertices,
        });
    }
    let mesh = GltfMesh { primitives };
    let entity = access
        .access()?
        .write()
        .unwrap()
        .spawn((asset_path, mesh))?;
    access.access()?.write().unwrap().relate::<true, _>(
        AssetDependency,
        handle.entity(),
        entity,
    )?;
    Ok(AssetHandle::new(entity))
}

fn process_skin(
    skin: Skin,
    path: &AssetPathStatic,
    handle: AssetHandle,
    access: &FutureStorageAccess,
    buffers: &[Vec<u8>],
    options: &Options,
) -> Result<AssetHandle, Box<dyn Error>> {
    let name = skin
        .name()
        .map(sanitize_name)
        .unwrap_or_else(|| skin.index().to_string());
    let asset_path = AssetPathStatic::new(format!("gltf-skin://{}/{}", path.path(), name));
    let reader = skin.reader(|buffer| buffers.get(buffer.index()).map(|v| v.as_slice()));
    let Some(inverse_bind_matrices) = reader.read_inverse_bind_matrices() else {
        return Err("Skin is missing inverse bind matrices".into());
    };
    let inverse_bind_matrices = inverse_bind_matrices
        .map(|m| {
            if options.row_major_matrices {
                Mat4::from_row_arrays(m)
            } else {
                Mat4::from_col_arrays(m)
            }
        })
        .collect::<Vec<_>>();
    let bones = skin
        .joints()
        .enumerate()
        .map(|(index, node)| GltfSkeletonBone {
            id: GltfNodeId {
                container_handle: handle,
                node_index: node.index(),
            },
            ibm_index: index,
        })
        .collect::<Vec<_>>();
    let skin = GltfSkin {
        inverse_bind_matrices,
        bones,
    };
    let entity = access
        .access()?
        .write()
        .unwrap()
        .spawn((asset_path, skin))?;
    access.access()?.write().unwrap().relate::<true, _>(
        AssetDependency,
        handle.entity(),
        entity,
    )?;
    Ok(AssetHandle::new(entity))
}

fn process_animation(
    animation: Animation,
    path: &AssetPathStatic,
    handle: AssetHandle,
    access: &FutureStorageAccess,
    buffers: &[Vec<u8>],
) -> Result<(), Box<dyn Error>> {
    let name = animation
        .name()
        .map(sanitize_name)
        .unwrap_or_else(|| animation.index().to_string());
    let asset_path = AssetPathStatic::new(format!("gltf-anim://{}/{}", path.path(), name));
    let mut channels = Vec::default();
    for channel in animation.channels() {
        let reader = channel.reader(|buffer| buffers.get(buffer.index()).map(|v| v.as_slice()));
        let values = match reader
            .read_outputs()
            .ok_or("Animation sampler is missing output values")?
        {
            ReadOutputs::Translations(iter) => {
                let translations = iter.map(|v| v.into()).collect::<Vec<Vec3<f32>>>();
                GltfAnimationValues::Translation(translations)
            }
            ReadOutputs::Rotations(iter) => {
                let rotations = iter
                    .into_f32()
                    .map(|v| Quaternion::from_vec4(v.into()).normalized())
                    .collect::<Vec<Quaternion<f32>>>();
                GltfAnimationValues::Rotation(rotations)
            }
            ReadOutputs::Scales(iter) => {
                let scales = iter.map(|v| v.into()).collect::<Vec<Vec3<f32>>>();
                GltfAnimationValues::Scale(scales)
            }
            _ => continue,
        };
        let times = reader
            .read_inputs()
            .ok_or("Animation sampler is missing input times")?
            .collect::<Vec<f32>>();
        let target_node = GltfNodeId {
            container_handle: handle,
            node_index: channel.target().node().index(),
        };
        let duration = times.iter().copied().fold(0.0_f32, f32::max);
        channels.push(GltfAnimationChannel {
            target_node,
            times,
            duration,
            values,
            interpolation: channel.sampler().interpolation(),
        });
    }
    let duration = channels
        .iter()
        .map(|channel| channel.duration)
        .fold(0.0_f32, f32::max);
    let entity = access
        .access()?
        .write()
        .unwrap()
        .spawn((asset_path.clone(), GltfAnimation { channels, duration }))?;
    access.access()?.write().unwrap().relate::<true, _>(
        AssetDependency,
        handle.entity(),
        entity,
    )?;
    Ok(())
}

fn process_scene(
    scene: Scene,
    path: &AssetPathStatic,
    handle: AssetHandle,
    access: &FutureStorageAccess,
    meshes_table: &HashMap<usize, AssetHandle>,
    skins_table: &HashMap<usize, AssetHandle>,
) -> Result<AssetHandle, Box<dyn Error>> {
    let name = scene
        .name()
        .map(sanitize_name)
        .unwrap_or_else(|| scene.index().to_string());
    let asset_path = AssetPathStatic::new(format!("gltf-scene://{}/{}", path.path(), name));
    let root_nodes = scene
        .nodes()
        .map(|node| process_node(node, handle, meshes_table, skins_table))
        .collect::<Vec<_>>();
    let scene = GltfSceneTemplate {
        name,
        container_handle: handle,
        root_nodes,
    };
    let entity = access
        .access()?
        .write()
        .unwrap()
        .spawn((asset_path, scene))?;
    access.access()?.write().unwrap().relate::<true, _>(
        AssetDependency,
        handle.entity(),
        entity,
    )?;
    Ok(AssetHandle::new(entity))
}

fn process_node(
    node: Node,
    handle: AssetHandle,
    meshes_table: &HashMap<usize, AssetHandle>,
    skins_table: &HashMap<usize, AssetHandle>,
) -> GltfNode {
    let name = node
        .name()
        .map(sanitize_name)
        .unwrap_or_else(|| node.index().to_string());
    let (translation, rotation, scale) = node.transform().decomposed();
    let mesh_handle = node
        .mesh()
        .map(|mesh| meshes_table.get(&mesh.index()).copied().unwrap());
    let skin_handle = node
        .skin()
        .map(|skin| skins_table.get(&skin.index()).copied().unwrap());
    let children = node
        .children()
        .map(|child| process_node(child, handle, meshes_table, skins_table))
        .collect::<Vec<_>>();
    GltfNode {
        id: GltfNodeId {
            container_handle: handle,
            node_index: node.index(),
        },
        name,
        transform: Transform {
            position: translation.into(),
            orientation: Quaternion::from_vec4(rotation.into()).normalized(),
            scale: scale.into(),
        },
        mesh_handle,
        skin_handle,
        children,
    }
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

struct TriangleIterator<I: Iterator<Item = u32>>(I);

impl<I: Iterator<Item = u32>> Iterator for TriangleIterator<I> {
    type Item = Triangle;

    fn next(&mut self) -> Option<Self::Item> {
        let a = self.0.next()?;
        let b = self.0.next()?;
        let c = self.0.next()?;
        Some(Triangle { a, b, c })
    }
}

struct VertexIterator<
    P: Iterator<Item = [f32; 3]>,
    U: Iterator<Item = [f32; 2]>,
    C: Iterator<Item = [f32; 4]>,
    J: Iterator<Item = [u16; 4]>,
    W: Iterator<Item = [f32; 4]>,
> {
    positions: P,
    uvs: Option<U>,
    colors: Option<C>,
    joints: Option<J>,
    weights: Option<W>,
}

impl<
    P: Iterator<Item = [f32; 3]>,
    U: Iterator<Item = [f32; 2]>,
    C: Iterator<Item = [f32; 4]>,
    J: Iterator<Item = [u16; 4]>,
    W: Iterator<Item = [f32; 4]>,
> Iterator for VertexIterator<P, U, C, J, W>
{
    type Item = GltfVertex;

    fn next(&mut self) -> Option<Self::Item> {
        let position = self.positions.next()?;
        let uv = match &mut self.uvs {
            Some(iter) => iter.next(),
            None => None,
        }
        .unwrap_or([0.0, 0.0]);
        let color = match &mut self.colors {
            Some(iter) => iter.next(),
            None => None,
        }
        .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let joints = match &mut self.joints {
            Some(iter) => iter.next(),
            None => None,
        };
        let weights = match &mut self.weights {
            Some(iter) => iter.next(),
            None => None,
        };
        Some(GltfVertex {
            position: position.into(),
            uv: uv.into(),
            color: color.into(),
            joints,
            weights,
        })
    }
}
