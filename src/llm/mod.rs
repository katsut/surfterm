pub mod classifier;
pub mod expander;
pub mod reviewer;
pub mod summarizer;

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::instrument;

// ---------------------------------------------------------------------------
// LlmBackend trait
// ---------------------------------------------------------------------------

/// Trait abstracting an LLM inference backend.
///
/// Will be implemented by llama-cpp-2 in a later phase. For now a
/// [`MockLlmBackend`] is provided for testing.
pub trait LlmBackend: Send + Sync {
    /// Run inference on the given prompt.
    fn infer(&self, prompt: &str, max_tokens: u32, timeout_ms: u64) -> Result<String>;
}

// ---------------------------------------------------------------------------
// MockLlmBackend
// ---------------------------------------------------------------------------

/// A mock backend that returns fixed responses and optionally simulates
/// latency. Useful for testing without GPU/model dependencies.
#[allow(dead_code)]
pub struct MockLlmBackend {
    /// Artificial delay injected before returning a response.
    pub delay: Duration,
}

#[allow(dead_code)]
impl MockLlmBackend {
    pub fn new(delay: Duration) -> Self {
        Self { delay }
    }

    pub fn instant() -> Self {
        Self {
            delay: Duration::ZERO,
        }
    }
}

impl LlmBackend for MockLlmBackend {
    fn infer(&self, prompt: &str, _max_tokens: u32, timeout_ms: u64) -> Result<String> {
        if self.delay > Duration::from_millis(timeout_ms) {
            anyhow::bail!("inference timed out");
        }
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }

        // Return a classification-style response based on prompt content.
        let response = if prompt.contains("Classify") {
            if prompt.contains("Cost:") || prompt.contains("token") || prompt.contains("Read ") {
                "State".to_string()
            } else if prompt.contains("Hello") || prompt.contains("help") {
                "Message".to_string()
            } else {
                "Raw".to_string()
            }
        } else {
            format!("Mock response for: {}", &prompt[..prompt.len().min(40)])
        };

        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// LlmRuntime
// ---------------------------------------------------------------------------

/// Holds an optional LLM backend and provides a uniform inference interface.
///
/// When no backend is configured every inference call returns `Ok(None)`,
/// letting callers fall back to deterministic (regex) logic.
#[allow(dead_code)]
pub struct LlmRuntime {
    backend: Option<Box<dyn LlmBackend + Send + Sync>>,
}

#[allow(dead_code)]
impl LlmRuntime {
    /// Create a new LlmRuntime with no backend (disabled).
    pub fn new() -> Self {
        Self { backend: None }
    }

    /// Create a runtime with the given backend.
    #[instrument(skip_all)]
    pub fn new_with_backend(backend: Option<Box<dyn LlmBackend + Send + Sync>>) -> Self {
        Self { backend }
    }

    /// Create a runtime with no backend — all calls return `None`.
    #[instrument]
    pub fn new_disabled() -> Self {
        Self { backend: None }
    }

    /// Create a runtime that reports as available (for testing).
    /// Uses a [`MockLlmBackend`] with zero delay.
    #[cfg(test)]
    pub fn new_available() -> Self {
        Self {
            backend: Some(Box::new(MockLlmBackend::instant())),
        }
    }

    /// Returns `true` when an LLM backend is configured.
    pub fn is_available(&self) -> bool {
        self.backend.is_some()
    }

    /// Run inference. Returns `Ok(None)` when no backend is present.
    #[instrument(skip(self, prompt))]
    pub fn infer(
        &self,
        prompt: &str,
        max_tokens: u32,
        timeout_ms: u64,
    ) -> Result<Option<String>> {
        match &self.backend {
            Some(backend) => {
                let result = backend.infer(prompt, max_tokens, timeout_ms)?;
                Ok(Some(result))
            }
            None => Ok(None),
        }
    }
}

impl Default for LlmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// LlmTaskPriority
// ---------------------------------------------------------------------------

/// Priority levels for LLM tasks. Lower numeric value = higher priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LlmTaskPriority {
    /// Classify a stream chunk — must be fast (< 30 ms target).
    StreamClassify = 0,
    /// Expand a user prompt (< 500 ms target).
    PromptExpand = 1,
    /// Summarise a session (< 1 s target).
    SessionSummary = 2,
    /// Review generated code (< 2 s target) — lowest priority.
    CodeReview = 3,
}

