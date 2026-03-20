use regex::Regex;
use tokio::sync::broadcast;
use tracing::instrument;

/// Classification result of a PTY output chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Classification {
    /// Conversation text from the AI tool.
    Message,
    /// Status information (tool execution, cost, tokens).
    State,
    /// Raw VT output (unclassified).
    Raw,
}

/// A named regex pattern with its target classification.
#[allow(dead_code)]
pub struct Pattern {
    pub name: String,
    pub regex: Regex,
    pub classification: Classification,
}

/// A chunk of PTY output with its classification.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClassifiedChunk {
    pub data: Vec<u8>,
    pub classification: Classification,
}

/// Receivers for the three classification channels.
#[allow(dead_code)]
pub struct Channels {
    pub message_rx: broadcast::Receiver<ClassifiedChunk>,
    pub state_rx: broadcast::Receiver<ClassifiedChunk>,
    pub raw_rx: broadcast::Receiver<ClassifiedChunk>,
}

/// Splits PTY output into Message/State/Raw channels based on regex patterns.
#[allow(dead_code)]
pub struct StreamSplitter {
    patterns: Vec<Pattern>,
    message_tx: broadcast::Sender<ClassifiedChunk>,
    state_tx: broadcast::Sender<ClassifiedChunk>,
    raw_tx: broadcast::Sender<ClassifiedChunk>,
}

const CHANNEL_CAPACITY: usize = 256;

#[allow(dead_code)]
impl StreamSplitter {
    /// Create a new `StreamSplitter` with the given patterns and return the
    /// receiver side of all three broadcast channels.
    #[instrument(skip_all)]
    pub fn new(patterns: Vec<Pattern>) -> (Self, Channels) {
        let (message_tx, message_rx) = broadcast::channel(CHANNEL_CAPACITY);
        let (state_tx, state_rx) = broadcast::channel(CHANNEL_CAPACITY);
        let (raw_tx, raw_rx) = broadcast::channel(CHANNEL_CAPACITY);

        let splitter = Self {
            patterns,
            message_tx,
            state_tx,
            raw_tx,
        };

        let channels = Channels {
            message_rx,
            state_rx,
            raw_rx,
        };

        (splitter, channels)
    }

    /// Classify a chunk of PTY output and send it to the appropriate channel.
    ///
    /// The data is converted to UTF-8 (lossy), split by lines, and each line
    /// is matched against the patterns in order. The first matching pattern
    /// determines the classification. Unmatched lines are classified as `Raw`.
    #[instrument(skip_all)]
    pub fn classify_chunk(&self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);

