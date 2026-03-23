//! End-to-end tests for Surfterm.
//!
//! These tests exercise full pipelines across multiple modules, simulating
//! realistic usage scenarios without requiring a GPU or BLE hardware.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use surfterm::ble::gatt::{GattService, SessionStatusData};
use surfterm::ble::{ChunkProtocol, Chunk};
use surfterm::config::ConfigEngine;
use surfterm::detector::patterns::{default_claude_code_state_patterns, load_patterns_from_toml};
use surfterm::detector::StateDetector;
use surfterm::input::{InputAction, InputHandler, InputMode};
use surfterm::layer::transition::{apply_state_change, apply_user_input, TransitionEvent};
use surfterm::layer::{Layer, LayerController};
use surfterm::llm::classifier::LlmClassifier;
use surfterm::llm::expander::PromptExpander;
use surfterm::llm::reviewer::CodeReviewer;
use surfterm::llm::summarizer::SessionSummarizer;
use surfterm::llm::{
    LlmRuntime, LlmScheduler, LlmTask, LlmTaskPriority, MockLlmBackend,
};
use surfterm::preview::diff::{compute_diff, DiffLine};
use surfterm::preview::syntax::SyntaxHighlighter;
use surfterm::preview::watcher::ToolOutputMonitor;
use surfterm::renderer::panel::{MessagePanel, StatePanel};
use surfterm::session::state::SessionState;
use surfterm::session::stream_splitter::{Classification, StreamSplitter};
use surfterm::session::terminal::Terminal;
use surfterm::session::SessionId;

use winit::keyboard::{Key, NamedKey, SmolStr};

// ═══════════════════════════════════════════════════════════════════════════
// Test fixtures and helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Simulates a Claude Code session output with tool use, conversation, and
/// state changes. Returns a sequence of (classification_hint, raw_text) lines
/// representing what would come from a PTY.
fn claude_code_session_fixture() -> Vec<&'static str> {
    vec![
        "Hello, I'll help you refactor the authentication module.",
        "Let me start by reading the existing code.",
        "⏺ Read src/auth/mod.rs",
        "  Reading file contents...",
        "The current implementation has a few issues:",
        "1. No password hashing",
        "2. Missing rate limiting",
        "## Proposed Changes",
        "- Add bcrypt hashing",
        "- Add login attempt throttling",
        "Would you like to proceed?",
        "⏺ Edit src/auth/mod.rs",
        "  Writing changes to file...",
        "⏺ Bash cargo test",
        "  Running cargo test...",
        "Cost: $0.12",
        "Token usage: 4523 tokens",
        "Error: test auth::tests::test_login failed",
        "⏺ Edit src/auth/mod.rs",
        "  Fixing the test...",
        "Running cargo test again...",
        "All tests passed!",
        "Do you want to continue?",
    ]
}

/// Simulates multiple sessions with different projects.
fn multi_session_fixture() -> Vec<(String, Vec<&'static str>)> {
    vec![
        (
            "api-server".to_string(),
            vec![
                "⏺ Read src/routes.rs",
                "Hello, I'll help you add the new endpoint.",
                "⏺ Edit src/routes.rs",
                "Running cargo build...",
                "Would you like to proceed?",
            ],
        ),
        (
            "web-frontend".to_string(),
            vec![
                "⏺ Read src/App.tsx",
                "I'll fix the React component.",
                "⏺ Edit src/App.tsx",
                "Error: TypeScript compilation failed",
            ],
        ),
        (
            "cli-tool".to_string(),
            vec![
                "Hello, I'll help you with the CLI parser.",
                "⏺ Read src/main.rs",
                "⏺ Bash cargo run -- --help",
                "Searching for similar patterns...",
            ],
        ),
    ]
}

fn char_key(c: &str) -> Key {
    Key::Character(SmolStr::new(c))
}

fn named_key(k: NamedKey) -> Key {
    Key::Named(k)
}

