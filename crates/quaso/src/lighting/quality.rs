use crate::lighting::LightingConfig;

/// A cost level of the lighting, like the scalability levels of a big engine.
/// A level changes only cost settings. The look settings (ambient, sky, sun, exposure) stay as the game sets them.
/// `High` is the default config, so a game that never sets a level keeps its look.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LightingQuality {
    Low,
    Medium,
    #[default]
    High,
    Epic,
}

impl LightingQuality {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Epic];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Epic => "Epic",
        }
    }

    pub fn down(self) -> Option<Self> {
        match self {
            Self::Low => None,
            Self::Medium => Some(Self::Low),
            Self::High => Some(Self::Medium),
            Self::Epic => Some(Self::High),
        }
    }

    pub fn up(self) -> Option<Self> {
        match self {
            Self::Low => Some(Self::Medium),
            Self::Medium => Some(Self::High),
            Self::High => Some(Self::Epic),
            Self::Epic => None,
        }
    }

    /// Phones start at `Medium`, other devices at `High`.
    pub fn detect(is_touch_device: bool) -> Self {
        if is_touch_device {
            Self::Medium
        } else {
            Self::High
        }
    }

    pub fn resolution_scale(self) -> f32 {
        match self {
            Self::Low => 0.25,
            Self::Medium => 0.35,
            Self::High => 0.5,
            Self::Epic => 0.75,
        }
    }

    /// A wider probe spacing also widens the snap border of the scene canvas.
    /// At a low resolution scale that border costs more full size G-buffer pixels than the spacing saves.
    pub fn probe_spacing(self) -> u32 {
        2
    }

    pub fn margin(self) -> f32 {
        match self {
            Self::Low => 0.0,
            Self::Medium => 0.05,
            Self::High | Self::Epic => 0.1,
        }
    }

    pub fn bloom_levels(self) -> usize {
        match self {
            Self::Low => 3,
            Self::Medium => 4,
            Self::High | Self::Epic => 6,
        }
    }

    /// The cap of the web canvas pixel ratio. None keeps the device pixel ratio.
    pub fn max_pixel_ratio(self) -> Option<f64> {
        match self {
            Self::Low => Some(1.0),
            Self::Medium => Some(1.5),
            Self::High | Self::Epic => None,
        }
    }

    pub fn max_fps(self) -> Option<f32> {
        match self {
            Self::Low | Self::Medium => Some(30.0),
            Self::High | Self::Epic => None,
        }
    }

    pub fn target_frame_time(self) -> f32 {
        1.0 / self.max_fps().unwrap_or(60.0)
    }
}

impl LightingConfig {
    pub fn with_quality(mut self, quality: LightingQuality) -> Self {
        self.apply_quality(quality);
        self
    }

    pub fn apply_quality(&mut self, quality: LightingQuality) {
        self.resolution_scale = quality.resolution_scale();
        self.probe_spacing = quality.probe_spacing();
        self.margin = quality.margin();
        if let Some(bloom) = &mut self.bloom {
            bloom.levels = quality.bloom_levels();
        }
    }
}

/// Watches the frame time after the start and steps the level down while frames are too slow.
/// Feed it the real time between rendered frames, not the fixed game step.
#[derive(Debug, Clone)]
pub struct QualityGovernor {
    pub quality: LightingQuality,
    pub warm_up: f32,
    pub window: f32,
    pub budget_factor: f32,
    pub watch_time: f32,
    elapsed: f32,
    window_elapsed: f32,
    samples: Vec<f32>,
}

impl QualityGovernor {
    pub fn new(quality: LightingQuality) -> Self {
        Self {
            quality,
            warm_up: 1.0,
            window: 2.0,
            budget_factor: 1.25,
            watch_time: 15.0,
            elapsed: 0.0,
            window_elapsed: 0.0,
            samples: Vec::new(),
        }
    }

    pub fn is_watching(&self) -> bool {
        self.elapsed < self.watch_time && self.quality.down().is_some()
    }

    /// Returns the new level when the governor steps down.
    pub fn update(&mut self, frame_time: f32) -> Option<LightingQuality> {
        if !self.is_watching() {
            return None;
        }
        self.elapsed += frame_time;
        if self.elapsed < self.warm_up {
            return None;
        }
        self.samples.push(frame_time);
        self.window_elapsed += frame_time;
        if self.window_elapsed < self.window {
            return None;
        }
        self.samples.sort_by(f32::total_cmp);
        let median = self.samples[self.samples.len() / 2];
        self.samples.clear();
        self.window_elapsed = 0.0;
        let budget = self.quality.target_frame_time() * self.budget_factor;
        if median > budget {
            self.quality = self.quality.down()?;
            self.elapsed = self.elapsed.min(self.watch_time - self.window);
            return Some(self.quality);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(governor: &mut QualityGovernor, frame_time: f32, seconds: f32) -> Vec<LightingQuality> {
        let mut changes = Vec::new();
        let mut time = 0.0;
        while time < seconds {
            if let Some(quality) = governor.update(frame_time) {
                changes.push(quality);
            }
            time += frame_time;
        }
        changes
    }

    #[test]
    fn high_is_the_default_config() {
        assert_eq!(
            LightingConfig::default(),
            LightingConfig::default().with_quality(LightingQuality::High)
        );
    }

    #[test]
    fn quality_keeps_the_look_settings() {
        let mut config = LightingConfig {
            exposure: 3.0,
            margin: 0.3,
            ..Default::default()
        };
        config.apply_quality(LightingQuality::Low);
        assert_eq!(config.exposure, 3.0);
        assert_eq!(config.margin, 0.0);
        assert_eq!(config.resolution_scale, 0.25);
    }

    #[test]
    fn fast_frames_keep_the_level() {
        let mut governor = QualityGovernor::new(LightingQuality::High);
        assert!(feed(&mut governor, 1.0 / 60.0, 20.0).is_empty());
        assert_eq!(governor.quality, LightingQuality::High);
        assert!(!governor.is_watching());
    }

    #[test]
    fn slow_frames_step_down_one_level_per_window() {
        let mut governor = QualityGovernor::new(LightingQuality::High);
        let changes = feed(&mut governor, 1.0 / 20.0, 3.1);
        assert_eq!(changes, vec![LightingQuality::Medium]);
    }

    #[test]
    fn very_slow_frames_stop_at_low() {
        let mut governor = QualityGovernor::new(LightingQuality::Epic);
        let changes = feed(&mut governor, 0.5, 60.0);
        assert_eq!(
            changes,
            vec![
                LightingQuality::High,
                LightingQuality::Medium,
                LightingQuality::Low
            ]
        );
        assert!(!governor.is_watching());
    }
}
