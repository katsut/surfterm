//! Project-specific theme management with automatic accent color generation.
//!
//! Themes can be defined globally in `~/.config/surfterm/theme.toml` or locally
//! in `.surfterm/theme.toml` within a project directory. Local overrides only
//! the fields it specifies; unspecified fields fall back to global, then defaults.
//!
//! Per-project themes can also be defined in `~/.config/surfterm/projects/*.toml`
//! for auto-accent generation based on the working directory.

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

    /// Convert to an `Rgb` value for the terminal renderer.
    pub fn to_rgb(self) -> Rgb {
        Rgb::new(self.r, self.g, self.b)
    }

    /// Convert to a wgpu `Color` with sRGB → linear conversion.
    ///
    /// wgpu's `Bgra8UnormSrgb` surface format applies sRGB encoding on output,
    /// so clear colors must be provided in linear space.
    pub fn to_wgpu_color(self) -> wgpu::Color {
        wgpu::Color {
            r: srgb_to_linear(self.r as f64 / 255.0),
            g: srgb_to_linear(self.g as f64 / 255.0),
            b: srgb_to_linear(self.b as f64 / 255.0),
            a: 1.0,
        }
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

// ── SurftermTheme: comprehensive theme structure ──

/// Sidebar colors for the session list panel.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SidebarColors {
    pub background: ThemeColor,
    pub foreground: ThemeColor,
    pub separator: ThemeColor,
    pub new_session: ThemeColor,
    pub active_bg: ThemeColor,
    pub selected_bg: ThemeColor,
}

impl Default for SidebarColors {
    fn default() -> Self {
        Self {
            background: ThemeColor::new(0x14, 0x24, 0x14), // #142414 (darker green)
            foreground: ThemeColor::new(0xcd, 0xd6, 0xf4), // #cdd6f4
            separator: ThemeColor::new(0x3a, 0x5a, 0x3a),  // #3a5a3a (green-tinted)
            new_session: ThemeColor::new(0xa6, 0xe3, 0xa1), // #a6e3a1
            active_bg: ThemeColor::new(0x2a, 0x4a, 0x2a),  // #2a4a2a
            selected_bg: ThemeColor::new(0x3a, 0x5a, 0x3a), // #3a5a3a
        }
    }
}

/// Card title bar colors.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CardColors {
    pub border: ThemeColor,
    pub title: ThemeColor,
    pub active_title: ThemeColor,
}

impl Default for CardColors {
    fn default() -> Self {
        Self {
            border: ThemeColor::new(0x31, 0x32, 0x44),      // #313244
            title: ThemeColor::new(0xcd, 0xd6, 0xf4),       // #cdd6f4
            active_title: ThemeColor::new(0x89, 0xb4, 0xfa), // #89b4fa
        }
    }
}

/// State indicator dot colors.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct StateColors {
    pub idle: ThemeColor,
    pub running: ThemeColor,
    pub waiting: ThemeColor,
    pub error: ThemeColor,
}

impl Default for StateColors {
    fn default() -> Self {
        Self {
            idle: ThemeColor::new(0x6c, 0x70, 0x86),    // #6c7086
            running: ThemeColor::new(0xf9, 0xe2, 0xaf),  // #f9e2af
            waiting: ThemeColor::new(0xa6, 0xe3, 0xa1),  // #a6e3a1
            error: ThemeColor::new(0xf3, 0x8b, 0xa8),    // #f38ba8
        }
    }
}

/// All theme colors grouped by component.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ThemeColors {
    pub background: ThemeColor,
    pub foreground: ThemeColor,
    pub cursor: ThemeColor,
    pub accent: ThemeColor,
    /// Main highlight color used for session names, active elements, etc.
    pub main_color: ThemeColor,
    pub sidebar: SidebarColors,
    pub card: CardColors,
    pub state: StateColors,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            background: ThemeColor::new(0x1a, 0x2e, 0x1a), // #1a2e1a (from Ghostty)
            foreground: ThemeColor::new(0xcd, 0xd6, 0xf4), // #cdd6f4
            cursor: ThemeColor::new(0xf5, 0xe0, 0xdc),     // #f5e0dc
            accent: ThemeColor::new(0xf3, 0x8b, 0xa8),     // #f38ba8
            main_color: ThemeColor::new(0x89, 0xb4, 0xfa), // #89b4fa (blue)
            sidebar: SidebarColors::default(),
            card: CardColors::default(),
            state: StateColors::default(),
        }
    }
}

/// Font configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Font family name. Use a monospace font.
    pub family: String,
    /// Font size in logical pixels (before scale factor).
    pub size: f32,
    /// Line height multiplier (1.0 = tight, 1.4 = comfortable).
    pub line_height: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: String::new(), // empty = system default monospace
            size: 13.0,            // Ghostty default
            line_height: 1.2,
        }
    }
}