/// Create a temporary directory for test config files.
fn tempdir(suffix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "surfterm_e2e_{suffix}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Full session lifecycle E2E
// ═══════════════════════════════════════════════════════════════════════════

/// Tests the full pipeline: PTY output → Terminal → StreamSplitter →
/// StateDetector, verifying classification and state transitions through a
/// simulated Claude Code session.
#[test]
fn e2e_full_session_lifecycle() {
    let fixture = claude_code_session_fixture();

    // Create the pipeline components.
    let mut terminal = Terminal::new(120, 40);
    let splitter_patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(splitter_patterns);
    let state_patterns = default_claude_code_state_patterns();
    let (mut detector, state_rx) = StateDetector::new(state_patterns);

    assert_eq!(detector.current_state(), SessionState::Idle);

    let mut message_count = 0usize;
    let mut state_count = 0usize;
    let mut raw_count = 0usize;
    let mut saw_running = false;
    let mut saw_waiting = false;
    let mut saw_error = false;

    for line in &fixture {
        // Feed through terminal emulator.
        let terminal_data = format!("{line}\r\n");
        terminal.feed(terminal_data.as_bytes());

        // Classify the line.
        splitter.classify_chunk(line.as_bytes());

        // Drain all channels and feed state chunks to the detector.
        while let Ok(chunk) = channels.message_rx.try_recv() {
            assert_eq!(chunk.classification, Classification::Message);
            message_count += 1;
        }
        while let Ok(chunk) = channels.state_rx.try_recv() {
            assert_eq!(chunk.classification, Classification::State);
            detector.process_chunk(&chunk.data);
            state_count += 1;
        }
        while let Ok(chunk) = channels.raw_rx.try_recv() {
            assert_eq!(chunk.classification, Classification::Raw);
            raw_count += 1;
        }

        // Also feed the raw line to the detector (it has its own patterns).
        detector.process_chunk(line.as_bytes());

        match detector.current_state() {
            SessionState::Running => saw_running = true,
            SessionState::WaitingForInput => saw_waiting = true,
            SessionState::Error => saw_error = true,
            _ => {}
        }
    }

    // Verify we saw all three categories of output.
    assert!(message_count > 0, "should have classified some Message chunks");
    assert!(state_count > 0, "should have classified some State chunks");
    assert!(raw_count > 0, "should have classified some Raw chunks");

    // Verify the detector saw meaningful state transitions.
    assert!(saw_running, "detector should have entered Running state");
    assert!(saw_waiting, "detector should have entered WaitingForInput state");
    assert!(saw_error, "detector should have entered Error state");

    // Verify the terminal has content from the session.
    let content = terminal.content();
    let first_row: String = content.rows[0].iter().take(5).map(|c| c.c).collect();
    assert_eq!(first_row, "Hello");

    // The watch receiver should reflect the final state.
    let final_state = *state_rx.borrow();
    // Last line is "Do you want to continue?" which matches WaitingForInput.
    assert_eq!(final_state, SessionState::WaitingForInput);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Multi-session with LayerController E2E
// ═══════════════════════════════════════════════════════════════════════════

/// Tests creating multiple sessions with a LayerController, verifying
/// automatic state-driven transitions and pinning behavior.
#[test]
fn e2e_multi_session_layer_transitions() {
    let mut layer_ctrl = LayerController::new();
    let sessions = multi_session_fixture();

    // Create session IDs and assign initial layers.
    let ids: Vec<SessionId> = (0..sessions.len()).map(|_| SessionId::new()).collect();

    // All sessions start in Background.
    for &id in &ids {
        layer_ctrl.assign(id, Layer::Background);
    }

    // Create detectors for each session.
    let mut detectors: Vec<(StateDetector, tokio::sync::watch::Receiver<SessionState>)> =
        ids.iter()
            .map(|_| StateDetector::new(default_claude_code_state_patterns()))
            .collect();

    // Simulate session 0 going to WaitingForInput.
    detectors[0].0.process_chunk(b"Would you like to proceed?");
    assert_eq!(detectors[0].0.current_state(), SessionState::WaitingForInput);

    let event = apply_state_change(
        &mut layer_ctrl,
        &ids[0],
        SessionState::Running,
        SessionState::WaitingForInput,
    );
    assert_eq!(event, TransitionEvent::MovedToForeground(ids[0]));
    assert_eq!(layer_ctrl.get_layer(&ids[0]), Some(Layer::Foreground));

    // Simulate session 1 encountering an error.
    detectors[1].0.process_chunk(b"Error: TypeScript compilation failed");
    assert_eq!(detectors[1].0.current_state(), SessionState::Error);

    let event = apply_state_change(
        &mut layer_ctrl,
        &ids[1],
        SessionState::Running,
        SessionState::Error,
    );
    assert_eq!(event, TransitionEvent::MovedToForeground(ids[1]));
    assert_eq!(layer_ctrl.get_layer(&ids[1]), Some(Layer::Foreground));

    // Both sessions 0 and 1 should be in foreground now.
    let fg = layer_ctrl.foreground_sessions();
    assert!(fg.contains(&ids[0]));
    assert!(fg.contains(&ids[1]));
    assert!(!fg.contains(&ids[2]));

    // Simulate user sending input to session 0 → moves to Background.
    let event = apply_user_input(&mut layer_ctrl, &ids[0]);
    assert_eq!(event, TransitionEvent::MovedToBackground(ids[0]));
    assert_eq!(layer_ctrl.get_layer(&ids[0]), Some(Layer::Background));

    // Pin session 2.
    layer_ctrl.pin(&ids[2]);
    assert_eq!(layer_ctrl.get_layer(&ids[2]), Some(Layer::Pinned));

    // Verify pinned session stays pinned even with state changes.
    let event = apply_state_change(
        &mut layer_ctrl,
        &ids[2],
        SessionState::Running,
        SessionState::WaitingForInput,
    );
    assert_eq!(event, TransitionEvent::NoChange);
    assert_eq!(layer_ctrl.get_layer(&ids[2]), Some(Layer::Pinned));

    // Pinned session should be primary foreground.
    assert_eq!(layer_ctrl.primary_foreground(), Some(ids[2]));

    // Kill session 1 — should be removed from tracking.
    layer_ctrl.remove(&ids[1]);
    assert_eq!(layer_ctrl.get_layer(&ids[1]), None);
    assert!(!layer_ctrl.foreground_sessions().contains(&ids[1]));
    assert!(!layer_ctrl.background_sessions().contains(&ids[1]));

    // Kill session 2 — should be removed.
    layer_ctrl.remove(&ids[2]);
    assert_eq!(layer_ctrl.get_layer(&ids[2]), None);

    // Only session 0 remains (in Background).
    assert_eq!(layer_ctrl.background_sessions().len(), 1);
    assert!(layer_ctrl.background_sessions().contains(&ids[0]));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Config → Detection pipeline E2E
// ═══════════════════════════════════════════════════════════════════════════

/// Tests loading custom detector TOML patterns via ConfigEngine, feeding test
/// data through the detection pipeline.
#[test]
fn e2e_config_to_detection_pipeline() {
    let dir = tempdir("config_detection");
    let detectors_dir = dir.join("detectors");
    std::fs::create_dir_all(&detectors_dir).unwrap();

    // Write a custom detector TOML.
    std::fs::write(
        detectors_dir.join("custom.toml"),
        r#"
[[patterns]]
name = "deploy_start"
regex = "Deploying to"
state = "Running"

[[patterns]]
name = "deploy_done"
regex = "Deploy complete"
state = "Idle"

[[patterns]]
name = "deploy_fail"
regex = "Deploy FAILED"
state = "Error"

[[patterns]]
name = "approval_needed"
regex = "Approve deployment\\?"
state = "WaitingForInput"
"#,
    )
    .unwrap();

    // Load via ConfigEngine.
    let engine = ConfigEngine::load(&dir);
    let patterns = engine.load_detector_patterns();

    // User patterns should be prepended before defaults.
    assert!(patterns.len() > default_claude_code_state_patterns().len());
    assert_eq!(patterns[0].name, "deploy_start");

    // Create detector with loaded patterns.
    let (mut detector, rx) = StateDetector::new(patterns);

    // Test custom patterns.
    detector.process_chunk(b"Deploying to production...");
    assert_eq!(detector.current_state(), SessionState::Running);
    assert_eq!(*rx.borrow(), SessionState::Running);

    detector.process_chunk(b"Approve deployment?");
    assert_eq!(detector.current_state(), SessionState::WaitingForInput);

    detector.process_chunk(b"Deploy FAILED with exit code 1");
    assert_eq!(detector.current_state(), SessionState::Error);

    detector.process_chunk(b"Deploy complete in 45s");
    assert_eq!(detector.current_state(), SessionState::Idle);

    // Default patterns should still work.
    detector.process_chunk("⏺ Read src/main.rs".as_bytes());
    assert_eq!(detector.current_state(), SessionState::Running);
}

/// Tests that the StreamSplitter and StateDetector work correctly with
/// patterns loaded from TOML.
#[test]
fn e2e_custom_toml_through_splitter_and_detector() {
    let toml = r#"
[[patterns]]
name = "ci_running"
regex = "CI pipeline started"
state = "Running"

[[patterns]]
name = "ci_passed"
regex = "CI pipeline passed"
state = "Idle"

[[patterns]]
name = "ci_failed"
regex = "CI pipeline failed"
state = "Error"

[[patterns]]
name = "ci_approval"
regex = "Approve merge\\?"
state = "WaitingForInput"
"#;

    let state_patterns = load_patterns_from_toml(toml).unwrap();
    let (mut detector, rx) = StateDetector::new(state_patterns);

    let splitter_patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(splitter_patterns);

    // Simulate a CI pipeline in output.
    let lines = [
        "CI pipeline started for commit abc123",
        "Running tests...",
        "CI pipeline failed with 3 errors",
        "CI pipeline started again",
        "CI pipeline passed",
        "Approve merge?",
    ];

    for line in &lines {
        splitter.classify_chunk(line.as_bytes());
        detector.process_chunk(line.as_bytes());

        // Drain channels.
        while channels.message_rx.try_recv().is_ok() {}
        while channels.state_rx.try_recv().is_ok() {}
        while channels.raw_rx.try_recv().is_ok() {}
    }

    // Final state should be WaitingForInput (from "Approve merge?").
    assert_eq!(detector.current_state(), SessionState::WaitingForInput);
    assert_eq!(*rx.borrow(), SessionState::WaitingForInput);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. InputHandler → PTY → Terminal roundtrip E2E
// ═══════════════════════════════════════════════════════════════════════════

/// Tests the full input roundtrip: InputHandler encodes keys → bytes fed to
/// Terminal → verify cell content.
#[test]
fn e2e_input_handler_to_terminal_roundtrip() {
    let mut handler = InputHandler::new();
    assert_eq!(handler.mode(), InputMode::Insert);

    let mut terminal = Terminal::new(80, 24);

    // Type "echo hello\r" via InputHandler.
    let keys = ["e", "c", "h", "o", " ", "h", "e", "l", "l", "o"];
    for k in &keys {
        let action = handler.process_key(&char_key(k));
        if let InputAction::SendToPty(bytes) = action {
            terminal.feed(&bytes);
        }
    }

    // Verify terminal content.
    let content = terminal.content();
    let text: String = content.rows[0].iter().take(10).map(|c| c.c).collect();
    assert_eq!(text, "echo hello");

    // Press Enter.
    let action = handler.process_key(&named_key(NamedKey::Enter));
    assert_eq!(action, InputAction::SendToPty(b"\r".to_vec()));

    // Switch to Normal mode, verify keys are not forwarded.
    handler.process_key(&named_key(NamedKey::Escape));
    assert_eq!(handler.mode(), InputMode::Normal);
    let action = handler.process_key(&char_key("a"));
    assert_eq!(action, InputAction::None);

    // Switch back to Insert mode.
    handler.process_key(&char_key("i"));
    assert_eq!(handler.mode(), InputMode::Insert);

    // Type more text.
    let action = handler.process_key(&char_key("x"));
    assert_eq!(action, InputAction::SendToPty(b"x".to_vec()));
}

/// Tests Ctrl key combinations through the full pipeline.
#[test]
fn e2e_ctrl_keys_roundtrip() {
    let mut handler = InputHandler::new();
    let mut terminal = Terminal::new(80, 24);

    // Type some text.
    for c in &["h", "i"] {
        if let InputAction::SendToPty(bytes) = handler.process_key(&char_key(c)) {
            terminal.feed(&bytes);
        }
    }

    let content = terminal.content();
    assert_eq!(content.rows[0][0].c, 'h');
    assert_eq!(content.rows[0][1].c, 'i');

    // Ctrl+C should produce ETX (0x03).
    handler.set_modifiers(winit::keyboard::ModifiersState::CONTROL);
    let action = handler.process_key(&char_key("c"));
    assert_eq!(action, InputAction::SendToPty(vec![0x03]));

    // Ctrl+L should produce FF (0x0c) — clear screen in many terminals.
    let action = handler.process_key(&char_key("l"));
    assert_eq!(action, InputAction::SendToPty(vec![0x0c]));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Panel rendering pipeline E2E
// ═══════════════════════════════════════════════════════════════════════════

/// Tests the full panel rendering pipeline: StreamSplitter classifies output →
/// MessagePanel and StatePanel accumulate data → render to terminal cells →
/// verify content and colors.
#[test]
fn e2e_panel_rendering_pipeline() {
    let fixture = claude_code_session_fixture();

    let splitter_patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(splitter_patterns);

    let state_patterns = default_claude_code_state_patterns();
    let (mut detector, _rx) = StateDetector::new(state_patterns);

    let mut msg_panel = MessagePanel::new();
    let mut state_panel = StatePanel::new();

    for line in &fixture {
        splitter.classify_chunk(line.as_bytes());
        detector.process_chunk(line.as_bytes());

        while let Ok(chunk) = channels.message_rx.try_recv() {
            let text = String::from_utf8_lossy(&chunk.data).to_string();
            msg_panel.push_message(text, false);
        }
        while let Ok(chunk) = channels.state_rx.try_recv() {
            let text = String::from_utf8_lossy(&chunk.data).to_string();
            state_panel.push_state_line(text);
        }
        while channels.raw_rx.try_recv().is_ok() {}

        state_panel.update_state(detector.current_state());
    }

    // Verify message panel has content.
    assert!(!msg_panel.messages.is_empty(), "message panel should have messages");
    let msg_cells = msg_panel.to_terminal_cells(60, 20);
    assert_eq!(msg_cells.len(), 20);
    assert_eq!(msg_cells[0].len(), 60);

    // First visible message should contain AI text.
    let first_text: String = msg_cells[0].iter().map(|c| c.c).collect::<String>();
    assert!(
        !first_text.trim().is_empty(),
        "message panel should render text"
    );

    // Verify state panel has extracted info.
    assert!(state_panel.current_tool.is_some(), "state panel should have a tool");
    assert!(state_panel.cost.is_some(), "state panel should have cost info");

    let state_cells = state_panel.to_terminal_cells(40, 15);
    assert_eq!(state_cells.len(), 15);

    // Row 0 is header.
    let header: String = state_cells[0].iter().map(|c| c.c).collect::<String>();
    assert!(header.contains("State"), "state panel header missing");

    // Row 2 should contain tool info.
    let tool_row: String = state_cells[2].iter().map(|c| c.c).collect::<String>();
    assert!(tool_row.contains("Tool:"), "tool row missing");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. File watcher → Preview pipeline E2E
// ═══════════════════════════════════════════════════════════════════════════

/// Tests the file preview pipeline: extract paths from tool output →
/// syntax-highlight file → modify file → compute diff → verify diff output.
#[test]
fn e2e_file_watcher_preview_pipeline() {
    // Create a temporary Rust file.
    let dir = tempdir("preview_pipeline");
    let file_path = dir.join("example.rs");
    let original_content = "fn main() {\n    println!(\"hello\");\n}\n";
    std::fs::write(&file_path, original_content).unwrap();

    // Extract path from tool output.
    let tool_output = format!("Read {}", file_path.display());
    let extracted = ToolOutputMonitor::extract_paths(&tool_output);
    assert_eq!(extracted.len(), 1);
    assert_eq!(extracted[0], file_path);

    // Syntax-highlight the file.
    let highlighter = SyntaxHighlighter::new();
    let highlighted = highlighter.highlight_file(&file_path).unwrap();
    assert_eq!(highlighted.len(), 3, "file should have 3 lines");
    assert_eq!(highlighted[0].line_number, 1);
    assert!(!highlighted[0].spans.is_empty(), "first line should have spans");

    // Render highlighted lines to terminal cells.
    let cells = surfterm::preview::syntax::to_terminal_cells(&highlighted, 60, 10, 0);
    assert_eq!(cells.len(), 10);
    // Line number "   1 " should be at the start.
    assert_eq!(cells[0][3].c, '1');

    // Modify the file.
    let modified_content = "fn main() {\n    println!(\"hello, world!\");\n    eprintln!(\"debug\");\n}\n";
    std::fs::write(&file_path, modified_content).unwrap();

    // Compute diff.
    let diff = compute_diff(original_content, modified_content);
    assert!(!diff.hunks.is_empty(), "diff should have hunks");

    // Verify diff has the expected changes.
    let has_added = diff
        .hunks
        .iter()
        .any(|h| h.lines.iter().any(|l| matches!(l, DiffLine::Added(_))));
    let has_removed = diff
        .hunks
        .iter()
        .any(|h| h.lines.iter().any(|l| matches!(l, DiffLine::Removed(_))));
    assert!(has_added, "diff should show added lines");
    assert!(has_removed, "diff should show removed lines");

    // Render diff to terminal cells.
    let diff_cells = surfterm::preview::diff::to_terminal_cells(&diff, 60, 15);
    assert_eq!(diff_cells.len(), 15);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. BLE data pipeline E2E
// ═══════════════════════════════════════════════════════════════════════════

/// Tests the full BLE data pipeline: create session data → serialize via
/// GattService → chunk with ChunkProtocol → serialize/deserialize chunks →
/// reassemble → deserialize and verify integrity.
#[test]
fn e2e_ble_data_pipeline() {
    // Create session status data.
    let sessions = vec![
        SessionStatusData {
            id: "session-001".to_string(),
            project_name: "api-server".to_string(),
            state: "Running".to_string(),
            layer: "Background".to_string(),
        },
        SessionStatusData {
            id: "session-002".to_string(),
            project_name: "web-frontend".to_string(),
            state: "WaitingForInput".to_string(),
            layer: "Foreground".to_string(),
        },
        SessionStatusData {
            id: "session-003".to_string(),
            project_name: "cli-tool".to_string(),
            state: "Idle".to_string(),
            layer: "Pinned".to_string(),
        },
    ];

    // Serialize via GattService.
    let payload = GattService::session_list_payload(&sessions);
    assert!(!payload.is_empty());

    // Chunk with a small MTU (simulating BLE constraint).
    let protocol = ChunkProtocol::new(64); // MTU=64 bytes
    let chunks = protocol.chunk_data(&payload);
    assert!(
        chunks.len() > 1,
        "payload should need multiple chunks with small MTU"
    );

    // Verify chunk metadata.
    let expected_total = chunks.len() as u16;
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.sequence, i as u16);
        assert_eq!(chunk.total, expected_total);
    }

    // Serialize each chunk to wire format and back.
    let wire_chunks: Vec<Chunk> = chunks
        .iter()
        .map(|c| {
            let bytes = c.to_bytes();
            Chunk::from_bytes(&bytes).unwrap()
        })
        .collect();

    // Reassemble.
    let reassembled = ChunkProtocol::reassemble(&wire_chunks).unwrap();
    assert_eq!(reassembled, payload, "reassembled data should match original");

    // Deserialize back to session data.
    let restored: Vec<SessionStatusData> =
        serde_json::from_slice(&reassembled).expect("should deserialize to session data");
    assert_eq!(restored.len(), 3);
    assert_eq!(restored[0].id, "session-001");
    assert_eq!(restored[0].project_name, "api-server");
    assert_eq!(restored[1].state, "WaitingForInput");
    assert_eq!(restored[2].layer, "Pinned");
}

/// Tests BLE command parsing through the full pipeline.
#[test]
fn e2e_ble_command_pipeline() {
    use surfterm::ble::gatt::{validate_command, BleCommand};

    // Simulate a mobile client sending a "respond" command.
    let json = r#"{"type":"respond","session_id":"session-001","payload":"yes, proceed"}"#;
    let cmd = GattService::parse_command(json.as_bytes()).unwrap();
    validate_command(&cmd).unwrap();

    match &cmd {
        BleCommand::Respond {
            session_id,
            payload,
        } => {
            assert_eq!(session_id, "session-001");
            assert_eq!(payload, "yes, proceed");
        }
        _ => panic!("expected Respond command"),
    }

    // Simulate chunking the command over BLE.
    let protocol = ChunkProtocol::new(32);
    let chunks = protocol.chunk_data(json.as_bytes());
    let reassembled = ChunkProtocol::reassemble(&chunks).unwrap();
    let restored_cmd = GattService::parse_command(&reassembled).unwrap();
    assert_eq!(cmd, restored_cmd);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. LLM pipeline E2E (with mock)
// ═══════════════════════════════════════════════════════════════════════════

/// Tests the LLM pipeline: MockLlmBackend → LlmRuntime → LlmScheduler with
/// priority ordering → LlmClassifier, PromptExpander, SessionSummarizer,
/// CodeReviewer.
#[test]
fn e2e_llm_pipeline_with_mock() {
    // Create a mock runtime.
    let backend = MockLlmBackend::instant();
    let runtime = Arc::new(LlmRuntime::new_with_backend(Some(Box::new(backend))));
    assert!(runtime.is_available());

    // Create scheduler.
    let mut scheduler = LlmScheduler::new(Arc::clone(&runtime));

    // Submit tasks at different priorities.
    let (tx_review, mut rx_review) = tokio::sync::oneshot::channel();
    let (tx_summary, mut rx_summary) = tokio::sync::oneshot::channel();
    let (tx_expand, mut rx_expand) = tokio::sync::oneshot::channel();
    let (tx_classify, mut rx_classify) = tokio::sync::oneshot::channel();

    scheduler.submit(LlmTask {
        priority: LlmTaskPriority::CodeReview,
        prompt: "review code".into(),
        max_tokens: 200,
        timeout_ms: 2000,
        response_tx: tx_review,
    });
    scheduler.submit(LlmTask {
        priority: LlmTaskPriority::SessionSummary,
        prompt: "summarize session".into(),
        max_tokens: 100,
        timeout_ms: 1000,
        response_tx: tx_summary,
    });
    scheduler.submit(LlmTask {
        priority: LlmTaskPriority::PromptExpand,
        prompt: "expand prompt".into(),
        max_tokens: 128,
        timeout_ms: 500,
        response_tx: tx_expand,
    });
    scheduler.submit(LlmTask {
        priority: LlmTaskPriority::StreamClassify,
        prompt: "Classify: Hello help me".into(),
        max_tokens: 16,
        timeout_ms: 30,
        response_tx: tx_classify,
    });

    assert_eq!(scheduler.queue_len(), 4);

    // Process all tasks — they should come out in priority order.
    // StreamClassify first (highest priority).
    scheduler.process_next().unwrap();
    assert_eq!(scheduler.queue_len(), 3);
    let classify_result = rx_classify.try_recv().unwrap();
    assert!(classify_result.is_ok());

    // PromptExpand second.
    scheduler.process_next().unwrap();
    let expand_result = rx_expand.try_recv().unwrap();
    assert!(expand_result.is_ok());

    // SessionSummary third.
    scheduler.process_next().unwrap();
    let summary_result = rx_summary.try_recv().unwrap();
    assert!(summary_result.is_ok());

    // CodeReview last (lowest priority).
    scheduler.process_next().unwrap();
    let review_result = rx_review.try_recv().unwrap();
    assert!(review_result.is_ok());

    assert_eq!(scheduler.queue_len(), 0);
}

/// Tests individual LLM components (classifier, expander, summarizer, reviewer).
#[test]
fn e2e_llm_individual_components() {
    let backend = MockLlmBackend::instant();
    let runtime = Arc::new(LlmRuntime::new_with_backend(Some(Box::new(backend))));

    // LlmClassifier
    let classifier = LlmClassifier::new(Arc::clone(&runtime));
    let result = classifier.classify("Hello, help me");
    assert_eq!(result, Some(Classification::Message));

    let result = classifier.classify("Cost: $0.05");
    assert_eq!(result, Some(Classification::State));

    let result = classifier.classify_with_fallback("Hello world", Classification::Raw);
    assert_eq!(result, Classification::Message);

    let result = classifier.classify_with_fallback("something", Classification::State);
    assert_eq!(result, Classification::State); // Trusts regex when non-Raw.

    // PromptExpander
    let expander = PromptExpander::new(Arc::clone(&runtime));
    assert!(expander.is_available());
    let expanded = expander.expand("fix bug");
    assert!(expanded.is_some());
    assert!(!expanded.unwrap().is_empty());

    // SessionSummarizer
    let summarizer = SessionSummarizer::new(Arc::clone(&runtime));
    assert!(summarizer.is_available());
    let history = vec![
        "User: fix the bug in auth".to_string(),
        "AI: I found the issue in line 42".to_string(),
    ];
    let summary = summarizer.summarize(&history);
    assert!(summary.is_some());

    let fallback = summarizer.summarize_or_truncate(&history, 30);
    assert!(!fallback.is_empty());
    assert!(fallback.len() <= 30);

    // CodeReviewer
    let reviewer = CodeReviewer::new(Arc::clone(&runtime));
    assert!(reviewer.is_available());
    let review = reviewer.review("fn main() { panic!() }", "rust");
    assert!(review.is_some());

    let diff_review = reviewer.review_diff("+ new line\n- old line");
    assert!(diff_review.is_some());
}

/// Tests that disabled LLM runtime causes all components to fall back gracefully.
#[test]
fn e2e_llm_disabled_fallback() {
    let runtime = Arc::new(LlmRuntime::new_disabled());
    assert!(!runtime.is_available());

    let classifier = LlmClassifier::new(Arc::clone(&runtime));
    assert_eq!(classifier.classify("Hello"), None);
    assert_eq!(
        classifier.classify_with_fallback("Hello", Classification::Raw),
        Classification::Raw
    );

    let expander = PromptExpander::new(Arc::clone(&runtime));
    assert!(!expander.is_available());
    assert!(expander.expand("fix bug").is_none());

    let summarizer = SessionSummarizer::new(Arc::clone(&runtime));
    assert!(!summarizer.is_available());
    assert!(summarizer.summarize(&["msg".to_string()]).is_none());
    assert_eq!(
        summarizer.summarize_or_truncate(&["hello world".to_string()], 20),
        "hello world"
    );

    let reviewer = CodeReviewer::new(Arc::clone(&runtime));
    assert!(!reviewer.is_available());
    assert!(reviewer.review("code", "rust").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Stress tests
// ═══════════════════════════════════════════════════════════════════════════

/// Stress test: rapidly create and process many sessions through the full
/// pipeline, verifying no panics or data corruption.
#[test]
fn e2e_stress_many_sessions() {
    let start = Instant::now();
    let num_sessions = 10;

    let mut terminals = Vec::new();
    let mut splitters = Vec::new();
    let mut channels_vec = Vec::new();
    let mut detectors = Vec::new();

    // Create 10 sessions.
    for _ in 0..num_sessions {
        terminals.push(Terminal::new(80, 24));
        let (splitter, channels) =
            StreamSplitter::new(StreamSplitter::default_claude_code_patterns());
        splitters.push(splitter);
        channels_vec.push(channels);
        detectors.push(StateDetector::new(default_claude_code_state_patterns()));
    }

    // Feed large data through all sessions.
    let mut data = Vec::new();
    for i in 0..500 {
        match i % 4 {
            0 => data.extend_from_slice(b"Hello, I'll help you with that\n"),
            1 => data.extend_from_slice("⏺ Read src/main.rs\n".as_bytes()),
            2 => data.extend_from_slice(b"Error: something failed\n"),
            _ => data.extend_from_slice(b"some raw output line\n"),
        }
    }

    for session_idx in 0..num_sessions {
        terminals[session_idx].feed(&data);
        splitters[session_idx].classify_chunk(&data);

        // Drain channels.
        while channels_vec[session_idx].message_rx.try_recv().is_ok() {}
        while let Ok(chunk) = channels_vec[session_idx].state_rx.try_recv() {
            detectors[session_idx].0.process_chunk(&chunk.data);
        }
        while channels_vec[session_idx].raw_rx.try_recv().is_ok() {}

        // Feed full data to detector too.
        detectors[session_idx].0.process_chunk(&data);
    }

    let elapsed = start.elapsed();

    // Verify all sessions have valid state.
    for (i, (detector, _rx)) in detectors.iter().enumerate() {
        let state = detector.current_state();
        assert!(
            matches!(
                state,
                SessionState::Running
                    | SessionState::WaitingForInput
                    | SessionState::Error
                    | SessionState::Idle
            ),
            "session {i} has unexpected state: {state:?}"
        );
    }

    // Verify all terminals have content.
    for (i, terminal) in terminals.iter().enumerate() {
        let content = terminal.content();
        assert_eq!(content.rows.len(), 24, "session {i} terminal rows mismatch");
        assert_eq!(
            content.rows[0].len(),
            80,
            "session {i} terminal cols mismatch"
        );
    }

    // Performance: 10 sessions * 500 lines should complete quickly.
    assert!(
        elapsed < Duration::from_secs(5),
        "stress test took too long: {:?}",
        elapsed
    );
}

/// Stress test: feed progressively larger data to StreamSplitter.
#[test]
fn e2e_stress_large_stream_splitter_data() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    // Build ~1MB of mixed content.
    let mut data = Vec::new();
    for i in 0..10_000 {
        match i % 5 {
            0 => data.extend_from_slice(b"Hello, I'll help you with that task\n"),
            1 => data.extend_from_slice("⏺ Read src/module.rs\n".as_bytes()),
            2 => data.extend_from_slice(b"Cost: $0.01\n"),
            3 => data.extend_from_slice(b"\x1b[32msome green text\x1b[0m\n"),
            _ => data.extend_from_slice(b"Running tests...\n"),
        }
    }

    let start = Instant::now();
    splitter.classify_chunk(&data);
    let elapsed = start.elapsed();

    // Drain all channels. Note: broadcast channels with capacity 256 will
    // lag when we send 10,000 messages. We count both Ok and Lagged results.
    let mut msg_count = 0u64;
    let mut state_count = 0u64;
    let mut raw_count = 0u64;

    loop {
        match channels.message_rx.try_recv() {
            Ok(_) => msg_count += 1,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                msg_count += n;
            }
            Err(_) => break,
        }
    }
    loop {
        match channels.state_rx.try_recv() {
            Ok(_) => state_count += 1,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                state_count += n;
            }
            Err(_) => break,
        }
    }
    loop {
        match channels.raw_rx.try_recv() {
            Ok(_) => raw_count += 1,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                raw_count += n;
            }
            Err(_) => break,
        }
    }

    let total = msg_count + state_count + raw_count;
    assert!(total > 0, "should have classified some chunks");
    assert_eq!(total, 10_000, "should have one chunk per line");

    assert!(
        elapsed < Duration::from_secs(2),
        "classifying ~1MB should be under 2s, took {:?}",
        elapsed
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Cross-module integration scenarios
// ═══════════════════════════════════════════════════════════════════════════

/// Tests the complete pipeline from simulated PTY output through terminal,
/// classification, detection, layer transitions, and panel rendering.
#[test]
fn e2e_complete_pipeline_integration() {
    let fixture = claude_code_session_fixture();

    // 1. Terminal + StreamSplitter + StateDetector.
    let mut terminal = Terminal::new(120, 40);
    let splitter_patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(splitter_patterns);
    let state_patterns = default_claude_code_state_patterns();
    let (mut detector, _rx) = StateDetector::new(state_patterns);

    // 2. LayerController.
    let mut layer_ctrl = LayerController::new();
    let session_id = SessionId::new();
    layer_ctrl.assign(session_id, Layer::Foreground);

    // 3. Panels.
    let mut msg_panel = MessagePanel::new();
    let mut state_panel = StatePanel::new();

    let mut prev_state = SessionState::Idle;

    for line in &fixture {
        // Feed to terminal.
        terminal.feed(format!("{line}\r\n").as_bytes());

        // Classify.
        splitter.classify_chunk(line.as_bytes());

        // Drain and route.
        while let Ok(chunk) = channels.message_rx.try_recv() {
            let text = String::from_utf8_lossy(&chunk.data).to_string();
            msg_panel.push_message(text, false);
        }
        while let Ok(chunk) = channels.state_rx.try_recv() {
            let text = String::from_utf8_lossy(&chunk.data).to_string();
            state_panel.push_state_line(text);
            detector.process_chunk(&chunk.data);
        }
        while channels.raw_rx.try_recv().is_ok() {}

        // Also process through detector directly for comprehensive detection.
        detector.process_chunk(line.as_bytes());
        let new_state = detector.current_state();

        if new_state != prev_state {
            state_panel.update_state(new_state);
            apply_state_change(&mut layer_ctrl, &session_id, prev_state, new_state);
            prev_state = new_state;
        }
    }

    // Verify the terminal rendered all lines.
    let content = terminal.content();
    assert_eq!(content.rows.len(), 40);

    // Verify the panels have data.
    assert!(!msg_panel.messages.is_empty());
    assert!(!state_panel.state_lines.is_empty());

    // Verify the layer controller is in a valid state.
    let layer = layer_ctrl.get_layer(&session_id);
    assert!(layer.is_some());

    // Render both panels and verify dimensions.
    let msg_cells = msg_panel.to_terminal_cells(80, 30);
    assert_eq!(msg_cells.len(), 30);
    assert_eq!(msg_cells[0].len(), 80);

    let state_cells = state_panel.to_terminal_cells(40, 30);
    assert_eq!(state_cells.len(), 30);
    assert_eq!(state_cells[0].len(), 40);
}

/// PTY spawning E2E with short-lived command, reading output, and feeding
/// to the terminal and detection pipeline.
#[tokio::test]
async fn e2e_pty_spawn_and_pipeline() {
    use surfterm::session::pty::PtyHandle;

    // Force a known shell.
    std::env::set_var("SHELL", "/bin/sh");

    let mut pty = PtyHandle::spawn(24, 80, "e2e-test", "/tmp/surfterm-e2e.sock").expect("spawn pty");

    // Send a quick command and exit.
    pty.write_input(b"echo E2E_TEST_MARKER\n")
        .await
        .expect("write echo");
    pty.write_input(b"exit\n").await.expect("write exit");

    // Collect output with a timeout.
    let mut all_output = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while let Ok(Some(chunk)) = tokio::time::timeout_at(deadline, pty.read_output()).await {
        all_output.extend_from_slice(&chunk);
    }

    let output_text = String::from_utf8_lossy(&all_output);
    assert!(
        output_text.contains("E2E_TEST_MARKER"),
        "PTY output should contain our marker, got: {}",
        &output_text[..output_text.len().min(500)]
    );

    // Feed the output through the terminal emulator.
    let mut terminal = Terminal::new(80, 24);
    terminal.feed(&all_output);

    // Feed through StreamSplitter.
    let splitter_patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(splitter_patterns);
    splitter.classify_chunk(&all_output);

    // At least some output should be classified.
    let mut classified = 0;
    while channels.message_rx.try_recv().is_ok() {
        classified += 1;
    }
    while channels.state_rx.try_recv().is_ok() {
        classified += 1;
    }
    while channels.raw_rx.try_recv().is_ok() {
        classified += 1;
    }
    assert!(classified > 0, "PTY output should produce classified chunks");
}

/// Tests the full SessionManager lifecycle E2E (create, operate, kill).
#[tokio::test]
async fn e2e_session_manager_lifecycle() {
    use surfterm::session::SessionManager;

    let mut mgr = SessionManager::new();
    assert_eq!(mgr.session_count(), 0);

    // Create 3 sessions.
    let id1 = mgr.create_session(None, None, 80, 24).unwrap();
    let id2 = mgr.create_session(None, None, 80, 24).unwrap();
    let id3 = mgr.create_session(None, None, 80, 24).unwrap();
    assert_eq!(mgr.session_count(), 3);

    // First session should be active.
    assert_eq!(mgr.active_session().unwrap().id(), id1);

    // Switch to session 2.
    mgr.switch_to(&id2).unwrap();
    assert_eq!(mgr.active_session().unwrap().id(), id2);

    // All sessions should start Idle.
    for id in [id1, id2, id3] {
        assert_eq!(
            mgr.get_session(&id).unwrap().state(),
            SessionState::Idle
        );
    }

    // Kill session 2 (active) — should switch to another.
    mgr.kill_session(&id2).unwrap();
    assert_eq!(mgr.session_count(), 2);
    assert!(mgr.active_session().is_some());

    // Kill remaining sessions.
    mgr.kill_session(&id1).unwrap();
    mgr.kill_session(&id3).unwrap();
    assert_eq!(mgr.session_count(), 0);
    assert!(mgr.active_session().is_none());
}
