pub mod anim_texture;
pub mod font;
pub mod gltf;
pub mod ldtk;
pub mod shader;
pub mod sound;
pub mod spine;
pub mod texture;

use crate::assets::{
    anim_texture::make_anim_texture_asset_protocol, font::FontAssetProtocol,
    gltf::make_gltf_asset_protocol, ldtk::LdtkAssetProtocol, shader::ShaderAssetProtocol,
    sound::SoundAssetProtocol, spine::SpineAssetProtocol, texture::TextureAssetProtocol,
};
use keket::{
    database::{AssetDatabase, path::AssetPath},
    fetch::{
        AssetFetch,
        container::{ContainerAssetFetch, ContainerPartialFetch},
        throttled::{ThrottledAssetFetch, ThrottledAssetFetchStrategy},
    },
    protocol::{bytes::BytesAssetProtocol, group::GroupAssetProtocol, text::TextAssetProtocol},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    error::Error,
    hash::{DefaultHasher, Hash, Hasher},
    io::{Cursor, Read, Write},
    ops::Range,
    path::Path,
};

pub fn name_from_path<'a>(path: &'a AssetPath<'a>) -> &'a str {
    path.meta_items()
        .find(|(key, _)| *key == "as")
        .map(|(_, value)| value)
        .unwrap_or(path.path())
}

pub fn make_database(fetch: impl AssetFetch) -> AssetDatabase {
    AssetDatabase::default()
        .with_protocol(BytesAssetProtocol)
        .with_protocol(TextAssetProtocol)
        .with_protocol(GroupAssetProtocol)
        .with_protocol(ShaderAssetProtocol)
        .with_protocol(TextureAssetProtocol)
        .with_protocol(make_anim_texture_asset_protocol())
        .with_protocol(FontAssetProtocol)
        .with_protocol(SoundAssetProtocol)
        .with_protocol(SpineAssetProtocol)
        .with_protocol(LdtkAssetProtocol)
        .with_protocol(make_gltf_asset_protocol())
        .with_fetch(fetch)
}

pub fn make_memory_database(package: &[u8]) -> Result<AssetDatabase, Box<dyn Error>> {
    Ok(make_database(ContainerAssetFetch::new(
        AssetPackage::decode(package)?,
    )))
}

pub fn make_throttled_memory_database(
    package: &[u8],
    strategy: ThrottledAssetFetchStrategy,
) -> Result<AssetDatabase, Box<dyn Error>> {
    Ok(make_database(ThrottledAssetFetch::new(
        ContainerAssetFetch::new(AssetPackage::decode(package)?),
        strategy,
    )))
}

pub fn make_directory_database(
    directory: impl AsRef<Path>,
) -> Result<AssetDatabase, Box<dyn Error>> {
    Ok(make_database(ContainerAssetFetch::new(
        AssetPackage::from_directory(directory)?,
    )))
}

pub fn make_throttled_directory_database(
    directory: impl AsRef<Path>,
    strategy: ThrottledAssetFetchStrategy,
) -> Result<AssetDatabase, Box<dyn Error>> {
    Ok(make_database(ThrottledAssetFetch::new(
        ContainerAssetFetch::new(AssetPackage::from_directory(directory)?),
        strategy,
    )))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AssetPackageRegistry {
    mappings: HashMap<String, Range<usize>>,
}

#[derive(Default)]
pub struct AssetPackage {
    registry: AssetPackageRegistry,
    content: Vec<u8>,
}

impl AssetPackage {
    pub fn from_directory(directory: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        Self::from_directory_filtered(directory, |path| {
            Some(path.file_name().unwrap().to_str().unwrap().to_owned())
        })
    }

    pub fn from_directory_filtered(
        directory: impl AsRef<Path>,
        filter: impl Fn(&Path) -> Option<String>,
    ) -> Result<Self, Box<dyn Error>> {
        fn visit_dirs(
            dir: &Path,
            root: &str,
            registry: &mut AssetPackageRegistry,
            content: &mut Cursor<Vec<u8>>,
            filter: &impl Fn(&Path) -> Option<String>,
        ) -> std::io::Result<()> {
            if dir.is_dir() {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    let name = path.file_name().unwrap().to_str().unwrap();
                    if path.is_dir() {
                        let name = if root.is_empty() {
                            name.to_owned()
                        } else {
                            format!("{root}/{name}")
                        };
                        visit_dirs(&path, &name, registry, content, filter)?;
                    } else if let Some(name) = filter(&path) {
                        let name = if root.is_empty() {
                            name.to_owned()
                        } else {
                            format!("{root}/{name}")
                        };
                        let bytes = std::fs::read(path)?;
                        let start = content.position() as usize;
                        content.write_all(&bytes)?;
                        let end = content.position() as usize;
                        registry.mappings.insert(name, start..end);
                    }
                }
            }
            Ok(())
        }

        let directory = directory.as_ref();
        let mut registry = AssetPackageRegistry::default();
        let mut content = Cursor::new(Vec::default());
        visit_dirs(directory, "", &mut registry, &mut content, &filter)?;
        Ok(AssetPackage {
            registry,
            content: content.into_inner(),
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut stream = Cursor::new(bytes);
        let mut size = 0u32.to_be_bytes();
        stream.read_exact(&mut size)?;
        let size = u32::from_be_bytes(size) as usize;
        let mut registry = vec![0u8; size];
        stream.read_exact(&mut registry)?;
        let registry = toml::from_str(std::str::from_utf8(&registry)?)?;
        let mut content = Vec::default();
        stream.read_to_end(&mut content)?;
        Ok(Self { registry, content })
    }

    pub fn encode(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut stream = Cursor::new(Vec::default());
        let registry = toml::to_string(&self.registry)?;
        let registry = registry.as_bytes();
        stream.write_all(&(registry.len() as u32).to_be_bytes())?;
        stream.write_all(registry)?;
        stream.write_all(&self.content)?;
        Ok(stream.into_inner())
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.registry.mappings.keys().map(|key| key.as_str())
    }

    pub fn paths_and_content_hashes(&self) -> impl Iterator<Item = (&str, u64)> {
        self.registry.mappings.iter().map(move |(key, range)| {
            let mut hasher = DefaultHasher::new();
            self.content[range.clone()].hash(&mut hasher);
            (key.as_str(), hasher.finish())
        })
    }
}

impl ContainerPartialFetch for AssetPackage {
    fn load_bytes(&mut self, path: AssetPath) -> Result<Vec<u8>, Box<dyn Error>> {
        if let Some(range) = self.registry.mappings.get(path.path()).cloned() {
            if range.end <= self.content.len() {
                Ok(self.content[range].to_owned())
            } else {
                Err(format!(
                    "Asset: `{}` out of content bounds! Bytes range: {:?}, content byte size: {}",
                    path,
                    range,
                    self.content.len()
                )
                .into())
            }
        } else {
            Err(format!("Asset: `{path}` not present in package!").into())
        }
    }
}

impl std::fmt::Debug for AssetPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetPackage")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}