        for line in text.lines() {
            let classification = self.classify_line(line);
            let chunk = ClassifiedChunk {
                data: line.as_bytes().to_vec(),
                classification: classification.clone(),
            };

            let _ = match classification {
                Classification::Message => self.message_tx.send(chunk),
                Classification::State => self.state_tx.send(chunk),
                Classification::Raw => self.raw_tx.send(chunk),
            };
        }
    }

    /// Return the default set of regex patterns for classifying Claude Code output.
    #[instrument]
    pub fn default_claude_code_patterns() -> Vec<Pattern> {
        let state_patterns = vec![
            ("tool_use_indicator", r"⏺"),
            ("cost_line", r"(?i)cost:\s*\$"),
            ("token_line", r"(?i)token"),
            ("tool_read", r"(?i)^\s*Read\b"),
            ("tool_write", r"(?i)^\s*Write\b"),
            ("tool_edit", r"(?i)^\s*Edit\b"),
            ("tool_bash", r"(?i)^\s*Bash\b"),
            ("tool_glob", r"(?i)^\s*Glob\b"),
            ("tool_grep", r"(?i)^\s*Grep\b"),
            ("permission_prompt", r"(?i)allow|deny|permission"),
        ];

        let message_patterns = vec![
            ("ai_greeting", r"(?i)^(hello|hi|hey|I'll help|I can help|let me|sure|certainly)"),
            ("ai_explanation", r"(?i)^(here('s| is)|this (is|will)|the |I('ve| have| will| would))"),
            ("ai_markdown_header", r"^#{1,6}\s"),
            ("ai_bullet_list", r"^\s*[-*]\s"),
            ("ai_numbered_list", r"^\s*\d+\.\s"),
        ];

        let mut patterns = Vec::new();

        for (name, re) in state_patterns {
            patterns.push(Pattern {
                name: name.to_string(),
                regex: Regex::new(re).expect("invalid default state pattern"),
                classification: Classification::State,
            });
        }

        for (name, re) in message_patterns {
            patterns.push(Pattern {
                name: name.to_string(),
                regex: Regex::new(re).expect("invalid default message pattern"),
                classification: Classification::Message,
            });
        }

        patterns
    }

    /// Classify a single line against the stored patterns.
    fn classify_line(&self, line: &str) -> Classification {
        for pattern in &self.patterns {
            if pattern.regex.is_match(line) {
                return pattern.classification.clone();
            }
        }
        Classification::Raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_splitter() -> (StreamSplitter, Channels) {
        let patterns = StreamSplitter::default_claude_code_patterns();
        StreamSplitter::new(patterns)
    }

    #[test]
    fn test_message_classification() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"Hello, I'll help you with that");
        let chunk = channels.message_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Message);
    }

    #[test]
    fn test_state_tool_use_indicator() {
        let (splitter, mut channels) = make_splitter();
        // ⏺ is a multi-byte UTF-8 character
        splitter.classify_chunk("⏺ Read src/main.rs".as_bytes());
        let chunk = channels.state_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::State);
    }

    #[test]
    fn test_state_cost_line() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"Cost: $0.05");
        let chunk = channels.state_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::State);
    }

    #[test]
    fn test_raw_classification() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"\x1b[32msome ansi output\x1b[0m");
        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
    }

    #[test]
    fn test_default_patterns_cover_basic_claude_output() {
        let patterns = StreamSplitter::default_claude_code_patterns();

        // Should have both state and message patterns
        let has_state = patterns
            .iter()
            .any(|p| p.classification == Classification::State);
        let has_message = patterns
            .iter()
            .any(|p| p.classification == Classification::Message);

        assert!(has_state, "default patterns should include State patterns");
        assert!(
            has_message,
            "default patterns should include Message patterns"
        );
        assert!(
            patterns.len() >= 10,
            "should have a reasonable number of patterns"
        );
    }

    #[test]
    fn test_tool_indicators_classified_as_state() {
        let (splitter, mut channels) = make_splitter();

        let tool_lines = vec![
            "Read src/main.rs",
            "Write src/lib.rs",
            "Edit src/app.rs",
            "Bash ls -la",
            "Glob **/*.rs",
            "Grep pattern",
        ];

        for line in tool_lines {
            splitter.classify_chunk(line.as_bytes());
            let chunk = channels.state_rx.try_recv().unwrap();
            assert_eq!(
                chunk.classification,
                Classification::State,
                "expected State for: {line}"
            );
        }
    }

    #[test]
    fn test_multiline_chunk_splits_correctly() {
        let (splitter, mut channels) = make_splitter();
        let input = "Hello, I'll help you\nCost: $0.05\n\x1b[0mraw\x1b[0m";
        splitter.classify_chunk(input.as_bytes());

        let msg = channels.message_rx.try_recv().unwrap();
        assert_eq!(msg.classification, Classification::Message);

        let state = channels.state_rx.try_recv().unwrap();
        assert_eq!(state.classification, Classification::State);

        let raw = channels.raw_rx.try_recv().unwrap();
        assert_eq!(raw.classification, Classification::Raw);
    }

    #[test]
    fn test_classify_performance() {
        let patterns = StreamSplitter::default_claude_code_patterns();
        let (splitter, _channels) = StreamSplitter::new(patterns);

        // Build a reasonably sized chunk (~4KB)
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(b"Hello, I'll help you with that task\n");
        }

        let start = std::time::Instant::now();
        splitter.classify_chunk(&data);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 5,
            "classification should complete within 5ms, took {}ms",
            elapsed.as_millis()
        );
    }

    // --- Edge case tests ---

    #[test]
    fn test_empty_input_sends_nothing() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"");

        assert!(channels.message_rx.try_recv().is_err());
        assert!(channels.state_rx.try_recv().is_err());
        assert!(channels.raw_rx.try_recv().is_err());
    }

    #[test]
    fn test_single_character_classified_as_raw() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"x");
        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
        assert_eq!(chunk.data, b"x");
    }

    #[test]
    fn test_binary_data_does_not_panic() {
        let (splitter, mut channels) = make_splitter();
        let binary = vec![0x00, 0x01, 0x02, 0x80, 0xff, 0xfe, 0xfd];
        splitter.classify_chunk(&binary);

        // Should be classified as Raw since no pattern matches binary
        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
    }

    #[test]
    fn test_very_long_line() {
        let (splitter, mut channels) = make_splitter();
        let long = "a".repeat(50_000);
        splitter.classify_chunk(long.as_bytes());

        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
        assert_eq!(chunk.data.len(), 50_000);
    }

    #[test]
    fn test_unicode_content_classified() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk("こんにちは世界".as_bytes());
        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
        assert_eq!(
            String::from_utf8(chunk.data).unwrap(),
            "こんにちは世界"
        );
    }

    #[test]
    fn test_emoji_content() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk("🚀 deploying...".as_bytes());
        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
    }

    #[test]
    fn test_newline_only() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"\n\n\n");
        // Empty lines between newlines should not produce chunks
        // (String::lines skips empty trailing but produces empty lines between)
        // All empty lines are classified as Raw
        // Actually, "\n\n\n".lines() yields ["", "", ""] — 3 empty strings
        // Each empty line still goes through classify_line which returns Raw
        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
        assert!(chunk.data.is_empty());
    }

    #[test]
    fn test_only_whitespace_classified_as_raw() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"   \t  ");
        let chunk = channels.raw_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Raw);
    }

    #[test]
    fn test_ai_markdown_header_classified_as_message() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"## Architecture Overview");
        let chunk = channels.message_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Message);
    }

    #[test]
    fn test_ai_bullet_list_classified_as_message() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"- First item in list");
        let chunk = channels.message_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Message);
    }

    #[test]
    fn test_ai_numbered_list_classified_as_message() {
        let (splitter, mut channels) = make_splitter();
        splitter.classify_chunk(b"1. First step");
        let chunk = channels.message_rx.try_recv().unwrap();
        assert_eq!(chunk.classification, Classification::Message);
    }

    #[test]
    fn test_large_mixed_content_performance() {
        let patterns = StreamSplitter::default_claude_code_patterns();
        let (splitter, _channels) = StreamSplitter::new(patterns);

        let mut data = Vec::new();
        for i in 0..1000 {
            match i % 4 {
                0 => data.extend_from_slice(b"Hello, I'll help with that\n"),
                1 => data.extend_from_slice("⏺ Read file.rs\n".as_bytes()),
                2 => data.extend_from_slice(b"Cost: $0.01\n"),
                _ => data.extend_from_slice(b"\x1b[0mrandom ansi\x1b[0m\n"),
            }
        }

        let start = std::time::Instant::now();
        splitter.classify_chunk(&data);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "1000-line mixed classification should be under 50ms, took {}ms",
            elapsed.as_millis()
        );
    }
}
