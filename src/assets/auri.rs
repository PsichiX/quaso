use ankha::{parser::AnkhaContentParser, script::AnkhaFile};
use anput::world::World;
use keket::{
    database::{handle::AssetHandle, path::AssetPathStatic},
    protocol::AssetProtocol,
};
use std::error::Error;

pub struct AuriAsset {
    pub file: AnkhaFile,
}

pub struct AuriAssetProtocol;

impl AssetProtocol for AuriAssetProtocol {
    fn name(&self) -> &str {
        "auri"
    }

    fn process_bytes(
        &mut self,
        handle: AssetHandle,
        storage: &mut World,
        bytes: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let path = storage.component::<true, AssetPathStatic>(handle.entity())?;
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| format!("Auri script: `{}` is not valid UTF-8!", path.path()))?;
        let file = AnkhaContentParser::default()
            .with_setup(ankha_auri::install)
            .parse_file_content(source)
            .map_err(|error| format!("Auri script: `{}` failed to parse! {error}", path.path()))?;
        drop(path);

        storage.insert(handle.entity(), (AuriAsset { file },))?;

        Ok(())
    }
}
