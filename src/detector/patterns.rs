use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use tracing::instrument;

use crate::session::state::SessionState;

/// A named regex pattern that maps to a target `SessionState`.
#[allow(dead_code)]
pub struct StatePattern {
    pub name: String,
    pub regex: Regex,
    pub target_state: SessionState,
}

/// Intermediate TOML representation for deserialization.
#[derive(Deserialize)]
struct PatternEntry {
    name: String,
    regex: String,
    state: String,
}

#[derive(Deserialize)]
struct PatternsFile {
    patterns: Vec<PatternEntry>,
}

/// Return the default set of state-detection patterns for Claude Code output.
#[instrument]
#[allow(dead_code)]
pub fn default_claude_code_state_patterns() -> Vec<StatePattern> {
    let waiting_patterns = vec![
        ("prompt_char", r"^>\s"),
        ("would_you_like", r"(?i)would you like"),
        ("do_you_want", r"(?i)do you want"),
        ("yn_prompt", r"(?i)Y/n"),
        ("yes_no_prompt", r"(?i)yes/no"),
    ];

    let running_patterns = vec![
        ("tool_indicator", r"⏺"),
        ("reading", r"(?i)\bReading\b"),
        ("writing", r"(?i)\bWriting\b"),
        ("searching", r"(?i)\bSearching\b"),
        ("running", r"(?i)\bRunning\b"),
    ];

    let error_patterns = vec![
        ("error_upper", r"Error:"),
        ("error_lower", r"error:"),
        ("failed", r"FAILED"),
        ("panic", r"\bpanic\b"),
        ("permission_denied", r"Permission denied"),
    ];

    let mut patterns = Vec::new();

    for (name, re) in waiting_patterns {
        patterns.push(StatePattern {
            name: name.to_string(),
            regex: Regex::new(re).expect("invalid default WaitingForInput pattern"),
            target_state: SessionState::WaitingForInput,
        });
    }

    for (name, re) in running_patterns {
        patterns.push(StatePattern {
            name: name.to_string(),
            regex: Regex::new(re).expect("invalid default Running pattern"),
            target_state: SessionState::Running,
        });
    }

    for (name, re) in error_patterns {
        patterns.push(StatePattern {
            name: name.to_string(),
            regex: Regex::new(re).expect("invalid default Error pattern"),
            target_state: SessionState::Error,
        });
    }

    patterns
}

