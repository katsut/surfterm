use std::sync::Arc;

use tracing::instrument;

use super::LlmRuntime;
use crate::session::stream_splitter::Classification;

/// Uses an LLM to classify terminal output as Message, State, or Raw.
///
/// Acts as a fallback/enhancement layer on top of regex-based classification.
/// When the LLM is unavailable or times out, methods return `None` so callers
/// can fall back to the regex result.
#[allow(dead_code)]
pub struct LlmClassifier {
    runtime: Arc<LlmRuntime>,
    timeout_ms: u64,
}

#[allow(dead_code)]
impl LlmClassifier {
    /// Default timeout in milliseconds for classification inference.
    const DEFAULT_TIMEOUT_MS: u64 = 30;

    /// Maximum tokens to generate for a classification response.
    const MAX_TOKENS: u32 = 16;

    pub fn new(runtime: Arc<LlmRuntime>) -> Self {
        Self {
            runtime,
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
        }
    }

    /// Ask the LLM to classify terminal output.
    ///
    /// Returns `None` if the LLM is unavailable, times out, or returns an
    /// unparseable response. The caller should fall back to regex
    /// classification in that case.
    #[instrument(skip(self, text))]
    pub fn classify(&self, text: &str) -> Option<Classification> {
        if !self.runtime.is_available() {
            return None;
        }

        let prompt = format!(
            "Classify this terminal output as Message, State, or Raw: {}",
            text
        );

        match self.runtime.infer(&prompt, Self::MAX_TOKENS, self.timeout_ms) {
            Ok(Some(response)) => parse_classification(&response),
            _ => None,
        }
    }

    /// Classify with regex fallback: if regex already gave a non-Raw result,
    /// trust it; otherwise try the LLM for a better answer.
    #[instrument(skip(self, text))]
    pub fn classify_with_fallback(
        &self,
        text: &str,
        regex_result: Classification,
    ) -> Classification {
        if regex_result != Classification::Raw {
            return regex_result;
        }

        // Regex said Raw — see if the LLM can do better.
        self.classify(text).unwrap_or(Classification::Raw)
    }
}

/// Parse a free-text LLM response into a [`Classification`].
fn parse_classification(response: &str) -> Option<Classification> {
    let lower = response.to_lowercase();
    if lower.contains("message") {
        Some(Classification::Message)
    } else if lower.contains("state") {
        Some(Classification::State)
    } else if lower.contains("raw") {
        Some(Classification::Raw)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_runtime() -> Arc<LlmRuntime> {
        Arc::new(LlmRuntime::new_available())
    }

    fn disabled_runtime() -> Arc<LlmRuntime> {
        Arc::new(LlmRuntime::new())
    }

    #[test]
    fn test_classify_returns_message() {
        let classifier = LlmClassifier::new(mock_runtime());
        // MockLlmBackend returns "Message" when prompt contains "Hello"
        let result = classifier.classify("Hello world");
        assert_eq!(result, Some(Classification::Message));
    }

    #[test]
    fn test_classify_returns_state() {
        let classifier = LlmClassifier::new(mock_runtime());
        // MockLlmBackend returns "State" when prompt contains "Cost:"
        let result = classifier.classify("Cost: $0.05");
        assert_eq!(result, Some(Classification::State));
    }

    #[test]
    fn test_classify_returns_raw() {
        let classifier = LlmClassifier::new(mock_runtime());
        // MockLlmBackend returns "Raw" for unrecognized content
        let result = classifier.classify("\x1b[32mansi\x1b[0m");
        assert_eq!(result, Some(Classification::Raw));
    }

    #[test]
    fn test_classify_disabled_returns_none() {
        let classifier = LlmClassifier::new(disabled_runtime());
        let result = classifier.classify("Hello world");
        assert!(result.is_none());
    }

    #[test]
    fn test_classify_with_fallback_trusts_regex_non_raw() {
        let classifier = LlmClassifier::new(mock_runtime());
        // When regex says Message, we trust it regardless of LLM.
        let result = classifier.classify_with_fallback("anything", Classification::Message);
        assert_eq!(result, Classification::Message);
    }

    #[test]
    fn test_classify_with_fallback_tries_llm_on_raw() {
        let classifier = LlmClassifier::new(mock_runtime());
        // Regex said Raw, but the text contains "Hello" so LLM returns Message.
        let result = classifier.classify_with_fallback("Hello there", Classification::Raw);
        assert_eq!(result, Classification::Message);
    }

    #[test]
    fn test_classify_with_fallback_keeps_raw_when_disabled() {
        let classifier = LlmClassifier::new(disabled_runtime());
        let result = classifier.classify_with_fallback("Hello there", Classification::Raw);
        assert_eq!(result, Classification::Raw);
    }

    #[test]
    fn test_parse_classification_message() {
        assert_eq!(
            parse_classification("Message"),
            Some(Classification::Message)
        );
        assert_eq!(
            parse_classification("message"),
            Some(Classification::Message)
        );
    }

    #[test]
    fn test_parse_classification_state() {
        assert_eq!(
            parse_classification("State"),
            Some(Classification::State)
        );
    }

    #[test]
    fn test_parse_classification_raw() {
        assert_eq!(parse_classification("Raw"), Some(Classification::Raw));
    }

    #[test]
    fn test_parse_classification_unknown() {
        assert_eq!(parse_classification("unknown output"), None);
    }
}
