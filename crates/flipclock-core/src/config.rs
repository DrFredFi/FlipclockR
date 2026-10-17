//! `config.toml`: defaults, per-key validation, corrupt-file backup, atomic save.

use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml::Table;

use crate::color::Rgb;
use crate::date::DateLanguage;

pub const FILE_NAME: &str = "config.toml";
pub const BACKUP_FILE_NAME: &str = "config.toml.bak";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatePosition {
    Left,
    #[default]
    Center,
    Right,
}

impl DatePosition {
    pub const ALL: [DatePosition; 3] = [Self::Left, Self::Center, Self::Right];
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Config {
    pub clock: ClockConfig,
    pub animation: AnimationConfig,
    pub lighting: LightingConfig,
    pub sound: SoundConfig,
    pub layout: LayoutConfig,
    pub date: DateConfig,
    pub colors: ColorConfig,
    pub fonts: FontConfig,
    pub background: BackgroundConfig,
    pub window: WindowConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClockConfig {
    /// Seconds tile size relative to hour/minute tiles, 0.3–1.0.
    pub seconds_scale: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnimationConfig {
    pub flip_duration_ms: u32,
    pub cascade_delay_ms: u32,
    /// 0 (none) to 1 (strong).
    pub bounce: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LightingConfig {
    /// 0 matte to 1 glossy.
    pub finish: f32,
    /// Direction the key light comes from, in degrees counter-clockwise from the right.
    pub light_angle_deg: f32,
    pub light_intensity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundConfig {
    pub enabled: bool,
    pub volume: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayoutConfig {
    /// Uniform margin in logical pixels, 0–500.
    pub margin: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DateConfig {
    pub visible: bool,
    pub language: DateLanguage,
    pub position: DatePosition,
    /// Size multiplier, 0.5–2.0.
    pub size: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColorConfig {
    pub tile: Rgb,
    pub digits: Rgb,
    pub date: Rgb,
    pub background: Rgb,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FontConfig {
    /// Empty means the system default sans-serif font.
    pub digits_family: String,
    pub digits_weight: u16,
    pub date_family: String,
    pub date_weight: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackgroundConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    /// 0 (none) to 0.9.
    pub dim: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WindowConfig {
    /// Outer position in physical pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    /// Inner size in logical pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    pub maximized: bool,
    pub borderless: bool,
    pub always_on_top: bool,
}

pub const MIN_WINDOW_WIDTH: f64 = 480.0;
pub const MIN_WINDOW_HEIGHT: f64 = 270.0;

impl Default for Config {
    fn default() -> Self {
        Self {
            clock: ClockConfig { seconds_scale: 1.0 },
            animation: AnimationConfig {
                flip_duration_ms: 450,
                cascade_delay_ms: 60,
                bounce: 0.5,
            },
            lighting: LightingConfig {
                finish: 0.5,
                light_angle_deg: 120.0,
                light_intensity: 1.0,
            },
            sound: SoundConfig {
                enabled: true,
                volume: 0.5,
                custom_path: None,
            },
            layout: LayoutConfig { margin: 250.0 },
            date: DateConfig {
                visible: true,
                language: DateLanguage::Italian,
                position: DatePosition::Center,
                size: 1.0,
            },
            colors: ColorConfig {
                tile: Rgb::new(0x1c, 0x1c, 0x1e),
                digits: Rgb::new(0xf2, 0xf0, 0xea),
                date: Rgb::new(0xd8, 0xd6, 0xd0),
                background: Rgb::new(0x2a, 0x2d, 0x33),
            },
            fonts: FontConfig {
                digits_family: String::new(),
                digits_weight: 700,
                date_family: String::new(),
                date_weight: 400,
            },
            background: BackgroundConfig {
                image_path: None,
                dim: 0.0,
            },
            window: WindowConfig {
                x: None,
                y: None,
                width: None,
                height: None,
                maximized: false,
                borderless: false,
                always_on_top: false,
            },
        }
    }
}

impl Config {
    /// Clamp every value into its allowed range.
    pub fn validated(mut self) -> Self {
        let d = Config::default();
        self.clock.seconds_scale = clamp(self.clock.seconds_scale, 0.3, 1.0, d.clock.seconds_scale);
        let a = &mut self.animation;
        a.flip_duration_ms = a.flip_duration_ms.clamp(150, 1000);
        a.cascade_delay_ms = a.cascade_delay_ms.min(200);
        a.bounce = clamp(a.bounce, 0.0, 1.0, d.animation.bounce);
        let l = &mut self.lighting;
        l.finish = clamp(l.finish, 0.0, 1.0, d.lighting.finish);
        l.light_angle_deg = if l.light_angle_deg.is_finite() {
            l.light_angle_deg.rem_euclid(360.0)
        } else {
            d.lighting.light_angle_deg
        };
        l.light_intensity = clamp(l.light_intensity, 0.0, 2.0, d.lighting.light_intensity);
        self.sound.volume = clamp(self.sound.volume, 0.0, 1.0, d.sound.volume);
        self.layout.margin = clamp(self.layout.margin, 0.0, 500.0, d.layout.margin);
        self.date.size = clamp(self.date.size, 0.5, 2.0, d.date.size);
        self.fonts.digits_weight = self.fonts.digits_weight.clamp(100, 900);
        self.fonts.date_weight = self.fonts.date_weight.clamp(100, 900);
        self.background.dim = clamp(self.background.dim, 0.0, 0.9, d.background.dim);
        let w = &mut self.window;
        w.width = w
            .width
            .filter(|v| v.is_finite())
            .map(|v| v.max(MIN_WINDOW_WIDTH));
        w.height = w
            .height
            .filter(|v| v.is_finite())
            .map(|v| v.max(MIN_WINDOW_HEIGHT));
        self
    }

    /// Parse config text. Unknown keys are ignored; keys with an invalid value
    /// fall back to their default individually and are reported by name.
    pub fn parse(text: &str) -> Result<(Config, Vec<String>), toml::de::Error> {
        let root: Table = toml::from_str(text)?;
        let mut r = Reader {
            invalid: Vec::new(),
        };
        let d = Config::default();
        let s = r.section(&root, "clock");
        let clock = ClockConfig {
            seconds_scale: r.get(&s, "clock.seconds_scale", d.clock.seconds_scale),
        };
        let s = r.section(&root, "animation");
        let animation = AnimationConfig {
            flip_duration_ms: r.get(
                &s,
                "animation.flip_duration_ms",
                d.animation.flip_duration_ms,
            ),
            cascade_delay_ms: r.get(
                &s,
                "animation.cascade_delay_ms",
                d.animation.cascade_delay_ms,
            ),
            bounce: r.get(&s, "animation.bounce", d.animation.bounce),
        };
        let s = r.section(&root, "lighting");
        let lighting = LightingConfig {
            finish: r.get(&s, "lighting.finish", d.lighting.finish),
            light_angle_deg: r.get(&s, "lighting.light_angle_deg", d.lighting.light_angle_deg),
            light_intensity: r.get(&s, "lighting.light_intensity", d.lighting.light_intensity),
        };
        let s = r.section(&root, "sound");
        let sound = SoundConfig {
            enabled: r.get(&s, "sound.enabled", d.sound.enabled),
            volume: r.get(&s, "sound.volume", d.sound.volume),
            custom_path: r.get(&s, "sound.custom_path", d.sound.custom_path),
        };
        let s = r.section(&root, "layout");
        let layout = LayoutConfig {
            margin: r.get(&s, "layout.margin", d.layout.margin),
        };
        let s = r.section(&root, "date");
        let date = DateConfig {
            visible: r.get(&s, "date.visible", d.date.visible),
            language: r.get(&s, "date.language", d.date.language),
            position: r.get(&s, "date.position", d.date.position),
            size: r.get(&s, "date.size", d.date.size),
        };
        let s = r.section(&root, "colors");
        let colors = ColorConfig {
            tile: r.get(&s, "colors.tile", d.colors.tile),
            digits: r.get(&s, "colors.digits", d.colors.digits),
            date: r.get(&s, "colors.date", d.colors.date),
            background: r.get(&s, "colors.background", d.colors.background),
        };
        let s = r.section(&root, "fonts");
        let fonts = FontConfig {
            digits_family: r.get(&s, "fonts.digits_family", d.fonts.digits_family),
            digits_weight: r.get(&s, "fonts.digits_weight", d.fonts.digits_weight),
            date_family: r.get(&s, "fonts.date_family", d.fonts.date_family),
            date_weight: r.get(&s, "fonts.date_weight", d.fonts.date_weight),
        };
        let s = r.section(&root, "background");
        let background = BackgroundConfig {
            image_path: r.get(&s, "background.image_path", d.background.image_path),
            dim: r.get(&s, "background.dim", d.background.dim),
        };
        let s = r.section(&root, "window");
        let window = WindowConfig {
            x: r.get(&s, "window.x", d.window.x),
            y: r.get(&s, "window.y", d.window.y),
            width: r.get(&s, "window.width", d.window.width),
            height: r.get(&s, "window.height", d.window.height),
            maximized: r.get(&s, "window.maximized", d.window.maximized),
            borderless: r.get(&s, "window.borderless", d.window.borderless),
            always_on_top: r.get(&s, "window.always_on_top", d.window.always_on_top),
        };
        let config = Config {
            clock,
            animation,
            lighting,
            sound,
            layout,
            date,
            colors,
            fonts,
            background,
            window,
        }
        .validated();
        Ok((config, r.invalid))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("config always serializes")
    }
}

fn clamp(v: f32, min: f32, max: f32, default: f32) -> f32 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        default
    }
}

struct Reader {
    invalid: Vec<String>,
}

impl Reader {
    fn section(&mut self, root: &Table, name: &str) -> Table {
        match root.get(name) {
            None => Table::new(),
            Some(toml::Value::Table(t)) => t.clone(),
            Some(_) => {
                self.invalid.push(name.to_owned());
                Table::new()
            }
        }
    }

    fn get<T: DeserializeOwned>(&mut self, section: &Table, key: &str, default: T) -> T {
        let field = key.rsplit('.').next().unwrap_or(key);
        match section.get(field) {
            None => default,
            Some(v) => v.clone().try_into().unwrap_or_else(|_| {
                self.invalid.push(key.to_owned());
                default
            }),
        }
    }
}

/// Outcome of reading `config.toml` from disk.
#[derive(Debug)]
pub enum LoadOutcome {
    /// No file yet: defaults.
    Missing,
    /// Parsed; the listed keys had invalid values and use defaults.
    Loaded { invalid_keys: Vec<String> },
    /// Not valid TOML: defaults; the file was copied to `config.toml.bak` (or the copy failed).
    Corrupt { backup: io::Result<PathBuf> },
    /// The file exists but could not be read: defaults.
    Unreadable(io::Error),
}

pub fn load(dir: &Path) -> (Config, LoadOutcome) {
    let path = dir.join(FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return (Config::default(), LoadOutcome::Missing);
        }
        Err(e) => return (Config::default(), LoadOutcome::Unreadable(e)),
    };
    match Config::parse(&text) {
        Ok((config, invalid_keys)) => (config, LoadOutcome::Loaded { invalid_keys }),
        Err(_) => {
            let backup_path = dir.join(BACKUP_FILE_NAME);
            let backup = std::fs::copy(&path, &backup_path).map(|_| backup_path);
            (Config::default(), LoadOutcome::Corrupt { backup })
        }
    }
}

/// Write `config.toml` atomically: temp file in the same folder, then rename.
pub fn save(dir: &Path, config: &Config) -> io::Result<()> {
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    std::fs::write(&tmp, config.to_toml())?;
    std::fs::rename(&tmp, dir.join(FILE_NAME)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The folder that holds `config.toml` (ADR-0003): next to the `.AppImage` file when
/// running as an AppImage, next to the `.app` bundle on macOS, otherwise next to the executable.
pub fn config_dir(exe: &Path, appimage: Option<&Path>) -> PathBuf {
    if let Some(image) = appimage
        && let Some(parent) = image.parent()
    {
        return parent.to_path_buf();
    }
    let mut dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    // .../Flipclock.app/Contents/MacOS/flipclock -> folder containing Flipclock.app
    if dir.file_name().is_some_and(|n| n == "MacOS")
        && let Some(contents) = dir
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
        && let Some(bundle) = contents
            .parent()
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
        && let Some(outer) = bundle.parent()
    {
        dir = outer.to_path_buf();
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("flipclock-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_round_trip_through_toml() {
        let c = Config::default();
        let (parsed, invalid) = Config::parse(&c.to_toml()).unwrap();
        assert_eq!(parsed, c);
        assert!(invalid.is_empty());
    }

    #[test]
    fn missing_keys_use_defaults() {
        let (c, invalid) = Config::parse("[animation]\nflip_duration_ms = 600\n").unwrap();
        assert_eq!(c.animation.flip_duration_ms, 600);
        assert_eq!(c.animation.cascade_delay_ms, 60);
        assert_eq!(c.layout.margin, 250.0);
        assert!(invalid.is_empty());
    }

    #[test]
    fn invalid_values_revert_individually() {
        let text = "[colors]\ntile = \"red\"\ndigits = \"#ffffff\"\n[date]\nlanguage = \"fr-FR\"\nsize = 1.5\n";
        let (c, invalid) = Config::parse(text).unwrap();
        assert_eq!(c.colors.tile, Config::default().colors.tile);
        assert_eq!(c.colors.digits, Rgb::new(255, 255, 255));
        assert_eq!(c.date.language, DateLanguage::Italian);
        assert_eq!(c.date.size, 1.5);
        assert_eq!(invalid, vec!["date.language", "colors.tile"]);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let text = "[animation]\nflip_duration_ms = 5000\ncascade_delay_ms = 999\n[layout]\nmargin = -10.0\n[clock]\nseconds_scale = 0.1\n";
        let (c, _) = Config::parse(text).unwrap();
        assert_eq!(c.animation.flip_duration_ms, 1000);
        assert_eq!(c.animation.cascade_delay_ms, 200);
        assert_eq!(c.layout.margin, 0.0);
        assert_eq!(c.clock.seconds_scale, 0.3);
    }

    #[test]
    fn integer_literals_are_accepted_for_float_settings() {
        let (c, invalid) = Config::parse("[layout]\nmargin = 120\n").unwrap();
        assert!(invalid.is_empty(), "{invalid:?}");
        assert_eq!(c.layout.margin, 120.0);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_defaults_are_used() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join(FILE_NAME), "this is [not toml").unwrap();
        let (c, outcome) = load(&dir);
        assert_eq!(c, Config::default());
        assert!(matches!(outcome, LoadOutcome::Corrupt { backup: Ok(_) }));
        assert_eq!(
            std::fs::read_to_string(dir.join(BACKUP_FILE_NAME)).unwrap(),
            "this is [not toml"
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("save");
        let mut c = Config::default();
        c.window.x = Some(-1200);
        c.window.width = Some(900.0);
        c.sound.custom_path = Some("C:\\sounds\\tick.mp3".into());
        save(&dir, &c).unwrap();
        save(&dir, &c).unwrap();
        let (loaded, outcome) = load(&dir);
        assert!(matches!(outcome, LoadOutcome::Loaded { .. }));
        assert_eq!(loaded, c);
        assert!(!dir.join("config.toml.tmp").exists());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        let (c, outcome) = load(&dir);
        assert_eq!(c, Config::default());
        assert!(matches!(outcome, LoadOutcome::Missing));
    }

    #[test]
    fn config_dir_is_next_to_the_executable() {
        let exe = Path::new("/opt/flipclock/flipclock");
        assert_eq!(config_dir(exe, None), Path::new("/opt/flipclock"));
    }

    #[test]
    fn config_dir_is_next_to_the_appimage() {
        let exe = Path::new("/tmp/.mount_abc/usr/bin/flipclock");
        let image = Path::new("/home/me/Apps/Flipclock.AppImage");
        assert_eq!(config_dir(exe, Some(image)), Path::new("/home/me/Apps"));
    }

    #[test]
    fn config_dir_is_next_to_the_app_bundle() {
        let exe = Path::new("/Users/me/Flipclock/Flipclock.app/Contents/MacOS/flipclock");
        assert_eq!(config_dir(exe, None), Path::new("/Users/me/Flipclock"));
    }
}