/// Parse state-detection patterns from a TOML string.
///
/// Expected format:
/// ```toml
/// [[patterns]]
/// name = "prompt_char"
/// regex = "^>\\s"
/// state = "WaitingForInput"
/// ```
#[instrument(skip(content))]
#[allow(dead_code)]
pub fn load_patterns_from_toml(content: &str) -> Result<Vec<StatePattern>> {
    let file: PatternsFile =
        toml::from_str(content).context("failed to parse patterns TOML")?;

    let mut patterns = Vec::new();
    for entry in file.patterns {
        let target_state = match entry.state.as_str() {
            "Idle" => SessionState::Idle,
            "Running" => SessionState::Running,
            "WaitingForInput" => SessionState::WaitingForInput,
            "Error" => SessionState::Error,
            other => anyhow::bail!("unknown state '{}' in pattern '{}'", other, entry.name),
        };

        let regex = Regex::new(&entry.regex)
            .with_context(|| format!("invalid regex in pattern '{}'", entry.name))?;

        patterns.push(StatePattern {
            name: entry.name,
            regex,
            target_state,
        });
    }

    Ok(patterns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_patterns_exist() {
        let patterns = default_claude_code_state_patterns();
        assert!(!patterns.is_empty(), "should have default patterns");

        let has_waiting = patterns
            .iter()
            .any(|p| p.target_state == SessionState::WaitingForInput);
        let has_running = patterns
            .iter()
            .any(|p| p.target_state == SessionState::Running);
        let has_error = patterns
            .iter()
            .any(|p| p.target_state == SessionState::Error);

        assert!(has_waiting, "should have WaitingForInput patterns");
        assert!(has_running, "should have Running patterns");
        assert!(has_error, "should have Error patterns");
    }

    #[test]
    fn test_default_patterns_match_expected_inputs() {
        let patterns = default_claude_code_state_patterns();

        // WaitingForInput patterns
        let waiting: Vec<_> = patterns
            .iter()
            .filter(|p| p.target_state == SessionState::WaitingForInput)
            .collect();
        assert!(waiting.iter().any(|p| p.regex.is_match("> ")));
        assert!(waiting.iter().any(|p| p.regex.is_match("Would you like to proceed?")));
        assert!(waiting.iter().any(|p| p.regex.is_match("Do you want to continue?")));
        assert!(waiting.iter().any(|p| p.regex.is_match("Proceed? Y/n")));
        assert!(waiting.iter().any(|p| p.regex.is_match("Continue? yes/no")));

        // Running patterns
        let running: Vec<_> = patterns
            .iter()
            .filter(|p| p.target_state == SessionState::Running)
            .collect();
        assert!(running.iter().any(|p| p.regex.is_match("⏺ Read file")));
        assert!(running.iter().any(|p| p.regex.is_match("Reading src/main.rs")));
        assert!(running.iter().any(|p| p.regex.is_match("Writing output")));
        assert!(running.iter().any(|p| p.regex.is_match("Searching for pattern")));
        assert!(running.iter().any(|p| p.regex.is_match("Running command")));

        // Error patterns
        let errors: Vec<_> = patterns
            .iter()
            .filter(|p| p.target_state == SessionState::Error)
            .collect();
        assert!(errors.iter().any(|p| p.regex.is_match("Error: something broke")));
        assert!(errors.iter().any(|p| p.regex.is_match("error: compilation failed")));
        assert!(errors.iter().any(|p| p.regex.is_match("FAILED to build")));
        assert!(errors.iter().any(|p| p.regex.is_match("thread panic")));
        assert!(errors.iter().any(|p| p.regex.is_match("Permission denied")));
    }

    #[test]
    fn test_toml_parsing() {
        let toml_content = r#"
[[patterns]]
name = "custom_prompt"
regex = "^\\$\\s"
state = "WaitingForInput"

[[patterns]]
name = "custom_running"
regex = "Compiling"
state = "Running"

[[patterns]]
name = "custom_error"
regex = "FATAL"
state = "Error"
"#;

        let patterns = load_patterns_from_toml(toml_content).unwrap();
        assert_eq!(patterns.len(), 3);
        assert_eq!(patterns[0].name, "custom_prompt");
        assert_eq!(patterns[0].target_state, SessionState::WaitingForInput);
        assert_eq!(patterns[1].name, "custom_running");
        assert_eq!(patterns[1].target_state, SessionState::Running);
        assert_eq!(patterns[2].name, "custom_error");
        assert_eq!(patterns[2].target_state, SessionState::Error);

        // Verify the regexes actually work
        assert!(patterns[0].regex.is_match("$ ls"));
        assert!(patterns[1].regex.is_match("Compiling surfterm v0.1.0"));
        assert!(patterns[2].regex.is_match("FATAL error occurred"));
    }

    #[test]
    fn test_toml_parsing_invalid_state() {
        let toml_content = r#"
[[patterns]]
name = "bad"
regex = "test"
state = "Unknown"
"#;
        let result = load_patterns_from_toml(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_toml_parsing_invalid_regex() {
        let toml_content = r#"
[[patterns]]
name = "bad_regex"
regex = "[invalid"
state = "Running"
"#;
        let result = load_patterns_from_toml(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_toml_parsing_idle_state() {
        let toml_content = r#"
[[patterns]]
name = "idle_marker"
regex = "^idle$"
state = "Idle"
"#;
        let patterns = load_patterns_from_toml(toml_content).unwrap();
        assert_eq!(patterns[0].target_state, SessionState::Idle);
    }

    // --- Additional TOML edge case tests ---

    #[test]
    fn test_empty_toml_string_fails() {
        let result = load_patterns_from_toml("");
        assert!(result.is_err(), "Empty TOML string should fail to parse");
    }

    #[test]
    fn test_toml_no_patterns_key() {
        let toml_content = r#"
[metadata]
name = "test"
"#;
        let result = load_patterns_from_toml(toml_content);
        assert!(result.is_err(), "TOML without patterns key should fail");
    }

    #[test]
    fn test_toml_empty_patterns_array() {
        let toml_content = "patterns = []\n";
        let patterns = load_patterns_from_toml(toml_content).unwrap();
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_toml_all_four_state_types() {
        let toml_content = r#"
[[patterns]]
name = "idle_marker"
regex = "IDLE"
state = "Idle"

[[patterns]]
name = "run_marker"
regex = "RUNNING"
state = "Running"

[[patterns]]
name = "wait_marker"
regex = "WAITING"
state = "WaitingForInput"

[[patterns]]
name = "err_marker"
regex = "ERROR"
state = "Error"
"#;
        let patterns = load_patterns_from_toml(toml_content).unwrap();
        assert_eq!(patterns.len(), 4);
        assert_eq!(patterns[0].target_state, SessionState::Idle);
        assert_eq!(patterns[1].target_state, SessionState::Running);
        assert_eq!(patterns[2].target_state, SessionState::WaitingForInput);
        assert_eq!(patterns[3].target_state, SessionState::Error);

        // Verify each pattern actually matches
        assert!(patterns[0].regex.is_match("IDLE"));
        assert!(patterns[1].regex.is_match("RUNNING"));
        assert!(patterns[2].regex.is_match("WAITING"));
        assert!(patterns[3].regex.is_match("ERROR"));
    }

    #[test]
    fn test_toml_missing_fields() {
        // Missing regex field
        let toml_content = r#"
[[patterns]]
name = "bad"
state = "Running"
"#;
        let result = load_patterns_from_toml(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_toml_missing_name_field() {
        let toml_content = r#"
[[patterns]]
regex = "test"
state = "Running"
"#;
        let result = load_patterns_from_toml(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_toml_missing_state_field() {
        let toml_content = r#"
[[patterns]]
name = "test"
regex = "test"
"#;
        let result = load_patterns_from_toml(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_toml_complex_regex() {
        let toml_content = r#"
[[patterns]]
name = "complex"
regex = "^\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}$"
state = "Running"
"#;
        let patterns = load_patterns_from_toml(toml_content).unwrap();
        assert!(patterns[0].regex.is_match("192.168.1.1"));
        assert!(!patterns[0].regex.is_match("not an ip"));
    }

    #[test]
    fn test_toml_many_patterns() {
        let mut toml = String::new();
        for i in 0..50 {
            toml.push_str(&format!(
                "[[patterns]]\nname = \"p{i}\"\nregex = \"pattern_{i}\"\nstate = \"Running\"\n\n"
            ));
        }
        let patterns = load_patterns_from_toml(&toml).unwrap();
        assert_eq!(patterns.len(), 50);
        assert!(patterns[49].regex.is_match("pattern_49"));
    }

    #[test]
    fn test_default_patterns_do_not_match_normal_text() {
        let patterns = default_claude_code_state_patterns();

        // Normal text should not match any pattern
        let normal_texts = vec![
            "hello world",
            "foo bar baz",
            "1234567890",
            "just some code: let x = 5;",
        ];

        for text in normal_texts {
            let matched = patterns.iter().any(|p| p.regex.is_match(text));
            assert!(
                !matched,
                "Normal text '{}' should not match any state pattern",
                text
            );
        }
    }
}
