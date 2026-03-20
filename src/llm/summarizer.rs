use std::sync::Arc;

use tracing::instrument;

use super::LlmRuntime;

/// Summarizes a session's conversation history into 1-2 short lines.
///
/// Falls back gracefully: if the LLM is unavailable, returns `None` or
/// a truncated version of the last message.
#[allow(dead_code)]
pub struct SessionSummarizer {
    runtime: Arc<LlmRuntime>,
    timeout_ms: u64,
}

#[allow(dead_code)]
impl SessionSummarizer {
    /// Default timeout in milliseconds for summarization.
    const DEFAULT_TIMEOUT_MS: u64 = 1000;

    /// Maximum tokens to generate for a summary.
    const MAX_TOKENS: u32 = 128;

    pub fn new(runtime: Arc<LlmRuntime>) -> Self {
        Self {
            runtime,
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
        }
    }

    /// Create with a custom timeout.
    pub fn with_timeout(runtime: Arc<LlmRuntime>, timeout_ms: u64) -> Self {
        Self {
            runtime,
            timeout_ms,
        }
    }

    /// Summarize a conversation history into 1-2 short lines.
    ///
    /// Returns `None` if the LLM is unavailable or the history is empty.
    #[instrument(skip(self, conversation_history))]
    pub fn summarize(&self, conversation_history: &[String]) -> Option<String> {
        if conversation_history.is_empty() {
            return None;
        }

        if !self.runtime.is_available() {
            return None;
        }

        let joined = conversation_history.join("\n");
        let prompt = format!(
            "Summarize this AI coding session in 1-2 short lines: {}",
            joined
        );

        match self.runtime.infer(&prompt, Self::MAX_TOKENS, self.timeout_ms) {
            Ok(Some(summary)) if !summary.trim().is_empty() => Some(summary),
            _ => None,
        }
    }

    /// Summarize using LLM, falling back to a truncated last message.
    ///
    /// Always returns a non-empty string:
    /// - Tries LLM summary first
    /// - Falls back to the last message in the history, truncated to `max_len`
    /// - Returns "(empty session)" if the history is empty
    #[instrument(skip(self, conversation_history))]
    pub fn summarize_or_truncate(
        &self,
        conversation_history: &[String],
        max_len: usize,
    ) -> String {
        // Try LLM summary first.
        if let Some(summary) = self.summarize(conversation_history) {
            return truncate_str(&summary, max_len);
        }

        // Fallback: truncate last message.
        match conversation_history.last() {
            Some(last) => truncate_str(last, max_len),
            None => "(empty session)".to_string(),
        }
    }

    /// Check whether the underlying LLM runtime is available.
    pub fn is_available(&self) -> bool {
        self.runtime.is_available()
    }
}

/// Truncate a string to at most `max_len` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else if max_len <= 3 {
        trimmed.chars().take(max_len).collect()
    } else {
        let mut result: String = trimmed.chars().take(max_len - 3).collect();
        result.push_str("...");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable_runtime() -> Arc<LlmRuntime> {
        Arc::new(LlmRuntime::new())
    }

    #[test]
    fn test_summarize_disabled_runtime_returns_none() {
        let summarizer = SessionSummarizer::new(unavailable_runtime());
        let history = vec!["Hello".to_string(), "Fix the bug".to_string()];
        assert!(summarizer.summarize(&history).is_none());
    }

    #[test]
    fn test_summarize_empty_history_returns_none() {
        let summarizer = SessionSummarizer::new(unavailable_runtime());
        assert!(summarizer.summarize(&[]).is_none());
    }

    #[test]
    fn test_summarize_or_truncate_fallback_to_last_message() {
        let summarizer = SessionSummarizer::new(unavailable_runtime());
        let history = vec![
            "First message".to_string(),
            "This is a very long last message that should be truncated".to_string(),
        ];
        let result = summarizer.summarize_or_truncate(&history, 20);
        assert_eq!(result, "This is a very lo...");
        assert!(result.len() <= 20);
    }

    #[test]
    fn test_summarize_or_truncate_short_last_message() {
        let summarizer = SessionSummarizer::new(unavailable_runtime());
        let history = vec!["Short".to_string()];
        let result = summarizer.summarize_or_truncate(&history, 20);
        assert_eq!(result, "Short");
    }

    #[test]
    fn test_summarize_or_truncate_empty_history() {
        let summarizer = SessionSummarizer::new(unavailable_runtime());
        let result = summarizer.summarize_or_truncate(&[], 20);
        assert_eq!(result, "(empty session)");
    }

    #[test]
    fn test_truncate_str_no_truncation() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact_length() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_with_ellipsis() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_str_very_short_max() {
        assert_eq!(truncate_str("hello", 2), "he");
    }

    #[test]
    fn test_truncate_str_trims_whitespace() {
        assert_eq!(truncate_str("  hello  ", 10), "hello");
    }

    #[test]
    fn test_summarize_with_available_runtime_returns_some() {
        let rt = Arc::new(LlmRuntime::new_available());
        let summarizer = SessionSummarizer::new(rt);
        let history = vec!["message".to_string()];
        // MockLlmBackend returns a non-empty response.
        assert!(summarizer.summarize(&history).is_some());
    }

    #[test]
    fn test_summarize_or_truncate_with_available_runtime_uses_llm() {
        let rt = Arc::new(LlmRuntime::new_available());
        let summarizer = SessionSummarizer::new(rt);
        let history = vec!["last message here".to_string()];
        // MockLlmBackend returns a response, so LLM result is used.
        let result = summarizer.summarize_or_truncate(&history, 50);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_with_custom_timeout() {
        let summarizer = SessionSummarizer::with_timeout(unavailable_runtime(), 2000);
        assert_eq!(summarizer.timeout_ms, 2000);
    }
}
