use kira::{
    AudioManager, AudioManagerSettings,
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
};
use std::collections::HashMap;

pub struct Audio {
    pub manager: AudioManager,
    pub sounds: HashMap<String, StaticSoundData>,
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            manager: AudioManager::new(AudioManagerSettings::default()).unwrap(),
            sounds: Default::default(),
        }
    }
}

impl Audio {
    pub fn play(&mut self, id: &str) -> Option<StaticSoundHandle> {
        self.manager.play(self.sounds.get(id)?.clone()).ok()
    }
}
