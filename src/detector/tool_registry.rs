use std::path::Path;

use regex::Regex;
use serde::Deserialize;
use tracing::{instrument, warn};

use crate::detector::patterns::StatePattern;
use crate::session::state::SessionState;
use crate::session::stream_splitter::{Classification, Pattern};

/// Definition of an AI tool with its detection and classification patterns.
#[allow(dead_code)]
pub struct ToolDefinition {
    pub name: String,
    pub command_patterns: Vec<Regex>,
    pub stream_patterns: Vec<Pattern>,
    pub state_patterns: Vec<StatePattern>,
}

/// TOML deserialization types for tool definition files.
#[derive(Deserialize)]
struct ToolDefinitionFile {
    name: String,
    command_patterns: Vec<String>,
    #[serde(default)]
    stream_patterns: Vec<StreamPatternEntry>,
    #[serde(default)]
    state_patterns: Vec<StatePatternEntry>,
}

#[derive(Deserialize)]
struct StreamPatternEntry {
    name: String,
    regex: String,
    classification: String,
}

#[derive(Deserialize)]
struct StatePatternEntry {
    name: String,
    regex: String,
    state: String,
}

/// Registry of known AI tools and their detection patterns.
#[allow(dead_code)]
pub struct ToolRegistry {
    tools: Vec<ToolDefinition>,
}

