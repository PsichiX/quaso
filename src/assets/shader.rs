use crate::{assets::name_from_path, context::GameContext, game::GameSubsystem};
use anput::world::World;
use keket::{
    database::{handle::AssetHandle, path::AssetPathStatic},
    protocol::AssetProtocol,
};
use std::{any::Any, borrow::Cow, error::Error};

pub struct ShaderAsset {
    pub vertex: Cow<'static, str>,
    pub fragment: Cow<'static, str>,
}

impl ShaderAsset {
    pub fn new(vertex: &'static str, fragment: &'static str) -> Self {
        Self {
            vertex: vertex.into(),
            fragment: fragment.into(),
        }
    }
}

pub struct ShaderAssetSubsystem;

impl GameSubsystem for ShaderAssetSubsystem {
    fn update(&mut self, context: GameContext, _: f32) {
        for entity in context.assets.storage.added().iter_of::<ShaderAsset>() {
            if let Some((path, asset)) = context
                .assets
                .storage
                .lookup_one::<true, (&AssetPathStatic, &ShaderAsset)>(entity)
            {
                context.draw.shaders.insert(
                    name_from_path(&path).to_owned().into(),
                    context
                        .graphics
                        .shader(asset.vertex.trim(), asset.fragment.trim())
                        .unwrap(),
                );
            }
        }
        for entity in context.assets.storage.removed().iter_of::<ShaderAsset>() {
            if let Some(path) = context
                .assets
                .storage
                .lookup_one::<true, &AssetPathStatic>(entity)
            {
                context.draw.shaders.remove(name_from_path(&path));
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

pub struct ShaderAssetProtocol;

impl AssetProtocol for ShaderAssetProtocol {
    fn name(&self) -> &str {
        "shader"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        enum Mode {
            Vertex,
            Fragment,
        }

        let mut vertex = String::default();
        let mut fragment = String::default();
        let mut mode = Mode::Vertex;
        for line in std::str::from_utf8(&bytes)?.lines() {
            let trimmed = line.trim();
            if let Some(comment) = trimmed.strip_prefix("///") {
                let comment = comment.trim().to_lowercase();
                if comment == "[vertex]" {
                    mode = Mode::Vertex;
                    continue;
                }
                if comment == "[fragment]" {
                    mode = Mode::Fragment;
                    continue;
                }
            }
            match mode {
                Mode::Vertex => {
                    vertex.push_str(line);
                    vertex.push('\n');
                }
                Mode::Fragment => {
                    fragment.push_str(line);
                    fragment.push('\n');
                }
            }
        }

        storage.insert(
            handle.entity(),
            (ShaderAsset {
                vertex: vertex.into(),
                fragment: fragment.into(),
            },),
        )?;

        Ok(())
    }
}
