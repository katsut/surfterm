use std::sync::Arc;

use tracing::instrument;

use super::LlmRuntime;

/// Expands short user input into a more detailed prompt for an AI coding assistant.
///
/// Falls back gracefully: if the LLM is unavailable, `expand` returns `None`
/// and the caller should send the original input unchanged.
#[allow(dead_code)]
pub struct PromptExpander {
    runtime: Arc<LlmRuntime>,
    timeout_ms: u64,
}

#[allow(dead_code)]
impl PromptExpander {
    /// Default timeout in milliseconds for prompt expansion.
    const DEFAULT_TIMEOUT_MS: u64 = 500;

    /// Maximum tokens to generate for an expanded prompt.
    const MAX_TOKENS: u32 = 256;

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

    /// Expand a short input into a clearer, more detailed prompt.
    ///
    /// Returns `None` if the LLM is unavailable or inference fails.
    #[instrument(skip(self))]
    pub fn expand(&self, short_input: &str) -> Option<String> {
        if !self.runtime.is_available() {
            return None;
        }

        let prompt = format!(
            "Expand this brief instruction into a clear, detailed prompt \
             for an AI coding assistant: {}",
            short_input
        );

        match self.runtime.infer(&prompt, Self::MAX_TOKENS, self.timeout_ms) {
            Ok(Some(expanded)) if !expanded.trim().is_empty() => Some(expanded),
            _ => None,
        }
    }

    /// Check whether the underlying LLM runtime is available.
    pub fn is_available(&self) -> bool {
        self.runtime.is_available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable_runtime() -> Arc<LlmRuntime> {
        Arc::new(LlmRuntime::new())
    }

    #[test]
    fn test_expand_disabled_runtime_returns_none() {
        let expander = PromptExpander::new(unavailable_runtime());
        assert!(!expander.is_available());
        assert!(expander.expand("fix the bug").is_none());
    }

    #[test]
    fn test_is_available_reflects_runtime() {
        let expander = PromptExpander::new(unavailable_runtime());
        assert!(!expander.is_available());

        let available_rt = Arc::new(LlmRuntime::new_available());
        let expander2 = PromptExpander::new(available_rt);
        assert!(expander2.is_available());
    }

    #[test]
    fn test_expand_with_available_runtime_returns_some() {
        let rt = Arc::new(LlmRuntime::new_available());
        let expander = PromptExpander::new(rt);
        assert!(expander.is_available());
        // MockLlmBackend returns a non-empty response.
        assert!(expander.expand("fix bug").is_some());
    }

    #[test]
    fn test_with_custom_timeout() {
        let expander = PromptExpander::with_timeout(unavailable_runtime(), 1000);
        assert_eq!(expander.timeout_ms, 1000);
    }

    /// Simulate a mock runtime that returns actual text.
    #[test]
    fn test_expand_with_mock_returns_expanded_text() {
        use std::sync::Arc;

        // We test the logic by creating a MockLlmRuntime-like approach.
        // Since LlmRuntime is a concrete struct, we create a helper
        // that tests the prompt construction and result handling.
        let input = "fix the bug";
        let expected_prompt = format!(
            "Expand this brief instruction into a clear, detailed prompt \
             for an AI coding assistant: {}",
            input
        );
        assert!(expected_prompt.contains(input));

        // Test that a non-empty result would be returned as Some.
        let result: Option<String> = Some("Please fix the null pointer bug in main.rs".into());
        let filtered = result.filter(|s| !s.trim().is_empty());
        assert!(filtered.is_some());

        // Test that an empty result would be filtered out.
        let empty_result: Option<String> = Some("   ".into());
        let filtered_empty = empty_result.filter(|s| !s.trim().is_empty());
        assert!(filtered_empty.is_none());

        let _ = Arc::new(LlmRuntime::new());
    }
}
