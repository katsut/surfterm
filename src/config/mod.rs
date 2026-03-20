use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::{instrument, warn};

use crate::detector::patterns::{self, StatePattern};

/// Global application configuration deserialized from `config.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
pub struct SurftermConfig {
    pub font_size: f32,
    pub panel_ratio: f32,
    pub max_scroll_lines: usize,
    pub ble_enabled: bool,
    pub llm_model_path: Option<String>,
}

impl Default for SurftermConfig {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            panel_ratio: 0.7,
            max_scroll_lines: 10000,
            ble_enabled: false,
            llm_model_path: None,
        }
    }
}

/// Keybind configuration deserialized from `keybinds.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
pub struct KeybindsConfig {
    pub prefix_key: Option<String>,
    #[serde(default)]
    pub overrides: HashMap<String, String>,
}

/// Central configuration engine that loads and holds all config state.
#[allow(dead_code)]
pub struct ConfigEngine {
    config: SurftermConfig,
    keybinds: KeybindsConfig,
    config_dir: PathBuf,
}

#[allow(dead_code)]
impl ConfigEngine {
    /// Create a new `ConfigEngine` loading from the default config directory
    /// (`~/.config/surfterm/`). Falls back to defaults if the directory or
    /// files do not exist.
    #[instrument]
    pub fn new() -> Self {
        let config_dir = dirs_config_dir();
        Self::load(&config_dir)
    }

    /// Load configuration from a specific directory.
    #[instrument(skip_all, fields(dir = %config_dir.display()))]
    pub fn load(config_dir: &Path) -> Self {
        let config = Self::load_config_file(&config_dir.join("config.toml"))
            .unwrap_or_default();
        let keybinds = Self::load_keybinds_file(&config_dir.join("keybinds.toml"))
            .unwrap_or_default();

        Self {
            config,
            keybinds,
            config_dir: config_dir.to_path_buf(),
        }
    }

    /// Access the global configuration.
    pub fn config(&self) -> &SurftermConfig {
        &self.config
    }

    /// Access the keybind configuration.
    pub fn keybinds(&self) -> &KeybindsConfig {
        &self.keybinds
    }

    /// Load state-detection patterns from all `.toml` files in the
    /// `detectors/` subdirectory, merged with the built-in defaults.
    ///
    /// User-defined patterns are prepended so they take priority over defaults.
    #[instrument(skip(self))]
    pub fn load_detector_patterns(&self) -> Vec<StatePattern> {
        let detectors_dir = self.config_dir.join("detectors");
        let mut user_patterns: Vec<StatePattern> = Vec::new();

        if detectors_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&detectors_dir) {
                let mut paths: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
                    .collect();
                paths.sort();

                for path in paths {
                    match std::fs::read_to_string(&path) {
                        Ok(content) => match patterns::load_patterns_from_toml(&content) {
                            Ok(loaded) => user_patterns.extend(loaded),
                            Err(e) => {
                                warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "failed to parse detector patterns file, skipping"
                                );
                            }
                        },
                        Err(e) => {
                            warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to read detector patterns file, skipping"
                            );
                        }
                    }
                }
            }
        }

        let defaults = patterns::default_claude_code_state_patterns();
        user_patterns.extend(defaults);
        user_patterns
    }

    /// Attempt to load and parse a `config.toml` file. Returns `None` on any
    /// error (file missing, parse failure), logging a warning for parse errors.
    fn load_config_file(path: &Path) -> Option<SurftermConfig> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return None,
        };
        match toml::from_str::<SurftermConfig>(&content) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to parse config.toml, using defaults"
                );
                None
            }
        }
    }

    /// Attempt to load and parse a `keybinds.toml` file. Returns `None` on any
    /// error, logging a warning for parse errors.
    fn load_keybinds_file(path: &Path) -> Option<KeybindsConfig> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return None,
        };
        match toml::from_str::<KeybindsConfig>(&content) {
            Ok(kb) => Some(kb),
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to parse keybinds.toml, using defaults"
                );
                None
            }
        }
    }
}