#[allow(dead_code)]
impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a tool definition.
    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.push(tool);
    }

    /// Create a registry pre-populated with the default Claude Code tool definition.
    #[instrument]
    pub fn default_registry() -> Self {
        let mut registry = Self::new();

        let command_patterns = vec![
            Regex::new("claude").expect("invalid default command pattern"),
        ];

        let stream_patterns = vec![
            Pattern {
                name: "tool_indicator".to_string(),
                regex: Regex::new("⏺").expect("invalid default stream pattern"),
                classification: Classification::State,
            },
        ];

        let state_patterns = vec![
            StatePattern {
                name: "prompt".to_string(),
                regex: Regex::new(r"^>\s").expect("invalid default state pattern"),
                target_state: SessionState::WaitingForInput,
            },
        ];

        registry.register(ToolDefinition {
            name: "claude-code".to_string(),
            command_patterns,
            stream_patterns,
            state_patterns,
        });

        registry
    }

    /// Load tool definitions from all `.toml` files in the given config directory's
    /// `detectors/` subdirectory. Each file defines one tool.
    #[instrument(skip_all, fields(dir = %config_dir.display()))]
    pub fn load_from_config(config_dir: &Path) -> Self {
        let mut registry = Self::new();
        let detectors_dir = config_dir.join("detectors");

        if !detectors_dir.is_dir() {
            return registry;
        }

        let mut paths: Vec<_> = match std::fs::read_dir(&detectors_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
                .collect(),
            Err(e) => {
                warn!(error = %e, "failed to read detectors directory");
                return registry;
            }
        };
        paths.sort();

        for path in paths {
            match std::fs::read_to_string(&path) {
                Ok(content) => match parse_tool_definition(&content) {
                    Ok(tool) => registry.register(tool),
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "failed to parse tool definition, skipping"
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to read tool definition file, skipping"
                    );
                }
            }
        }

        registry
    }

    /// Look up a tool definition by name.
    pub fn get_tool(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// Find the first tool whose command patterns match the given command string.
    pub fn detect_tool(&self, command: &str) -> Option<&ToolDefinition> {
        self.tools
            .iter()
            .find(|t| t.command_patterns.iter().any(|p| p.is_match(command)))
    }

    /// Detect a tool from a command string and return its name.
    pub fn detect_from_command(&self, command: &str) -> Option<String> {
        self.detect_tool(command).map(|t| t.name.clone())
    }

    /// Return the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a TOML string into a `ToolDefinition`.
fn parse_tool_definition(content: &str) -> anyhow::Result<ToolDefinition> {
    let file: ToolDefinitionFile =
        toml::from_str(content).map_err(|e| anyhow::anyhow!("TOML parse error: {e}"))?;

    let command_patterns: Vec<Regex> = file
        .command_patterns
        .iter()
        .map(|p| Regex::new(p))
        .collect::<Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("invalid command pattern regex: {e}"))?;

    let stream_patterns: Vec<Pattern> = file
        .stream_patterns
        .into_iter()
        .map(|entry| {
            let classification = match entry.classification.as_str() {
                "Message" => Classification::Message,
                "State" => Classification::State,
                "Raw" => Classification::Raw,
                other => {
                    return Err(anyhow::anyhow!(
                        "unknown classification '{}' in stream pattern '{}'",
                        other,
                        entry.name
                    ))
                }
            };
            let regex = Regex::new(&entry.regex).map_err(|e| {
                anyhow::anyhow!("invalid regex in stream pattern '{}': {e}", entry.name)
            })?;
            Ok(Pattern {
                name: entry.name,
                regex,
                classification,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let state_patterns: Vec<StatePattern> = file
        .state_patterns
        .into_iter()
        .map(|entry| {
            let target_state = match entry.state.as_str() {
                "Idle" => SessionState::Idle,
                "Running" => SessionState::Running,
                "WaitingForInput" => SessionState::WaitingForInput,
                "Error" => SessionState::Error,
                other => {
                    return Err(anyhow::anyhow!(
                        "unknown state '{}' in state pattern '{}'",
                        other,
                        entry.name
                    ))
                }
            };
            let regex = Regex::new(&entry.regex).map_err(|e| {
                anyhow::anyhow!("invalid regex in state pattern '{}': {e}", entry.name)
            })?;
            Ok(StatePattern {
                name: entry.name,
                regex,
                target_state,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(ToolDefinition {
        name: file.name,
        command_patterns,
        stream_patterns,
        state_patterns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    // --- ToolRegistry basic tests ---

    #[test]
    fn test_new_registry_is_empty() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.tool_count(), 0);
    }

    #[test]
    fn test_default_registry_has_claude_code() {
        let registry = ToolRegistry::default_registry();
        assert_eq!(registry.tool_count(), 1);

        let tool = registry.get_tool("claude-code");
        assert!(tool.is_some(), "default registry should contain claude-code");
        let tool = tool.unwrap();
        assert_eq!(tool.name, "claude-code");
        assert!(!tool.command_patterns.is_empty());
        assert!(!tool.stream_patterns.is_empty());
        assert!(!tool.state_patterns.is_empty());
    }

    #[test]
    fn test_register_custom_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDefinition {
            name: "my-tool".to_string(),
            command_patterns: vec![Regex::new("my-tool").unwrap()],
            stream_patterns: vec![],
            state_patterns: vec![],
        });

        assert_eq!(registry.tool_count(), 1);
        assert!(registry.get_tool("my-tool").is_some());
        assert!(registry.get_tool("nonexistent").is_none());
    }

    // --- detect_tool tests ---

    #[test]
    fn test_detect_tool_matches_claude() {
        let registry = ToolRegistry::default_registry();

        let tool = registry.detect_tool("claude");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, "claude-code");

        let tool = registry.detect_tool("claude --help");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, "claude-code");
    }

    #[test]
    fn test_detect_tool_no_match_for_unknown() {
        let registry = ToolRegistry::default_registry();
        assert!(registry.detect_tool("vim").is_none());
        assert!(registry.detect_tool("emacs").is_none());
        assert!(registry.detect_tool("").is_none());
    }

    // --- detect_from_command tests ---

    #[test]
    fn test_detect_from_command_returns_name() {
        let registry = ToolRegistry::default_registry();

        assert_eq!(
            registry.detect_from_command("claude"),
            Some("claude-code".to_string())
        );
        assert_eq!(
            registry.detect_from_command("claude --model opus"),
            Some("claude-code".to_string())
        );
    }

    #[test]
    fn test_detect_from_command_no_match() {
        let registry = ToolRegistry::default_registry();
        assert_eq!(registry.detect_from_command("vim"), None);
        assert_eq!(registry.detect_from_command("unknown-tool"), None);
    }

    // --- TOML parsing tests ---

    #[test]
    fn test_parse_tool_definition_full() {
        let toml_content = r#"
name = "claude-code"
command_patterns = ["claude"]

[[stream_patterns]]
name = "tool_indicator"
regex = "⏺"
classification = "State"

[[state_patterns]]
name = "prompt"
regex = "^>\\s"
state = "WaitingForInput"
"#;

        let tool = parse_tool_definition(toml_content).unwrap();
        assert_eq!(tool.name, "claude-code");
        assert_eq!(tool.command_patterns.len(), 1);
        assert!(tool.command_patterns[0].is_match("claude"));
        assert_eq!(tool.stream_patterns.len(), 1);
        assert_eq!(tool.stream_patterns[0].name, "tool_indicator");
        assert_eq!(tool.stream_patterns[0].classification, Classification::State);
        assert_eq!(tool.state_patterns.len(), 1);
        assert_eq!(tool.state_patterns[0].name, "prompt");
        assert_eq!(tool.state_patterns[0].target_state, SessionState::WaitingForInput);
    }

    #[test]
    fn test_parse_tool_definition_minimal() {
        let toml_content = r#"
name = "simple-tool"
command_patterns = ["simple"]
"#;

        let tool = parse_tool_definition(toml_content).unwrap();
        assert_eq!(tool.name, "simple-tool");
        assert!(tool.stream_patterns.is_empty());
        assert!(tool.state_patterns.is_empty());
    }

    #[test]
    fn test_parse_tool_definition_multiple_command_patterns() {
        let toml_content = r#"
name = "cursor"
command_patterns = ["cursor", "cursor-cli", "code --cursor"]
"#;

        let tool = parse_tool_definition(toml_content).unwrap();
        assert_eq!(tool.command_patterns.len(), 3);
        assert!(tool.command_patterns[0].is_match("cursor"));
        assert!(tool.command_patterns[1].is_match("cursor-cli"));
        assert!(tool.command_patterns[2].is_match("code --cursor"));
    }

    #[test]
    fn test_parse_tool_definition_invalid_toml() {
        let result = parse_tool_definition("this is not valid toml {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tool_definition_invalid_regex() {
        let toml_content = r#"
name = "bad"
command_patterns = ["[invalid"]
"#;
        let result = parse_tool_definition(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tool_definition_invalid_classification() {
        let toml_content = r#"
name = "bad"
command_patterns = ["bad"]

[[stream_patterns]]
name = "bad_pattern"
regex = "test"
classification = "Unknown"
"#;
        let result = parse_tool_definition(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tool_definition_invalid_state() {
        let toml_content = r#"
name = "bad"
command_patterns = ["bad"]

[[state_patterns]]
name = "bad_pattern"
regex = "test"
state = "Unknown"
"#;
        let result = parse_tool_definition(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tool_definition_all_classifications() {
        let toml_content = r#"
name = "test"
command_patterns = ["test"]

[[stream_patterns]]
name = "msg"
regex = "msg"
classification = "Message"

[[stream_patterns]]
name = "state"
regex = "state"
classification = "State"

[[stream_patterns]]
name = "raw"
regex = "raw"
classification = "Raw"
"#;

        let tool = parse_tool_definition(toml_content).unwrap();
        assert_eq!(tool.stream_patterns.len(), 3);
        assert_eq!(tool.stream_patterns[0].classification, Classification::Message);
        assert_eq!(tool.stream_patterns[1].classification, Classification::State);
        assert_eq!(tool.stream_patterns[2].classification, Classification::Raw);
    }

    #[test]
    fn test_parse_tool_definition_all_states() {
        let toml_content = r#"
name = "test"
command_patterns = ["test"]

[[state_patterns]]
name = "idle"
regex = "IDLE"
state = "Idle"

[[state_patterns]]
name = "running"
regex = "RUN"
state = "Running"

[[state_patterns]]
name = "waiting"
regex = "WAIT"
state = "WaitingForInput"

[[state_patterns]]
name = "error"
regex = "ERR"
state = "Error"
"#;

        let tool = parse_tool_definition(toml_content).unwrap();
        assert_eq!(tool.state_patterns.len(), 4);
        assert_eq!(tool.state_patterns[0].target_state, SessionState::Idle);
        assert_eq!(tool.state_patterns[1].target_state, SessionState::Running);
        assert_eq!(tool.state_patterns[2].target_state, SessionState::WaitingForInput);
        assert_eq!(tool.state_patterns[3].target_state, SessionState::Error);
    }

    // --- load_from_config tests ---

    #[test]
    fn test_load_from_config_nonexistent_dir() {
        let registry = ToolRegistry::load_from_config(Path::new("/tmp/nonexistent_surfterm_dir_xyz"));
        assert_eq!(registry.tool_count(), 0);
    }

    #[test]
    fn test_load_from_config_with_tool_file() {
        let dir = tempdir("tool_registry_load");
        let detectors_dir = dir.join("detectors");
        fs::create_dir_all(&detectors_dir).unwrap();
        fs::write(
            detectors_dir.join("claude-code.toml"),
            r#"
name = "claude-code"
command_patterns = ["claude"]

[[stream_patterns]]
name = "tool_indicator"
regex = "⏺"
classification = "State"

[[state_patterns]]
name = "prompt"
regex = "^>\\s"
state = "WaitingForInput"
"#,
        )
        .unwrap();

        let registry = ToolRegistry::load_from_config(&dir);
        assert_eq!(registry.tool_count(), 1);
        let tool = registry.get_tool("claude-code").unwrap();
        assert_eq!(tool.name, "claude-code");
        assert!(tool.command_patterns[0].is_match("claude"));
    }

    #[test]
    fn test_load_from_config_multiple_tools() {
        let dir = tempdir("tool_registry_multi");
        let detectors_dir = dir.join("detectors");
        fs::create_dir_all(&detectors_dir).unwrap();

        fs::write(
            detectors_dir.join("claude-code.toml"),
            r#"
name = "claude-code"
command_patterns = ["claude"]
"#,
        )
        .unwrap();

        fs::write(
            detectors_dir.join("cursor.toml"),
            r#"
name = "cursor"
command_patterns = ["cursor"]
"#,
        )
        .unwrap();

        let registry = ToolRegistry::load_from_config(&dir);
        assert_eq!(registry.tool_count(), 2);
        assert!(registry.get_tool("claude-code").is_some());
        assert!(registry.get_tool("cursor").is_some());
    }

    #[test]
    fn test_load_from_config_skips_invalid() {
        let dir = tempdir("tool_registry_invalid");
        let detectors_dir = dir.join("detectors");
        fs::create_dir_all(&detectors_dir).unwrap();

        fs::write(
            detectors_dir.join("good.toml"),
            r#"
name = "good-tool"
command_patterns = ["good"]
"#,
        )
        .unwrap();

        fs::write(detectors_dir.join("bad.toml"), "invalid toml {{{").unwrap();

        let registry = ToolRegistry::load_from_config(&dir);
        assert_eq!(registry.tool_count(), 1);
        assert!(registry.get_tool("good-tool").is_some());
    }

    // --- Integration: detect_tool with loaded registry ---

    #[test]
    fn test_detect_tool_from_loaded_registry() {
        let dir = tempdir("tool_registry_detect");
        let detectors_dir = dir.join("detectors");
        fs::create_dir_all(&detectors_dir).unwrap();

        fs::write(
            detectors_dir.join("claude-code.toml"),
            r#"
name = "claude-code"
command_patterns = ["claude"]
"#,
        )
        .unwrap();

        fs::write(
            detectors_dir.join("copilot.toml"),
            r#"
name = "copilot-cli"
command_patterns = ["gh copilot", "copilot"]
"#,
        )
        .unwrap();

        let registry = ToolRegistry::load_from_config(&dir);

        assert_eq!(
            registry.detect_from_command("claude"),
            Some("claude-code".to_string())
        );
        assert_eq!(
            registry.detect_from_command("gh copilot suggest"),
            Some("copilot-cli".to_string())
        );
        assert_eq!(registry.detect_from_command("vim"), None);
    }

    fn tempdir(suffix: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("surfterm_test_{suffix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