impl Ord for LlmTaskPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower numeric value = higher priority = Greater in BinaryHeap.
        (*other as u8).cmp(&(*self as u8))
    }
}

impl PartialOrd for LlmTaskPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------------------
// LlmTask
// ---------------------------------------------------------------------------

/// A unit of work submitted to the [`LlmScheduler`].
#[allow(dead_code)]
pub struct LlmTask {
    pub priority: LlmTaskPriority,
    pub prompt: String,
    pub max_tokens: u32,
    pub timeout_ms: u64,
    pub response_tx: tokio::sync::oneshot::Sender<Result<String>>,
}

impl Eq for LlmTask {}

impl PartialEq for LlmTask {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Ord for LlmTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for LlmTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------------------
// LlmScheduler
// ---------------------------------------------------------------------------

/// A simple priority queue that dispatches [`LlmTask`]s to an [`LlmRuntime`].
#[allow(dead_code)]
pub struct LlmScheduler {
    queue: BinaryHeap<LlmTask>,
    runtime: Arc<LlmRuntime>,
}

#[allow(dead_code)]
impl LlmScheduler {
    pub fn new(runtime: Arc<LlmRuntime>) -> Self {
        Self {
            queue: BinaryHeap::new(),
            runtime,
        }
    }

    /// Enqueue a task for later processing.
    pub fn submit(&mut self, task: LlmTask) {
        self.queue.push(task);
    }

    /// Pop the highest-priority task, run inference, and send the result
    /// through the task's oneshot channel. Returns `None` when the queue is
    /// empty.
    #[instrument(skip(self))]
    pub fn process_next(&mut self) -> Option<()> {
        let task = self.queue.pop()?;
        let result = self
            .runtime
            .infer(&task.prompt, task.max_tokens, task.timeout_ms);

        let send_result = match result {
            Ok(Some(text)) => Ok(text),
            Ok(None) => Err(anyhow::anyhow!("LLM backend unavailable")),
            Err(e) => Err(e),
        };

        // Ignore send error — the receiver may have been dropped.
        let _ = task.response_tx.send(send_result);
        Some(())
    }

    /// Number of tasks waiting in the queue.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Remove all pending tasks. Response channels will be dropped
    /// (receivers will see a `RecvError`).
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- LlmRuntime tests --

    #[test]
    fn test_runtime_new_with_backend() {
        let backend = MockLlmBackend::instant();
        let runtime = LlmRuntime::new_with_backend(Some(Box::new(backend)));
        assert!(runtime.is_available());
    }

    #[test]
    fn test_runtime_new_disabled() {
        let runtime = LlmRuntime::new_disabled();
        assert!(!runtime.is_available());
    }

    #[test]
    fn test_runtime_new_default_disabled() {
        let runtime = LlmRuntime::new();
        assert!(!runtime.is_available());
    }

