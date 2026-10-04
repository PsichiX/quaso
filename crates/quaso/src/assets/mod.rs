pub mod anim_texture;
pub mod atlas_texture;
pub mod auri;
pub mod font;
pub mod gltf;
pub mod ldtk;
pub mod shader;
pub mod sound;
pub mod spine;
pub mod texture;

use crate::assets::{
    anim_texture::make_anim_texture_asset_protocol,
    atlas_texture::make_atlas_texture_asset_protocol, auri::AuriAssetProtocol,
    font::FontAssetProtocol, gltf::make_gltf_asset_protocol, ldtk::LdtkAssetProtocol,
    shader::ShaderAssetProtocol, sound::SoundAssetProtocol, spine::SpineAssetProtocol,
    texture::TextureAssetProtocol,
};
use keket::{
    database::{
        AssetDatabase,
        handle::AssetHandle,
        path::{AssetPath, AssetPathStatic},
    },
    fetch::{
        AssetFetch,
        container::{ContainerAssetFetch, ContainerPartialFetch},
        throttled::{ThrottledAssetFetch, ThrottledAssetFetchStrategy},
    },
    protocol::{bytes::BytesAssetProtocol, group::GroupAssetProtocol, text::TextAssetProtocol},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    hash::{DefaultHasher, Hash, Hasher},
    io::{Cursor, Read, Write},
    ops::Range,
    path::{Path, PathBuf},
};

pub fn name_from_path<'a>(path: &'a AssetPath<'a>) -> &'a str {
    path.meta_items()
        .find(|(key, _)| *key == "as")
        .map(|(_, value)| value)
        .unwrap_or(path.path())
}

pub fn find_asset_by_name(database: &AssetDatabase, name: &str) -> Option<AssetHandle> {
    database
        .storage
        .find_with::<true, AssetPathStatic>(|path| name_from_path(path) == name)
        .map(AssetHandle::new)
}

pub fn make_database_protocols() -> AssetDatabase {
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
        .with_protocol(make_atlas_texture_asset_protocol())
        .with_protocol(AuriAssetProtocol)
}