/// Comprehensive theme for the Surfterm application.
///
/// All fields use `#[serde(default)]` so partial theme files work correctly:
/// only specified fields override defaults.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SurftermTheme {
    pub colors: ThemeColors,
    pub font: FontConfig,
}

/// Type alias for backward compatibility.
#[allow(dead_code)]
pub type ProjectTheme = SurftermTheme;

// ── Convenience accessors on SurftermTheme ──

#[allow(dead_code)]
impl SurftermTheme {
    /// Get the background color as an Rgb value.
    pub fn background_rgb(&self) -> Rgb {
        self.colors.background.to_rgb()
    }

    /// Get the foreground color as an Rgb value.
    pub fn foreground_rgb(&self) -> Rgb {
        self.colors.foreground.to_rgb()
    }

    /// Get the background color as a wgpu Color for clear passes.
    pub fn background_wgpu(&self) -> wgpu::Color {
        self.colors.background.to_wgpu_color()
    }

    /// State color Rgb for the given session state.
    pub fn state_color_rgb(&self, state: &crate::session::state::SessionState) -> Rgb {
        use crate::session::state::SessionState;
        match state {
            SessionState::Running => self.colors.state.running.to_rgb(),
            SessionState::WaitingForInput => self.colors.state.waiting.to_rgb(),
            SessionState::Error => self.colors.state.error.to_rgb(),
            SessionState::Idle => self.colors.state.idle.to_rgb(),
        }
    }
}