    #[test]
    fn test_runtime_infer_with_mock() {
        let backend = MockLlmBackend::instant();
        let runtime = LlmRuntime::new_with_backend(Some(Box::new(backend)));
        let result = runtime.infer("Hello world", 100, 1000).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_runtime_infer_disabled_returns_none() {
        let runtime = LlmRuntime::new_disabled();
        let result = runtime.infer("Hello world", 100, 1000).unwrap();
        assert!(result.is_none());
    }

    // -- MockLlmBackend tests --

    #[test]
    fn test_mock_returns_responses() {
        let mock = MockLlmBackend::instant();
        let resp = mock.infer("Classify: Hello", 100, 1000).unwrap();
        assert!(!resp.is_empty());
    }

    #[test]
    fn test_mock_respects_timeout() {
        let mock = MockLlmBackend::new(Duration::from_millis(100));
        let result = mock.infer("test", 100, 10);
        assert!(result.is_err());
    }

    // -- LlmTask ordering tests --

    #[test]
    fn test_task_priority_ordering() {
        assert!(LlmTaskPriority::StreamClassify > LlmTaskPriority::CodeReview);
        assert!(LlmTaskPriority::StreamClassify > LlmTaskPriority::PromptExpand);
        assert!(LlmTaskPriority::PromptExpand > LlmTaskPriority::SessionSummary);
        assert!(LlmTaskPriority::SessionSummary > LlmTaskPriority::CodeReview);
    }

    #[test]
    fn test_stream_classify_before_code_review() {
        let mut heap = BinaryHeap::new();

        let (tx1, _rx1) = tokio::sync::oneshot::channel();
        let (tx2, _rx2) = tokio::sync::oneshot::channel();

        heap.push(LlmTask {
            priority: LlmTaskPriority::CodeReview,
            prompt: "review".into(),
            max_tokens: 200,
            timeout_ms: 2000,
            response_tx: tx1,
        });
        heap.push(LlmTask {
            priority: LlmTaskPriority::StreamClassify,
            prompt: "classify".into(),
            max_tokens: 50,
            timeout_ms: 30,
            response_tx: tx2,
        });

        let first = heap.pop().unwrap();
        assert_eq!(first.priority, LlmTaskPriority::StreamClassify);

        let second = heap.pop().unwrap();
        assert_eq!(second.priority, LlmTaskPriority::CodeReview);
    }

    // -- LlmScheduler tests --

    #[test]
    fn test_scheduler_submit_and_queue_len() {
        let runtime = Arc::new(LlmRuntime::new_disabled());
        let mut scheduler = LlmScheduler::new(runtime);

        assert_eq!(scheduler.queue_len(), 0);

        let (tx, _rx) = tokio::sync::oneshot::channel();
        scheduler.submit(LlmTask {
            priority: LlmTaskPriority::StreamClassify,
            prompt: "test".into(),
            max_tokens: 50,
            timeout_ms: 30,
            response_tx: tx,
        });

        assert_eq!(scheduler.queue_len(), 1);
    }

    #[test]
    fn test_scheduler_process_next_highest_priority() {
        let backend = MockLlmBackend::instant();
        let runtime = Arc::new(LlmRuntime::new_with_backend(Some(Box::new(backend))));
        let mut scheduler = LlmScheduler::new(runtime);

        let (tx1, mut rx1) = tokio::sync::oneshot::channel();
        let (tx2, mut rx2) = tokio::sync::oneshot::channel();

        scheduler.submit(LlmTask {
            priority: LlmTaskPriority::CodeReview,
            prompt: "review code".into(),
            max_tokens: 200,
            timeout_ms: 2000,
            response_tx: tx1,
        });
        scheduler.submit(LlmTask {
            priority: LlmTaskPriority::StreamClassify,
            prompt: "Classify: Hello help me".into(),
            max_tokens: 50,
            timeout_ms: 30,
            response_tx: tx2,
        });

        // First process should handle StreamClassify (higher priority).
        assert!(scheduler.process_next().is_some());
        assert_eq!(scheduler.queue_len(), 1);

        // The StreamClassify response should arrive.
        let resp2 = rx2.try_recv().unwrap();
        assert!(resp2.is_ok());

        // rx1 should still be waiting.
        assert!(rx1.try_recv().is_err());

        // Process the remaining CodeReview task.
        assert!(scheduler.process_next().is_some());
        let resp1 = rx1.try_recv().unwrap();
        assert!(resp1.is_ok());
    }

    #[test]
    fn test_scheduler_process_next_empty() {
        let runtime = Arc::new(LlmRuntime::new_disabled());
        let mut scheduler = LlmScheduler::new(runtime);
        assert!(scheduler.process_next().is_none());
    }

    #[test]
    fn test_scheduler_clear() {
        let runtime = Arc::new(LlmRuntime::new_disabled());
        let mut scheduler = LlmScheduler::new(runtime);

        let (tx, _rx) = tokio::sync::oneshot::channel();
        scheduler.submit(LlmTask {
            priority: LlmTaskPriority::SessionSummary,
            prompt: "summarize".into(),
            max_tokens: 100,
            timeout_ms: 1000,
            response_tx: tx,
        });

        assert_eq!(scheduler.queue_len(), 1);
        scheduler.clear();
        assert_eq!(scheduler.queue_len(), 0);
    }
}
