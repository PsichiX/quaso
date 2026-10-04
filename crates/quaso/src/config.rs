use serde::{Deserialize, Serialize};
use spitfire_glow::app::AppConfig;
use std::{error::Error, path::Path};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub maximized: bool,
    pub vsync: bool,
    pub double_buffer: Option<bool>,
    pub hardware_acceleration: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: Self::default_width(),
            height: Self::default_height(),
            fullscreen: Self::default_fullscreen(),
            maximized: Default::default(),
            vsync: Self::default_vsync(),
            double_buffer: Default::default(),
            hardware_acceleration: Default::default(),
        }
    }
}

impl Config {
    fn default_width() -> u32 {
        1024
    }

    fn default_height() -> u32 {
        576
    }

    fn default_fullscreen() -> bool {
        true
    }

    fn default_vsync() -> bool {
        true
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        Self::load_from_str(&std::fs::read_to_string(path).unwrap_or_default())
    }

    pub fn load_from_str(content: &str) -> Result<Self, Box<dyn Error>> {
        Ok(toml::from_str(content)?)
    }

    pub fn to_app_config(&self, name: impl ToString) -> AppConfig {
        AppConfig {
            title: name.to_string(),
            width: self.width,
            height: self.height,
            fullscreen: self.fullscreen,
            maximized: self.maximized,
            vsync: self.vsync,
            double_buffer: self.double_buffer,
            hardware_acceleration: self.hardware_acceleration,
            color: [0.0, 0.0, 0.0, 0.0],
            ..Default::default()
        }
    }
}
