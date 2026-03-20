//! Project-specific theme management with automatic accent color generation.
//!
//! Themes can be defined per-project in `~/.config/surfterm/projects/*.toml`.
//! When no theme is configured for a project, an accent color is automatically
//! generated from the project's working directory path using `seahash`.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};
use tracing::{instrument, warn};

use crate::session::terminal::Rgb;

/// A color represented as RGB components.
///
/// Deserializes from hex strings like `"#1e1e2e"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[allow(dead_code)]
impl ThemeColor {
    /// Create a new `ThemeColor` from RGB components.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse a hex color string like `"#RRGGBB"`.
    ///
    /// Returns `None` if the string is not a valid 7-character hex color.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.len() != 7 || !s.starts_with('#') {
            return None;
        }
        let r = u8::from_str_radix(&s[1..3], 16).ok()?;
        let g = u8::from_str_radix(&s[3..5], 16).ok()?;
        let b = u8::from_str_radix(&s[5..7], 16).ok()?;
        Some(Self { r, g, b })
    }
}

impl From<ThemeColor> for Rgb {
    fn from(c: ThemeColor) -> Self {
        Rgb::new(c.r, c.g, c.b)
    }
}

/// Custom deserializer for `ThemeColor` that accepts `"#RRGGBB"` strings.
impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ThemeColorVisitor;

        impl<'de> Visitor<'de> for ThemeColorVisitor {
            type Value = ThemeColor;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex color string like \"#1e1e2e\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<ThemeColor, E>
            where
                E: de::Error,
            {
                ThemeColor::from_hex(value).ok_or_else(|| {
                    de::Error::invalid_value(
                        de::Unexpected::Str(value),
                        &"a hex color string like \"#1e1e2e\"",
                    )
                })
            }
        }

        deserializer.deserialize_str(ThemeColorVisitor)
    }
}

/// Project-specific theme configuration.
///
/// Deserializable from TOML files in `~/.config/surfterm/projects/`.
/// Defaults to Catppuccin Mocha colors.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
pub struct ProjectTheme {
    /// Accent color used for highlights, borders, active indicators.
    pub accent: ThemeColor,
    /// Background color for the terminal area.
    pub background: ThemeColor,
    /// Foreground (text) color.
    pub foreground: ThemeColor,
    /// Cursor color.
    pub cursor: ThemeColor,
}

impl Default for ProjectTheme {
    /// Default theme uses Catppuccin Mocha colors.
    fn default() -> Self {
        Self {
            accent: ThemeColor::new(0xf3, 0x8b, 0xa8),    // #f38ba8 (red/pink)
            background: ThemeColor::new(0x1e, 0x1e, 0x2e), // #1e1e2e
            foreground: ThemeColor::new(0xcd, 0xd6, 0xf4), // #cdd6f4
            cursor: ThemeColor::new(0xf5, 0xe0, 0xdc),     // #f5e0dc
        }
    }
}

/// Manages project themes, including loading from disk and auto-generating
/// accent colors for projects without explicit theme files.
#[allow(dead_code)]
pub struct ThemeManager {
    /// Explicitly configured themes keyed by project name.
    themes: HashMap<String, ProjectTheme>,
    /// Cached auto-generated themes for projects without explicit config.
    auto_themes: HashMap<String, ProjectTheme>,
}

#[allow(dead_code)]
impl ThemeManager {
    /// Create a new empty `ThemeManager`.
    pub fn new() -> Self {
        Self {
            themes: HashMap::new(),
            auto_themes: HashMap::new(),
        }
    }