impl Default for ConfigEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the default config directory: `~/.config/surfterm/`.
fn dirs_config_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join("surfterm")
    } else {
        PathBuf::from(".config/surfterm")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_default_config_values() {
        let cfg = SurftermConfig::default();
        assert!((cfg.font_size - 14.0).abs() < f32::EPSILON);
        assert!((cfg.panel_ratio - 0.7).abs() < f32::EPSILON);
        assert_eq!(cfg.max_scroll_lines, 10000);
        assert!(!cfg.ble_enabled);
        assert!(cfg.llm_model_path.is_none());
    }

    #[test]
    fn test_default_keybinds_values() {
        let kb = KeybindsConfig::default();
        assert!(kb.prefix_key.is_none());
        assert!(kb.overrides.is_empty());
    }

    #[test]
    fn test_load_from_nonexistent_directory() {
        let engine = ConfigEngine::load(Path::new("/tmp/surfterm_test_nonexistent_dir_xyz"));
        // Should return defaults
        assert!((engine.config().font_size - 14.0).abs() < f32::EPSILON);
        assert!((engine.config().panel_ratio - 0.7).abs() < f32::EPSILON);
        assert_eq!(engine.config().max_scroll_lines, 10000);
        assert!(!engine.config().ble_enabled);
        assert!(engine.config().llm_model_path.is_none());
        assert!(engine.keybinds().prefix_key.is_none());
        assert!(engine.keybinds().overrides.is_empty());
    }

    #[test]
    fn test_parse_valid_config_toml() {
        let dir = tempdir("config_valid");
        fs::write(
            dir.join("config.toml"),
            r#"
font_size = 18.0
panel_ratio = 0.5
max_scroll_lines = 5000
ble_enabled = true
llm_model_path = "/path/to/model.gguf"
"#,
        )
        .unwrap();

        let engine = ConfigEngine::load(&dir);
        assert!((engine.config().font_size - 18.0).abs() < f32::EPSILON);
        assert!((engine.config().panel_ratio - 0.5).abs() < f32::EPSILON);
        assert_eq!(engine.config().max_scroll_lines, 5000);
        assert!(engine.config().ble_enabled);
        assert_eq!(
            engine.config().llm_model_path.as_deref(),
            Some("/path/to/model.gguf")
        );
    }

    #[test]
    fn test_parse_partial_config_toml_uses_defaults_for_missing() {
        let dir = tempdir("config_partial");
        fs::write(dir.join("config.toml"), "font_size = 20.0\n").unwrap();

        let engine = ConfigEngine::load(&dir);
        assert!((engine.config().font_size - 20.0).abs() < f32::EPSILON);
        // Other fields should be defaults
        assert!((engine.config().panel_ratio - 0.7).abs() < f32::EPSILON);
        assert_eq!(engine.config().max_scroll_lines, 10000);
    }

    #[test]
    fn test_parse_invalid_config_toml_falls_back_to_defaults() {
        let dir = tempdir("config_invalid");
        fs::write(dir.join("config.toml"), "this is not valid toml {{{").unwrap();

        let engine = ConfigEngine::load(&dir);
        // Should fall back to all defaults
        assert!((engine.config().font_size - 14.0).abs() < f32::EPSILON);
        assert!((engine.config().panel_ratio - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_valid_keybinds_toml() {
        let dir = tempdir("keybinds_valid");
        fs::write(
            dir.join("keybinds.toml"),
            r#"
prefix_key = "Ctrl+a"

[overrides]
new_session = "Ctrl+n"
close_session = "Ctrl+w"
"#,
        )
        .unwrap();

        let engine = ConfigEngine::load(&dir);
        assert_eq!(engine.keybinds().prefix_key.as_deref(), Some("Ctrl+a"));
        assert_eq!(
            engine.keybinds().overrides.get("new_session").map(|s| s.as_str()),
            Some("Ctrl+n")
        );
        assert_eq!(
            engine.keybinds().overrides.get("close_session").map(|s| s.as_str()),
            Some("Ctrl+w")
        );
    }

    #[test]
    fn test_parse_invalid_keybinds_toml_falls_back_to_defaults() {
        let dir = tempdir("keybinds_invalid");
        fs::write(dir.join("keybinds.toml"), "bad toml [[[").unwrap();

        let engine = ConfigEngine::load(&dir);
        assert!(engine.keybinds().prefix_key.is_none());
        assert!(engine.keybinds().overrides.is_empty());
    }

    #[test]
    fn test_load_detector_patterns_returns_defaults_when_no_files() {
        let dir = tempdir("detectors_empty");
        let engine = ConfigEngine::load(&dir);
        let patterns = engine.load_detector_patterns();

        // Should have at least the default patterns
        assert!(!patterns.is_empty());
        let default_count = crate::detector::patterns::default_claude_code_state_patterns().len();
        assert_eq!(patterns.len(), default_count);
    }

    #[test]
    fn test_load_detector_patterns_merges_user_and_defaults() {
        let dir = tempdir("detectors_merge");
        let detectors_dir = dir.join("detectors");
        fs::create_dir_all(&detectors_dir).unwrap();
        fs::write(
            detectors_dir.join("custom.toml"),
            r#"
[[patterns]]
name = "custom_idle"
regex = "^IDLE$"
state = "Idle"
"#,
        )
        .unwrap();

        let engine = ConfigEngine::load(&dir);
        let patterns = engine.load_detector_patterns();

        let default_count = crate::detector::patterns::default_claude_code_state_patterns().len();
        assert_eq!(patterns.len(), default_count + 1);

        // User pattern should come first
        assert_eq!(patterns[0].name, "custom_idle");
    }

    #[test]
    fn test_load_detector_patterns_skips_invalid_files() {
        let dir = tempdir("detectors_invalid");
        let detectors_dir = dir.join("detectors");
        fs::create_dir_all(&detectors_dir).unwrap();

        // One valid, one invalid
        fs::write(
            detectors_dir.join("good.toml"),
            r#"
[[patterns]]
name = "good"
regex = "GOOD"
state = "Running"
"#,
        )
        .unwrap();
        fs::write(detectors_dir.join("bad.toml"), "invalid toml [[[").unwrap();

        let engine = ConfigEngine::load(&dir);
        let patterns = engine.load_detector_patterns();

        let default_count = crate::detector::patterns::default_claude_code_state_patterns().len();
        // Should have defaults + 1 from good.toml (bad.toml skipped)
        assert_eq!(patterns.len(), default_count + 1);
    }

    /// Helper to create a unique temporary directory for each test.
    fn tempdir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("surfterm_test_{suffix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
