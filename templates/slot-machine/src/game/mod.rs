pub mod machine;
pub mod states;

use self::states::gameplay::Gameplay;
use quaso::{assets::make_memory_database, config::Config, game::GameInstance, GameLauncher};

pub fn main() {
    GameLauncher::new(
        GameInstance::new(Gameplay::default()).setup_assets(|assets| {
            *assets = make_memory_database(include_bytes!("../../assets.pack")).unwrap();
        }),
    )
    .title("Quaso")
    .config(
        Config::load_from_str(include_str!("../../assets/GameConfig.toml"))
            .expect("Could not load Game Config!"),
    )
    .run();
}