pub fn make_database(fetch: impl AssetFetch) -> AssetDatabase {
    make_database_protocols().with_fetch(fetch)
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

pub fn make_replacement_package_filter(
    items: &[(&str, Option<&str>)],
) -> impl Fn(&Path) -> Option<String> {
    move |path| {
        let name = path.file_name()?.to_string_lossy().to_string();
        for (subextension, replacement) in items {
            if name.contains(subextension) {
                let replacement = replacement.as_deref()?;
                return Some(name.replace(subextension, replacement));
            }
        }
        Some(name)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AssetPackageRegistry {
    mappings: HashMap<String, Range<usize>>,
}

pub struct AssetPackageWriter<'a> {
    package: &'a mut AssetPackage,
}

impl AssetPackageWriter<'_> {
    pub fn write(
        &mut self,
        path: impl ToString,
        bytes: impl AsRef<[u8]>,
    ) -> Result<(), Box<dyn Error>> {
        let path = path.to_string();
        if self.package.registry.mappings.contains_key(&path) {
            return Err(format!("Asset: `{path}` already exists in package!").into());
        }
        let bytes = bytes.as_ref();
        let start = self.package.content.len();
        self.package.content.extend_from_slice(bytes);
        let end = self.package.content.len();
        self.package.registry.mappings.insert(path, start..end);
        Ok(())
    }
}

#[derive(Default)]
pub struct AssetPackage {
    registry: AssetPackageRegistry,
    content: Vec<u8>,
}

impl AssetPackage {
    pub fn from_directory(directory: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        Self::from_directory_filtered(directory, |path| {
            Some(path.file_name()?.to_string_lossy().to_string())
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

#[derive(Default)]
pub struct AssetCooker {
    package: AssetPackage,
    #[allow(clippy::type_complexity)]
    recipes: HashMap<
        String,
        Box<dyn Fn(&Value, &Path, &str, AssetPackageWriter) -> Result<(), Box<dyn Error>>>,
    >,
}

impl AssetCooker {
    pub fn with_basic_recipes(self) -> Self {
        self.with_recipe::<FileAssetCookRecipe>("file")
            .with_recipe::<IndexAssetCookRecipe>("index")
            .with_recipe::<ArchiveAssetCookRecipe>("archive")
    }

    pub fn with_recipe<T: AssetCookRecipe>(mut self, name: impl ToString) -> Self {
        self.add_recipe::<T>(name);
        self
    }

    pub fn add_recipe<T: AssetCookRecipe>(&mut self, name: impl ToString) {
        self.recipes.insert(
            name.to_string(),
            Box::new(|value, path, parent, writer| {
                let node = serde_json::from_value::<T>(value.clone())?;
                node.cook(path, parent, writer)
            }),
        );
    }

    pub fn cook(
        &mut self,
        directory: impl AsRef<Path>,
        extension: &str,
    ) -> Result<(), Box<dyn Error>> {
        self.cook_directory(directory.as_ref(), "", extension)
    }

    fn cook_directory(
        &mut self,
        directory: &Path,
        parent: &str,
        extension: &str,
    ) -> Result<(), Box<dyn Error>> {
        if directory.is_dir() {
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                let name = path.file_name().unwrap().to_str().unwrap();
                if path.is_dir() {
                    let name = if parent.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{parent}/{name}")
                    };
                    self.cook_directory(&path, &name, extension)?;
                } else if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
                    let bytes = std::fs::read(&path)?;
                    let node = serde_json::from_slice::<AssetCookNode>(&bytes)?;
                    if let Some(recipe) = self.recipes.get(&node.recipe) {
                        let writer = AssetPackageWriter {
                            package: &mut self.package,
                        };
                        recipe(&node.data, &path, parent, writer)?;
                    } else {
                        return Err(format!("No recipe found for asset: `{}`!", node.recipe).into());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn into_package(self) -> AssetPackage {
        self.package
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetCookNode {
    pub recipe: String,
    #[serde(default)]
    pub data: Value,
}

pub trait AssetCookRecipe: DeserializeOwned {
    fn cook(
        self,
        path: &Path,
        parent: &str,
        writer: AssetPackageWriter,
    ) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAssetCookRecipe {
    pub path: PathBuf,
    #[serde(default)]
    pub rename: Option<String>,
}

impl AssetCookRecipe for FileAssetCookRecipe {
    fn cook(
        self,
        path: &Path,
        parent: &str,
        mut writer: AssetPackageWriter,
    ) -> Result<(), Box<dyn Error>> {
        let directory = path
            .parent()
            .ok_or_else(|| format!("Asset path: `{path:?}` has no parent directory!"))?;
        let name = match self.rename {
            Some(rename) => rename,
            None => path
                .to_path_buf()
                .with_extension("")
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        };
        let path = if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent}/{name}")
        };
        let bytes = std::fs::read(directory.join(&self.path))?;
        writer.write(path, bytes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexAssetCookRecipe(pub Vec<AssetPathStatic>);

impl AssetCookRecipe for IndexAssetCookRecipe {
    fn cook(
        self,
        path: &Path,
        parent: &str,
        mut writer: AssetPackageWriter,
    ) -> Result<(), Box<dyn Error>> {
        let path = path.to_path_buf().with_extension("");
        let name = path.file_name().unwrap().to_string_lossy();
        let path = if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent}/{name}")
        };
        let mut result = String::new();
        for item in self.0 {
            let protocol = item.protocol();
            let path = item.path();
            let path = if parent.is_empty() {
                path.to_string()
            } else {
                format!("{parent}/{path}")
            };
            result.push_str(&format!("{protocol}://{path}\n"));
        }
        writer.write(path, result.as_bytes())?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchiveAssetCookRecipe {
    Zip {
        paths: HashSet<PathBuf>,
        #[serde(default)]
        write_archive: bool,
    },
}

impl AssetCookRecipe for ArchiveAssetCookRecipe {
    fn cook(
        self,
        path: &Path,
        parent: &str,
        mut writer: AssetPackageWriter,
    ) -> Result<(), Box<dyn Error>> {
        match self {
            ArchiveAssetCookRecipe::Zip {
                paths,
                write_archive,
            } => {
                let path = path.to_path_buf().with_extension("");
                let directory = path
                    .parent()
                    .ok_or_else(|| format!("Asset path: `{path:?}` has no parent directory!"))?;
                let name = path.file_name().unwrap().to_string_lossy();
                let archive_path = if parent.is_empty() {
                    name.to_string()
                } else {
                    format!("{parent}/{name}")
                };
                let mut zip_buffer = Vec::new();
                let mut zip = zip::ZipWriter::new(Cursor::new(&mut zip_buffer));
                for file_path in paths {
                    let file_path = directory.join(&file_path);
                    let file_name = file_path
                        .file_name()
                        .ok_or_else(|| format!("Invalid file path: {:?}", &file_path))?
                        .to_string_lossy()
                        .to_string();
                    let file_content = std::fs::read(&file_path)
                        .map_err(|_| format!("Invalid file path: {:?}", &file_path))?;
                    zip.start_file(file_name, zip::write::SimpleFileOptions::default())?;
                    zip.write_all(&file_content)?;
                }
                zip.finish()?;
                writer.write(archive_path, &zip_buffer)?;
                if write_archive {
                    let archive_path = path.with_extension("zip");
                    std::fs::write(archive_path, zip_buffer)?;
                }
                Ok(())
            }
        }
    }
}
