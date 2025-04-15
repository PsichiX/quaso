pub mod drawables;
pub mod enemy;
pub mod item;
pub mod player;
pub mod states;
pub mod torch;
pub mod ui;
pub mod utils;

use self::states::preloader::Preloader;
use quaso::{assets::make_memory_database, config::Config, game::GameInstance, GameLauncher};

pub fn main() {
    GameLauncher::new(GameInstance::new(Preloader).setup_assets(|assets| {
        *assets = make_memory_database(include_bytes!("../../assets.pack")).unwrap();
    }))
    .title("RED HOOD")
    .config(
        Config::load_from_str(include_str!("../../assets/GameConfig.toml"))
            .expect("Could not load Game Config!"),
    )
    .run();
}