    /// Load all project themes from `.toml` files in `config_dir/projects/`.
    ///
    /// Each file's stem (filename without extension) becomes the project name key.
    #[instrument(skip_all, fields(dir = %config_dir.display()))]
    pub fn load_themes(config_dir: &Path) -> Self {
        let projects_dir = config_dir.join("projects");
        let mut themes = HashMap::new();

        if projects_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&projects_dir) {
                let mut paths: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
                    .collect();
                paths.sort();

                for path in paths {
                    let project_name = match path.file_stem().and_then(|s| s.to_str()) {
                        Some(name) => name.to_string(),
                        None => continue,
                    };

                    match std::fs::read_to_string(&path) {
                        Ok(content) => match toml::from_str::<ProjectTheme>(&content) {
                            Ok(theme) => {
                                themes.insert(project_name, theme);
                            }
                            Err(e) => {
                                warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "failed to parse project theme file, skipping"
                                );
                            }
                        },
                        Err(e) => {
                            warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to read project theme file, skipping"
                            );
                        }
                    }
                }
            }
        }

        Self {
            themes,
            auto_themes: HashMap::new(),
        }
    }

    /// Get the theme for a project. If an explicit theme exists, return it.
    /// Otherwise, generate and cache an auto theme based on the cwd.
    pub fn get_theme(&mut self, project_name: &str, cwd: &Path) -> &ProjectTheme {
        if self.themes.contains_key(project_name) {
            return &self.themes[project_name];
        }

        let key = project_name.to_string();
        self.auto_themes.entry(key).or_insert_with(|| {
            let accent = Self::auto_accent(cwd);
            ProjectTheme {
                accent,
                ..ProjectTheme::default()
            }
        });

        &self.auto_themes[project_name]
    }

    /// Generate an accent color from a working directory path.
    ///
    /// Uses `seahash` to hash the path string, then maps the result to an HSL
    /// color with fixed saturation (0.7) and lightness (0.6) for good visibility.
    pub fn auto_accent(cwd: &Path) -> ThemeColor {
        let cwd_str = cwd.to_string_lossy();
        let hash = seahash::hash(cwd_str.as_bytes());
        let hue = (hash % 360) as f64;
        hsl_to_rgb(hue, 0.7, 0.6)
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert HSL color values to a `ThemeColor` (RGB).
///
/// - `h`: hue in degrees (0.0..360.0)
/// - `s`: saturation (0.0..1.0)
/// - `l`: lightness (0.0..1.0)
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> ThemeColor {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());

    let (r1, g1, b1) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let m = l - c / 2.0;
    let r = ((r1 + m) * 255.0).round() as u8;
    let g = ((g1 + m) * 255.0).round() as u8;
    let b = ((b1 + m) * 255.0).round() as u8;

    ThemeColor::new(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_default_theme_has_catppuccin_colors() {
        let theme = ProjectTheme::default();
        // Catppuccin Mocha background: #1e1e2e
        assert_eq!(theme.background, ThemeColor::new(0x1e, 0x1e, 0x2e));
        // Catppuccin Mocha foreground: #cdd6f4
        assert_eq!(theme.foreground, ThemeColor::new(0xcd, 0xd6, 0xf4));
        // Catppuccin Mocha cursor: #f5e0dc
        assert_eq!(theme.cursor, ThemeColor::new(0xf5, 0xe0, 0xdc));
        // Catppuccin Mocha accent: #f38ba8
        assert_eq!(theme.accent, ThemeColor::new(0xf3, 0x8b, 0xa8));
    }

    #[test]
    fn test_auto_accent_generates_valid_rgb() {
        let paths = [
            Path::new("/home/user/project-a"),
            Path::new("/home/user/project-b"),
            Path::new("/tmp/test"),
            Path::new("/var/data/workspace"),
        ];

        for path in &paths {
            let color = ThemeManager::auto_accent(path);
            // All RGB values are u8, so they're always 0..=255.
            // Just verify the color was produced (not all zeros unless it happens to be).
            let _ = (color.r, color.g, color.b);
        }
    }

    #[test]
    fn test_auto_accent_is_deterministic() {
        let path = Path::new("/home/user/my-project");
        let color1 = ThemeManager::auto_accent(path);
        let color2 = ThemeManager::auto_accent(path);
        assert_eq!(color1, color2);
    }

    #[test]
    fn test_auto_accent_produces_different_colors_for_different_paths() {
        let color_a = ThemeManager::auto_accent(Path::new("/home/user/project-alpha"));
        let color_b = ThemeManager::auto_accent(Path::new("/home/user/project-beta"));
        // It's theoretically possible for two paths to collide, but extremely unlikely
        assert_ne!(color_a, color_b);
    }

    #[test]
    fn test_hsl_to_rgb_red() {
        // Pure red: HSL(0, 1.0, 0.5) = RGB(255, 0, 0)
        let c = hsl_to_rgb(0.0, 1.0, 0.5);
        assert_eq!(c, ThemeColor::new(255, 0, 0));
    }

    #[test]
    fn test_hsl_to_rgb_green() {
        // Pure green: HSL(120, 1.0, 0.5) = RGB(0, 255, 0)
        let c = hsl_to_rgb(120.0, 1.0, 0.5);
        assert_eq!(c, ThemeColor::new(0, 255, 0));
    }

    #[test]
    fn test_hsl_to_rgb_blue() {
        // Pure blue: HSL(240, 1.0, 0.5) = RGB(0, 0, 255)
        let c = hsl_to_rgb(240.0, 1.0, 0.5);
        assert_eq!(c, ThemeColor::new(0, 0, 255));
    }

    #[test]
    fn test_hsl_to_rgb_white() {
        // White: HSL(0, 0.0, 1.0) = RGB(255, 255, 255)
        let c = hsl_to_rgb(0.0, 0.0, 1.0);
        assert_eq!(c, ThemeColor::new(255, 255, 255));
    }

    #[test]
    fn test_hsl_to_rgb_black() {
        // Black: HSL(0, 0.0, 0.0) = RGB(0, 0, 0)
        let c = hsl_to_rgb(0.0, 0.0, 0.0);
        assert_eq!(c, ThemeColor::new(0, 0, 0));
    }

    #[test]
    fn test_theme_color_from_hex_valid() {
        let c = ThemeColor::from_hex("#1e1e2e").unwrap();
        assert_eq!(c, ThemeColor::new(0x1e, 0x1e, 0x2e));

        let c = ThemeColor::from_hex("#ff00ff").unwrap();
        assert_eq!(c, ThemeColor::new(255, 0, 255));

        let c = ThemeColor::from_hex("#000000").unwrap();
        assert_eq!(c, ThemeColor::new(0, 0, 0));
    }

    #[test]
    fn test_theme_color_from_hex_invalid() {
        assert!(ThemeColor::from_hex("1e1e2e").is_none()); // missing #
        assert!(ThemeColor::from_hex("#1e1e2").is_none()); // too short
        assert!(ThemeColor::from_hex("#1e1e2e2e").is_none()); // too long
        assert!(ThemeColor::from_hex("#gggggg").is_none()); // invalid hex
        assert!(ThemeColor::from_hex("").is_none());
    }

    #[test]
    fn test_theme_color_deserialize_from_hex_string() {
        #[derive(Deserialize)]
        struct Wrapper {
            color: ThemeColor,
        }

        let toml_str = r##"color = "#f38ba8""##;
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(w.color, ThemeColor::new(0xf3, 0x8b, 0xa8));
    }

    #[test]
    fn test_theme_color_to_rgb_conversion() {
        let tc = ThemeColor::new(0xf3, 0x8b, 0xa8);
        let rgb: Rgb = tc.into();
        assert_eq!(rgb, Rgb::new(0xf3, 0x8b, 0xa8));
    }

    #[test]
    fn test_load_themes_from_directory() {
        let dir = tempdir("theme_load");
        let projects_dir = dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();

        fs::write(
            projects_dir.join("my-project.toml"),
            r##"
accent = "#ff0000"
background = "#000000"
foreground = "#ffffff"
cursor = "#00ff00"
"##,
        )
        .unwrap();

        let manager = ThemeManager::load_themes(&dir);
        let theme = manager.themes.get("my-project").unwrap();
        assert_eq!(theme.accent, ThemeColor::new(255, 0, 0));
        assert_eq!(theme.background, ThemeColor::new(0, 0, 0));
        assert_eq!(theme.foreground, ThemeColor::new(255, 255, 255));
        assert_eq!(theme.cursor, ThemeColor::new(0, 255, 0));
    }

    #[test]
    fn test_load_themes_partial_uses_defaults() {
        let dir = tempdir("theme_partial");
        let projects_dir = dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();

        // Only accent specified, rest should be defaults
        fs::write(
            projects_dir.join("partial.toml"),
            r##"accent = "#ff0000""##,
        )
        .unwrap();

        let manager = ThemeManager::load_themes(&dir);
        let theme = manager.themes.get("partial").unwrap();
        assert_eq!(theme.accent, ThemeColor::new(255, 0, 0));
        // Defaults for the rest
        assert_eq!(theme.background, ThemeColor::new(0x1e, 0x1e, 0x2e));
        assert_eq!(theme.foreground, ThemeColor::new(0xcd, 0xd6, 0xf4));
        assert_eq!(theme.cursor, ThemeColor::new(0xf5, 0xe0, 0xdc));
    }

    #[test]
    fn test_load_themes_skips_invalid_files() {
        let dir = tempdir("theme_invalid");
        let projects_dir = dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();

        fs::write(projects_dir.join("bad.toml"), "not valid [[[").unwrap();
        fs::write(
            projects_dir.join("good.toml"),
            r##"accent = "#00ff00""##,
        )
        .unwrap();

        let manager = ThemeManager::load_themes(&dir);
        assert!(manager.themes.get("bad").is_none());
        assert!(manager.themes.get("good").is_some());
    }

    #[test]
    fn test_get_theme_returns_explicit_theme() {
        let dir = tempdir("theme_get_explicit");
        let projects_dir = dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();

        fs::write(
            projects_dir.join("my-proj.toml"),
            r##"accent = "#abcdef""##,
        )
        .unwrap();

        let mut manager = ThemeManager::load_themes(&dir);
        let theme = manager.get_theme("my-proj", Path::new("/some/path"));
        assert_eq!(theme.accent, ThemeColor::new(0xab, 0xcd, 0xef));
    }

    #[test]
    fn test_get_theme_auto_generates_when_missing() {
        let mut manager = ThemeManager::new();
        let cwd = Path::new("/home/user/my-project");
        let theme = manager.get_theme("unknown-project", cwd);

        // Should have auto-generated accent but default background/foreground/cursor
        let expected_accent = ThemeManager::auto_accent(cwd);
        assert_eq!(theme.accent, expected_accent);
        assert_eq!(theme.background, ProjectTheme::default().background);
        assert_eq!(theme.foreground, ProjectTheme::default().foreground);
        assert_eq!(theme.cursor, ProjectTheme::default().cursor);
    }

    #[test]
    fn test_load_themes_empty_directory() {
        let dir = tempdir("theme_empty");
        let manager = ThemeManager::load_themes(&dir);
        assert!(manager.themes.is_empty());
    }

    /// Helper to create a unique temporary directory for each test.
    fn tempdir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("surfterm_test_{suffix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
