use std::sync::Arc;

use tracing::instrument;

use super::LlmRuntime;

/// Severity level for a code review issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A single issue found during code review.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct ReviewIssue {
    pub severity: Severity,
    pub message: String,
    pub line: Option<usize>,
}

/// The result of a code review.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ReviewResult {
    pub issues: Vec<ReviewIssue>,
    pub summary: String,
}

#[allow(dead_code)]
impl ReviewResult {
    /// Create an empty review result.
    pub fn empty() -> Self {
        Self {
            issues: Vec::new(),
            summary: String::new(),
        }
    }

    /// Check if the review found any issues.
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    /// Count issues by severity.
    pub fn count_by_severity(&self, severity: Severity) -> usize {
        self.issues.iter().filter(|i| i.severity == severity).count()
    }
}

/// Reviews code and diffs using the local LLM.
///
/// Falls back gracefully: if the LLM is unavailable, all review methods return `None`.
#[allow(dead_code)]
pub struct CodeReviewer {
    runtime: Arc<LlmRuntime>,
    timeout_ms: u64,
}

#[allow(dead_code)]
impl CodeReviewer {
    /// Default timeout in milliseconds for code review.
    const DEFAULT_TIMEOUT_MS: u64 = 2000;

    /// Maximum tokens to generate for a review.
    const MAX_TOKENS: u32 = 512;

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

    /// Review code for bugs, security issues, and improvements.
    ///
    /// Returns `None` if the LLM is unavailable.
    #[instrument(skip(self, code))]
    pub fn review(&self, code: &str, language: &str) -> Option<ReviewResult> {
        if !self.runtime.is_available() {
            return None;
        }

        let prompt = format!(
            "Review this {} code for bugs, security issues, and improvements. \
             List issues as JSON: {}",
            language, code
        );

        match self.runtime.infer(&prompt, Self::MAX_TOKENS, self.timeout_ms) {
            Ok(Some(response)) if !response.trim().is_empty() => {
                Some(parse_review_response(&response))
            }
            _ => None,
        }
    }

    /// Review a diff for issues.
    ///
    /// Returns `None` if the LLM is unavailable.
    #[instrument(skip(self, diff))]
    pub fn review_diff(&self, diff: &str) -> Option<ReviewResult> {
        if !self.runtime.is_available() {
            return None;
        }

        let prompt = format!(
            "Review this code diff for bugs, security issues, and improvements. \
             List issues as JSON: {}",
            diff
        );

        match self.runtime.infer(&prompt, Self::MAX_TOKENS, self.timeout_ms) {
            Ok(Some(response)) if !response.trim().is_empty() => {
                Some(parse_review_response(&response))
            }
            _ => None,
        }
    }

    /// Check whether the underlying LLM runtime is available.
    pub fn is_available(&self) -> bool {
        self.runtime.is_available()
    }
}

/// Parse the LLM response into a `ReviewResult`.
///
/// In a real implementation this would parse JSON from the LLM output.
/// For now, it treats the entire response as a summary with no structured issues.
fn parse_review_response(response: &str) -> ReviewResult {
    // Attempt basic JSON parsing of issues. If the response doesn't
    // parse cleanly, treat the whole thing as a summary.
    // Full implementation would use serde_json to parse structured output.
    ReviewResult {
        issues: Vec::new(),
        summary: response.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable_runtime() -> Arc<LlmRuntime> {
        Arc::new(LlmRuntime::new())
    }

    #[test]
    fn test_review_disabled_runtime_returns_none() {
        let reviewer = CodeReviewer::new(unavailable_runtime());
        assert!(!reviewer.is_available());
        assert!(reviewer.review("fn main() {}", "rust").is_none());
    }

    #[test]
    fn test_review_diff_disabled_runtime_returns_none() {
        let reviewer = CodeReviewer::new(unavailable_runtime());
        assert!(reviewer.review_diff("+ new line\n- old line").is_none());
    }

    #[test]
    fn test_is_available_reflects_runtime() {
        let reviewer = CodeReviewer::new(unavailable_runtime());
        assert!(!reviewer.is_available());

        let available_rt = Arc::new(LlmRuntime::new_available());
        let reviewer2 = CodeReviewer::new(available_rt);
        assert!(reviewer2.is_available());
    }

    #[test]
    fn test_review_with_available_runtime_returns_some() {
        let rt = Arc::new(LlmRuntime::new_available());
        let reviewer = CodeReviewer::new(rt);
        // MockLlmBackend returns a non-empty response.
        let result = reviewer.review("fn main() {}", "rust");
        assert!(result.is_some());
    }

    #[test]
    fn test_review_diff_with_available_runtime_returns_some() {
        let rt = Arc::new(LlmRuntime::new_available());
        let reviewer = CodeReviewer::new(rt);
        let result = reviewer.review_diff("+ line");
        assert!(result.is_some());
    }

    #[test]
    fn test_review_result_empty() {
        let result = ReviewResult::empty();
        assert!(!result.has_issues());
        assert_eq!(result.count_by_severity(Severity::Error), 0);
        assert!(result.summary.is_empty());
    }

    #[test]
    fn test_review_result_with_issues() {
        let result = ReviewResult {
            issues: vec![
                ReviewIssue {
                    severity: Severity::Error,
                    message: "Null pointer dereference".to_string(),
                    line: Some(42),
                },
                ReviewIssue {
                    severity: Severity::Warning,
                    message: "Unused variable".to_string(),
                    line: Some(10),
                },
                ReviewIssue {
                    severity: Severity::Info,
                    message: "Consider using a constant".to_string(),
                    line: None,
                },
                ReviewIssue {
                    severity: Severity::Error,
                    message: "Buffer overflow".to_string(),
                    line: Some(55),
                },
            ],
            summary: "Found 2 errors, 1 warning, 1 info".to_string(),
        };

        assert!(result.has_issues());
        assert_eq!(result.count_by_severity(Severity::Error), 2);
        assert_eq!(result.count_by_severity(Severity::Warning), 1);
        assert_eq!(result.count_by_severity(Severity::Info), 1);
    }

    #[test]
    fn test_review_issue_structure() {
        let issue = ReviewIssue {
            severity: Severity::Warning,
            message: "Test issue".to_string(),
            line: Some(5),
        };
        assert_eq!(issue.severity, Severity::Warning);
        assert_eq!(issue.message, "Test issue");
        assert_eq!(issue.line, Some(5));
    }

    #[test]
    fn test_severity_variants() {
        assert_ne!(Severity::Info, Severity::Warning);
        assert_ne!(Severity::Warning, Severity::Error);
        assert_ne!(Severity::Info, Severity::Error);
    }

    #[test]
    fn test_parse_review_response() {
        let result = parse_review_response("  Some review text  ");
        assert_eq!(result.summary, "Some review text");
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_with_custom_timeout() {
        let reviewer = CodeReviewer::with_timeout(unavailable_runtime(), 5000);
        assert_eq!(reviewer.timeout_ms, 5000);
    }
}