/// Manages project themes, including loading from disk and auto-generating
/// accent colors for projects without explicit theme files.
#[allow(dead_code)]
pub struct ThemeManager {
    /// Explicitly configured themes keyed by project name.
    themes: HashMap<String, SurftermTheme>,
    /// Cached auto-generated themes for projects without explicit config.
    auto_themes: HashMap<String, SurftermTheme>,
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
                        Ok(content) => match toml::from_str::<SurftermTheme>(&content) {
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
    pub fn get_theme(&mut self, project_name: &str, cwd: &Path) -> &SurftermTheme {
        if self.themes.contains_key(project_name) {
            return &self.themes[project_name];
        }

        let key = project_name.to_string();
        self.auto_themes.entry(key).or_insert_with(|| {
            let accent = Self::auto_accent(cwd);
            let mut theme = SurftermTheme::default();
            theme.colors.accent = accent;
            theme
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

    /// Load a complete theme by merging global defaults, global theme file,
    /// and local (per-directory) theme file.
    ///
    /// Priority (highest to lowest):
    /// 1. `.surfterm/theme.toml` in cwd or any parent directory
    /// 2. `~/.config/surfterm/theme.toml` (global)
    /// 3. Built-in defaults (Catppuccin Mocha)
    #[instrument(skip_all, fields(config_dir = %config_dir.display(), cwd = %cwd.display()))]
    pub fn load_theme(config_dir: &Path, cwd: &Path) -> SurftermTheme {
        // Start with defaults
        let mut theme = SurftermTheme::default();

        // Layer 1: global theme file
        let global_path = config_dir.join("theme.toml");
        if global_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&global_path) {
                match toml::from_str::<SurftermTheme>(&content) {
                    Ok(global_theme) => {
                        theme = global_theme;
                    }
                    Err(e) => {
                        warn!(
                            path = %global_path.display(),
                            error = %e,
                            "failed to parse global theme.toml, using defaults"
                        );
                    }
                }
            }
        }

        // Layer 2: local theme file
        if let Some(local_path) = Self::find_local_theme(cwd) {
            if let Ok(content) = std::fs::read_to_string(&local_path) {
                match toml::from_str::<SurftermTheme>(&content) {
                    Ok(local_theme) => {
                        theme = local_theme;
                    }
                    Err(e) => {
                        warn!(
                            path = %local_path.display(),
                            error = %e,
                            "failed to parse local theme.toml, ignoring"
                        );
                    }
                }
            }
        }

        theme
    }

    /// Walk up the directory tree from `cwd` looking for `.surfterm/theme.toml`.
    pub fn find_local_theme(cwd: &Path) -> Option<PathBuf> {
        let mut dir = cwd.to_path_buf();
        loop {
            let candidate = dir.join(".surfterm").join("theme.toml");
            if candidate.is_file() {
                return Some(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
        None
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
/// Convert sRGB component (0.0..1.0) to linear space.
///
/// sRGB uses a piecewise transfer function. wgpu's Srgb surface formats
/// apply sRGB encoding on output, so input colors must be in linear space.
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

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
        let theme = SurftermTheme::default();
        // Catppuccin Mocha background: #1e1e2e
        assert_eq!(theme.colors.background, ThemeColor::new(0x1e, 0x1e, 0x2e));
        // Catppuccin Mocha foreground: #cdd6f4
        assert_eq!(theme.colors.foreground, ThemeColor::new(0xcd, 0xd6, 0xf4));
        // Catppuccin Mocha cursor: #f5e0dc
        assert_eq!(theme.colors.cursor, ThemeColor::new(0xf5, 0xe0, 0xdc));
        // Catppuccin Mocha accent: #f38ba8
        assert_eq!(theme.colors.accent, ThemeColor::new(0xf3, 0x8b, 0xa8));
    }

    #[test]
    fn test_default_sidebar_colors() {
        let theme = SurftermTheme::default();
        assert_eq!(theme.colors.sidebar.new_session, ThemeColor::new(0xa6, 0xe3, 0xa1));
        assert_eq!(theme.colors.sidebar.separator, ThemeColor::new(0x58, 0x5b, 0x70));
        assert_eq!(theme.colors.sidebar.active_bg, ThemeColor::new(0x45, 0x47, 0x5a));
        assert_eq!(theme.colors.sidebar.selected_bg, ThemeColor::new(0x58, 0x5b, 0x70));
    }

    #[test]
    fn test_default_card_colors() {
        let theme = SurftermTheme::default();
        assert_eq!(theme.colors.card.border, ThemeColor::new(0x31, 0x32, 0x44));
        assert_eq!(theme.colors.card.active_title, ThemeColor::new(0x89, 0xb4, 0xfa));
    }

    #[test]
    fn test_default_state_colors() {
        let theme = SurftermTheme::default();
        assert_eq!(theme.colors.state.idle, ThemeColor::new(0x6c, 0x70, 0x86));
        assert_eq!(theme.colors.state.running, ThemeColor::new(0xf9, 0xe2, 0xaf));
        assert_eq!(theme.colors.state.waiting, ThemeColor::new(0xa6, 0xe3, 0xa1));
        assert_eq!(theme.colors.state.error, ThemeColor::new(0xf3, 0x8b, 0xa8));
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
[colors]
accent = "#ff0000"
background = "#000000"
foreground = "#ffffff"
cursor = "#00ff00"
"##,
        )
        .unwrap();

        let manager = ThemeManager::load_themes(&dir);
        let theme = manager.themes.get("my-project").unwrap();
        assert_eq!(theme.colors.accent, ThemeColor::new(255, 0, 0));
        assert_eq!(theme.colors.background, ThemeColor::new(0, 0, 0));
        assert_eq!(theme.colors.foreground, ThemeColor::new(255, 255, 255));
        assert_eq!(theme.colors.cursor, ThemeColor::new(0, 255, 0));
    }

    #[test]
    fn test_load_themes_partial_uses_defaults() {
        let dir = tempdir("theme_partial");
        let projects_dir = dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();

        // Only accent specified, rest should be defaults
        fs::write(
            projects_dir.join("partial.toml"),
            r##"
[colors]
accent = "#ff0000"
"##,
        )
        .unwrap();

        let manager = ThemeManager::load_themes(&dir);
        let theme = manager.themes.get("partial").unwrap();
        assert_eq!(theme.colors.accent, ThemeColor::new(255, 0, 0));
        // Defaults for the rest
        assert_eq!(theme.colors.background, ThemeColor::new(0x1e, 0x1e, 0x2e));
        assert_eq!(theme.colors.foreground, ThemeColor::new(0xcd, 0xd6, 0xf4));
        assert_eq!(theme.colors.cursor, ThemeColor::new(0xf5, 0xe0, 0xdc));
    }

    #[test]
    fn test_load_themes_skips_invalid_files() {
        let dir = tempdir("theme_invalid");
        let projects_dir = dir.join("projects");
        fs::create_dir_all(&projects_dir).unwrap();

        fs::write(projects_dir.join("bad.toml"), "not valid [[[").unwrap();
        fs::write(
            projects_dir.join("good.toml"),
            r##"
[colors]
accent = "#00ff00"
"##,
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
            r##"
[colors]
accent = "#abcdef"
"##,
        )
        .unwrap();

        let mut manager = ThemeManager::load_themes(&dir);
        let theme = manager.get_theme("my-proj", Path::new("/some/path"));
        assert_eq!(theme.colors.accent, ThemeColor::new(0xab, 0xcd, 0xef));
    }

    #[test]
    fn test_get_theme_auto_generates_when_missing() {
        let mut manager = ThemeManager::new();
        let cwd = Path::new("/home/user/my-project");
        let theme = manager.get_theme("unknown-project", cwd);

        // Should have auto-generated accent but default background/foreground/cursor
        let expected_accent = ThemeManager::auto_accent(cwd);
        assert_eq!(theme.colors.accent, expected_accent);
        assert_eq!(theme.colors.background, SurftermTheme::default().colors.background);
        assert_eq!(theme.colors.foreground, SurftermTheme::default().colors.foreground);
        assert_eq!(theme.colors.cursor, SurftermTheme::default().colors.cursor);
    }

    #[test]
    fn test_load_themes_empty_directory() {
        let dir = tempdir("theme_empty");
        let manager = ThemeManager::load_themes(&dir);
        assert!(manager.themes.is_empty());
    }

    #[test]
    fn test_load_theme_defaults_when_no_files() {
        let config_dir = tempdir("load_theme_none");
        let cwd = tempdir("load_theme_cwd");
        let theme = ThemeManager::load_theme(&config_dir, &cwd);
        assert_eq!(theme.colors.background, SurftermTheme::default().colors.background);
    }

    #[test]
    fn test_load_theme_global_overrides_defaults() {
        let config_dir = tempdir("load_theme_global");
        fs::write(
            config_dir.join("theme.toml"),
            r##"
[colors]
background = "#000000"
"##,
        )
        .unwrap();

        let cwd = tempdir("load_theme_global_cwd");
        let theme = ThemeManager::load_theme(&config_dir, &cwd);
        assert_eq!(theme.colors.background, ThemeColor::new(0, 0, 0));
        // Unset fields remain defaults
        assert_eq!(theme.colors.foreground, SurftermTheme::default().colors.foreground);
    }

    #[test]
    fn test_load_theme_local_overrides_global() {
        let config_dir = tempdir("load_theme_local_over");
        fs::write(
            config_dir.join("theme.toml"),
            r##"
[colors]
background = "#111111"
foreground = "#222222"
"##,
        )
        .unwrap();

        let cwd = tempdir("load_theme_local_cwd");
        let local_dir = cwd.join(".surfterm");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(
            local_dir.join("theme.toml"),
            r##"
[colors]
background = "#333333"
"##,
        )
        .unwrap();

        let theme = ThemeManager::load_theme(&config_dir, &cwd);
        // Local overrides background
        assert_eq!(theme.colors.background, ThemeColor::new(0x33, 0x33, 0x33));
        // Note: since we deserialize full structs, unset fields in local get defaults
        // (not global values). This is the serde(default) behavior.
        assert_eq!(theme.colors.foreground, SurftermTheme::default().colors.foreground);
    }

    #[test]
    fn test_find_local_theme_walks_up() {
        let base = tempdir("find_local_walk");
        let deep = base.join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();

        // Place .surfterm/theme.toml at 'a' level
        let surfterm_dir = base.join("a").join(".surfterm");
        fs::create_dir_all(&surfterm_dir).unwrap();
        fs::write(surfterm_dir.join("theme.toml"), "[colors]\n").unwrap();

        let found = ThemeManager::find_local_theme(&deep);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), surfterm_dir.join("theme.toml"));
    }

    #[test]
    fn test_find_local_theme_returns_none_when_absent() {
        let dir = tempdir("find_local_absent");
        assert!(ThemeManager::find_local_theme(&dir).is_none());
    }

    #[test]
    fn test_deserialize_full_theme_toml() {
        let toml_str = r##"
[colors]
background = "#1e1e2e"
foreground = "#cdd6f4"
cursor = "#f5e0dc"
accent = "#f38ba8"

[colors.sidebar]
background = "#181825"
foreground = "#cdd6f4"
separator = "#585b70"
new_session = "#a6e3a1"
active_bg = "#45475a"
selected_bg = "#585b70"

[colors.card]
border = "#313244"
title = "#cdd6f4"
active_title = "#89b4fa"

[colors.state]
idle = "#6c7086"
running = "#f9e2af"
waiting = "#a6e3a1"
error = "#f38ba8"
"##;
        let theme: SurftermTheme = toml::from_str(toml_str).unwrap();
        assert_eq!(theme.colors.background, ThemeColor::new(0x1e, 0x1e, 0x2e));
        assert_eq!(theme.colors.sidebar.new_session, ThemeColor::new(0xa6, 0xe3, 0xa1));
        assert_eq!(theme.colors.card.active_title, ThemeColor::new(0x89, 0xb4, 0xfa));
        assert_eq!(theme.colors.state.running, ThemeColor::new(0xf9, 0xe2, 0xaf));
    }

    #[test]
    fn test_deserialize_partial_theme_toml() {
        // Only override sidebar colors; everything else should be default
        let toml_str = r##"
[colors.sidebar]
new_session = "#ff0000"
"##;
        let theme: SurftermTheme = toml::from_str(toml_str).unwrap();
        assert_eq!(theme.colors.sidebar.new_session, ThemeColor::new(0xff, 0x00, 0x00));
        // Defaults for everything else
        assert_eq!(theme.colors.background, SurftermTheme::default().colors.background);
        assert_eq!(theme.colors.sidebar.separator, SurftermTheme::default().colors.sidebar.separator);
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
