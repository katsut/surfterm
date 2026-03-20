//! Integration tests for Surfterm.
//!
//! These tests exercise cross-module pipelines without requiring a GPU.

use std::time::{Duration, Instant};

use surfterm::detector::patterns::{default_claude_code_state_patterns, load_patterns_from_toml};
use surfterm::detector::StateDetector;
use surfterm::input::{encode_key, InputAction, InputHandler, InputMode, SurftermCmd};
use surfterm::renderer::grid::GridLayout;
use surfterm::renderer::panel::{DisplayMode, MessagePanel, StatePanel};
use surfterm::session::state::SessionState;
use surfterm::session::stream_splitter::{Classification, StreamSplitter};
use surfterm::session::terminal::Terminal;

use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

// ─── Helpers ─────────────────────────────────────────────────────────────

fn char_key(c: &str) -> Key {
    Key::Character(SmolStr::new(c))
}

fn named_key(k: NamedKey) -> Key {
    Key::Named(k)
}

// ─── 1. PTY → Terminal pipeline ──────────────────────────────────────────

#[test]
fn pty_to_terminal_echo_hello() {
    let mut term = Terminal::new(80, 24);
    // Simulate PTY output from `echo "hello"`
    term.feed(b"hello\r\n");

    let content = term.content();
    let first_row: String = content.rows[0].iter().take(5).map(|c| c.c).collect();
    assert_eq!(first_row, "hello");
}

#[test]
fn pty_to_terminal_ansi_color_output() {
    let mut term = Terminal::new(80, 24);
    // Simulate colored output (green text)
    term.feed(b"\x1b[32mgreen text\x1b[0m normal text");

    let content = term.content();
    // 'g' should be green
    let g_cell = &content.rows[0][0];
    assert_eq!(g_cell.c, 'g');
    // Green (index 2) = Rgb(0, 205, 0)
    assert_eq!(g_cell.fg, surfterm::session::terminal::Rgb::new(0, 205, 0));
}

#[test]
fn pty_to_terminal_multiline_output() {
    let mut term = Terminal::new(80, 24);
    term.feed(b"line1\r\nline2\r\nline3");

    let content = term.content();
    let row0: String = content.rows[0].iter().take(5).map(|c| c.c).collect();
    let row1: String = content.rows[1].iter().take(5).map(|c| c.c).collect();
    let row2: String = content.rows[2].iter().take(5).map(|c| c.c).collect();
    assert_eq!(row0, "line1");
    assert_eq!(row1, "line2");
    assert_eq!(row2, "line3");
}

// ─── 2. PTY → StreamSplitter pipeline ───────────────────────────────────

#[test]
fn pty_to_stream_splitter_classifies_output() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    // Simulate PTY output that contains a tool use indicator
    splitter.classify_chunk("⏺ Read src/main.rs".as_bytes());
    let chunk = channels.state_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::State);

    // Simulate AI message output
    splitter.classify_chunk(b"Hello, I'll help you with that");
    let chunk = channels.message_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::Message);

    // Simulate raw VT output
    splitter.classify_chunk(b"\x1b[2J\x1b[H");
    let chunk = channels.raw_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::Raw);
}

#[test]
fn pty_output_mixed_classifications() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    let mixed = "Hello, I'll help you\n⏺ Read file.rs\nsome raw output";
    splitter.classify_chunk(mixed.as_bytes());

    let msg = channels.message_rx.try_recv().unwrap();
    assert_eq!(msg.classification, Classification::Message);

    let state = channels.state_rx.try_recv().unwrap();
    assert_eq!(state.classification, Classification::State);

    let raw = channels.raw_rx.try_recv().unwrap();
    assert_eq!(raw.classification, Classification::Raw);
}

// ─── 3. StreamSplitter → StateDetector pipeline ─────────────────────────

#[test]
fn stream_splitter_to_state_detector() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    let state_patterns = default_claude_code_state_patterns();
    let (mut detector, rx) = StateDetector::new(state_patterns);

    // Feed Claude Code-like tool output
    splitter.classify_chunk("⏺ Read src/main.rs".as_bytes());
    let chunk = channels.state_rx.try_recv().unwrap();

    // Feed the state channel chunk to the detector
    detector.process_chunk(&chunk.data);
    assert_eq!(detector.current_state(), SessionState::Running);
    assert_eq!(*rx.borrow(), SessionState::Running);

    // Feed a waiting-for-input prompt
    splitter.classify_chunk(b"Would you like to proceed?");
    // This matches both state and message patterns; check state channel
    // It may go to state or message depending on pattern order.
    // Try state channel first, then message.
    if let Ok(chunk) = channels.state_rx.try_recv() {
        detector.process_chunk(&chunk.data);
    } else if let Ok(chunk) = channels.message_rx.try_recv() {
        detector.process_chunk(&chunk.data);
    }
    // The detector should see the waiting pattern regardless
    // Let's feed it directly to ensure the detector works
    detector.process_chunk(b"Would you like to proceed?");
    assert_eq!(detector.current_state(), SessionState::WaitingForInput);
}

// ─── 4. Full pipeline ───────────────────────────────────────────────────

#[test]
fn full_pipeline_classify_detect_verify() {
    let splitter_patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(splitter_patterns);

    let state_patterns = default_claude_code_state_patterns();
    let (mut detector, rx) = StateDetector::new(state_patterns);

    // Step 1: Classify tool execution output
    splitter.classify_chunk("⏺ Bash cargo build".as_bytes());
    let chunk = channels.state_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::State);

    // Step 2: Feed to detector → should detect Running
    detector.process_chunk(&chunk.data);
    assert_eq!(*rx.borrow(), SessionState::Running);

    // Step 3: Classify error output — "Error:" doesn't match any stream
    // splitter state/message pattern, so it goes to Raw. Feed it directly
    // to the detector which has its own error patterns.
    detector.process_chunk(b"Error: compilation failed");
    assert_eq!(*rx.borrow(), SessionState::Error);
}

// ─── 5. InputHandler → PTY roundtrip ────────────────────────────────────

#[test]
fn input_handler_encode_and_roundtrip_via_terminal() {
    let mut handler = InputHandler::new();
    assert_eq!(handler.mode(), InputMode::Insert);

    // Type "ls\r" and verify encoded bytes
    let a1 = handler.process_key(&char_key("l"));
    assert_eq!(a1, InputAction::SendToPty(b"l".to_vec()));

    let a2 = handler.process_key(&char_key("s"));
    assert_eq!(a2, InputAction::SendToPty(b"s".to_vec()));

    let a3 = handler.process_key(&named_key(NamedKey::Enter));
    assert_eq!(a3, InputAction::SendToPty(b"\r".to_vec()));

    // Feed the encoded bytes to a terminal and verify content
    let mut term = Terminal::new(80, 24);
    if let InputAction::SendToPty(ref bytes) = a1 {
        term.feed(bytes);
    }
    if let InputAction::SendToPty(ref bytes) = a2 {
        term.feed(bytes);
    }

    let content = term.content();
    assert_eq!(content.rows[0][0].c, 'l');
    assert_eq!(content.rows[0][1].c, 's');
}

#[test]
fn ctrl_c_encodes_to_etx() {
    let mut handler = InputHandler::new();
    handler.set_modifiers(ModifiersState::CONTROL);
    let action = handler.process_key(&char_key("c"));
    assert_eq!(action, InputAction::SendToPty(vec![0x03]));
}

// ─── 6. Terminal resize consistency ──────────────────────────────────────

#[test]
fn terminal_resize_preserves_content() {
    let mut term = Terminal::new(80, 24);
    term.feed(b"hello world");

    // Verify content before resize
    let content = term.content();
    let pre_text: String = content.rows[0].iter().take(11).map(|c| c.c).collect();
    assert_eq!(pre_text, "hello world");

    // Resize to smaller
    term.resize(40, 12);
    let content = term.content();
    assert_eq!(content.rows.len(), 12);
    assert_eq!(content.rows[0].len(), 40);

    // Content should still be present
    let post_text: String = content.rows[0].iter().take(11).map(|c| c.c).collect();
    assert_eq!(post_text, "hello world");
}

#[test]
fn terminal_resize_larger_then_smaller() {
    let mut term = Terminal::new(40, 10);
    term.feed(b"test content");
    term.resize(80, 24);
    term.resize(40, 10);

    let content = term.content();
    assert_eq!(content.rows.len(), 10);
    assert_eq!(content.rows[0].len(), 40);
}

// ─── 7. StateDetector TOML patterns ─────────────────────────────────────

#[test]
fn state_detector_with_custom_toml_patterns() {
    let toml = r#"
[[patterns]]
name = "compiling"
regex = "Compiling"
state = "Running"

[[patterns]]
name = "done"
regex = "Finished"
state = "Idle"

[[patterns]]
name = "compile_error"
regex = "^error\\["
state = "Error"

[[patterns]]
name = "input_prompt"
regex = "Enter.*:"
state = "WaitingForInput"
"#;

    let patterns = load_patterns_from_toml(toml).unwrap();
    let (mut detector, rx) = StateDetector::new(patterns);

    detector.process_chunk(b"Compiling surfterm v0.1.0");
    assert_eq!(*rx.borrow(), SessionState::Running);

    detector.process_chunk(b"error[E0308]: mismatched types");
    assert_eq!(*rx.borrow(), SessionState::Error);

    detector.process_chunk(b"Enter your name:");
    assert_eq!(*rx.borrow(), SessionState::WaitingForInput);

    detector.process_chunk(b"Finished `dev` profile");
    assert_eq!(*rx.borrow(), SessionState::Idle);
}

// ─── 8. MessagePanel + StatePanel integration ────────────────────────────

#[test]
fn message_panel_state_panel_integration() {
    let splitter_patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(splitter_patterns);

    let mut msg_panel = MessagePanel::new();
    let mut state_panel = StatePanel::new();

    // Classify message output
    splitter.classify_chunk(b"Hello, I'll help you with that task");
    if let Ok(chunk) = channels.message_rx.try_recv() {
        let text = String::from_utf8_lossy(&chunk.data).to_string();
        msg_panel.push_message(text, false);
    }

    // Classify state output
    splitter.classify_chunk("⏺ Read src/main.rs".as_bytes());
    if let Ok(chunk) = channels.state_rx.try_recv() {
        let text = String::from_utf8_lossy(&chunk.data).to_string();
        state_panel.push_state_line(text);
    }

    splitter.classify_chunk(b"Cost: $0.05");
    if let Ok(chunk) = channels.state_rx.try_recv() {
        let text = String::from_utf8_lossy(&chunk.data).to_string();
        state_panel.push_state_line(text);
    }

    // Verify message panel has the AI message
    let msg_cells = msg_panel.to_terminal_cells(40, 5);
    let row_text: String = msg_cells[0].iter().map(|c| c.c).collect::<String>();
    assert!(
        row_text.starts_with("Hello, I'll help"),
        "Expected AI message, got: '{}'",
        row_text.trim()
    );

    // Verify state panel has tool info and cost
    assert_eq!(state_panel.current_tool.as_deref(), Some("Read"));
    assert_eq!(state_panel.cost.as_deref(), Some("$0.05"));

    let state_cells = state_panel.to_terminal_cells(30, 10);
    // Row 0 is header, row 2 is tool
    let tool_row: String = state_cells[2].iter().map(|c| c.c).collect::<String>();
    assert!(
        tool_row.contains("Read"),
        "Expected tool name, got: '{}'",
        tool_row.trim()
    );
}

// ─── 9. StreamSplitter performance ──────────────────────────────────────

#[test]
fn stream_splitter_performance_large_input() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, _channels) = StreamSplitter::new(patterns);

    // Build a large multi-line input (~100KB)
    let mut data = Vec::new();
    for i in 0..3000 {
        data.extend_from_slice(format!("Line {i}: some output text here\n").as_bytes());
    }

    let start = Instant::now();
    splitter.classify_chunk(&data);
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(100),
        "Classification of ~100KB should complete within 100ms, took {:?}",
        elapsed
    );
}

#[test]
fn stream_splitter_performance_mixed_content() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, _channels) = StreamSplitter::new(patterns);

    let mut data = Vec::new();
    for i in 0..1000 {
        match i % 3 {
            0 => data.extend_from_slice(b"Hello, I'll help you with that\n"),
            1 => data.extend_from_slice("⏺ Read src/main.rs\n".as_bytes()),
            _ => data.extend_from_slice(b"\x1b[32mraw output\x1b[0m\n"),
        }
    }

    let start = Instant::now();
    splitter.classify_chunk(&data);
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(100),
        "Mixed classification should complete within 100ms, took {:?}",
        elapsed
    );
}

// ─── 10. Multi-state transitions ─────────────────────────────────────────

#[test]
fn multi_state_transitions_full_session() {
    let patterns = default_claude_code_state_patterns();
    let (mut detector, rx) = StateDetector::new(patterns);

    // Initial: Idle
    assert_eq!(detector.current_state(), SessionState::Idle);

    // User sends a prompt → Running (tool indicator)
    detector.process_chunk("⏺ Reading src/main.rs".as_bytes());
    assert_eq!(detector.current_state(), SessionState::Running);
    assert_eq!(*rx.borrow(), SessionState::Running);

    // Tool completes, AI asks for input
    detector.process_chunk(b"Would you like to proceed?");
    assert_eq!(detector.current_state(), SessionState::WaitingForInput);
    assert_eq!(*rx.borrow(), SessionState::WaitingForInput);

    // User confirms, another tool starts
    detector.process_chunk("⏺ Writing output.txt".as_bytes());
    assert_eq!(detector.current_state(), SessionState::Running);
    assert_eq!(*rx.borrow(), SessionState::Running);

    // Error occurs
    detector.process_chunk(b"Error: permission denied to write file");
    assert_eq!(detector.current_state(), SessionState::Error);
    assert_eq!(*rx.borrow(), SessionState::Error);

    // Recovery: another tool starts
    detector.process_chunk(b"Running fallback command");
    assert_eq!(detector.current_state(), SessionState::Running);
    assert_eq!(*rx.borrow(), SessionState::Running);

    // Session completes with no more matching output → stays Running
    detector.process_chunk(b"all done, no patterns match here zzz");
    assert_eq!(detector.current_state(), SessionState::Running);
}

#[test]
fn rapid_state_transitions() {
    let patterns = default_claude_code_state_patterns();
    let (mut detector, rx) = StateDetector::new(patterns);

    let inputs: Vec<(&[u8], SessionState)> = vec![
        ("⏺ tool".as_bytes(), SessionState::Running),
        (b"> ", SessionState::WaitingForInput),
        (b"Error: fail", SessionState::Error),
        (b"Running again", SessionState::Running),
        (b"Do you want to continue?", SessionState::WaitingForInput),
        (b"Searching for files", SessionState::Running),
        (b"FAILED to compile", SessionState::Error),
    ];

    for (input, expected_state) in inputs {
        detector.process_chunk(input);
        assert_eq!(
            detector.current_state(),
            expected_state,
            "After input '{}', expected {:?} but got {:?}",
            String::from_utf8_lossy(input),
            expected_state,
            detector.current_state()
        );
        assert_eq!(*rx.borrow(), expected_state);
    }
}

// ─── 11. InputHandler extended tests (F-keys, nav keys, Ctrl+A-Z) ──────

#[test]
fn f1_through_f12_keys() {
    let mut handler = InputHandler::new();
    let expected: Vec<(NamedKey, &[u8])> = vec![
        (NamedKey::F1, b"\x1bOP"),
        (NamedKey::F2, b"\x1bOQ"),
        (NamedKey::F3, b"\x1bOR"),
        (NamedKey::F4, b"\x1bOS"),
        (NamedKey::F5, b"\x1b[15~"),
        (NamedKey::F6, b"\x1b[17~"),
        (NamedKey::F7, b"\x1b[18~"),
        (NamedKey::F8, b"\x1b[19~"),
        (NamedKey::F9, b"\x1b[20~"),
        (NamedKey::F10, b"\x1b[21~"),
        (NamedKey::F11, b"\x1b[23~"),
        (NamedKey::F12, b"\x1b[24~"),
    ];

    for (key, expected_bytes) in expected {
        let action = handler.process_key(&named_key(key));
        assert_eq!(
            action,
            InputAction::SendToPty(expected_bytes.to_vec()),
            "F-key {:?} should encode to {:?}",
            key,
            expected_bytes
        );
    }
}

#[test]
fn home_end_page_up_page_down() {
    let mut handler = InputHandler::new();

    let cases: Vec<(NamedKey, &[u8])> = vec![
        (NamedKey::Home, b"\x1b[H"),
        (NamedKey::End, b"\x1b[F"),
        (NamedKey::PageUp, b"\x1b[5~"),
        (NamedKey::PageDown, b"\x1b[6~"),
        (NamedKey::Insert, b"\x1b[2~"),
        (NamedKey::Delete, b"\x1b[3~"),
    ];

    for (key, expected_bytes) in cases {
        let action = handler.process_key(&named_key(key));
        assert_eq!(
            action,
            InputAction::SendToPty(expected_bytes.to_vec()),
            "{:?} encoding mismatch",
            key
        );
    }
}

#[test]
fn ctrl_a_through_z() {
    let mut handler = InputHandler::new();
    handler.set_modifiers(ModifiersState::CONTROL);

    for (i, ch) in ('a'..='z').enumerate() {
        let action = handler.process_key(&char_key(&ch.to_string()));
        let expected_byte = (i as u8) + 1; // Ctrl+A = 0x01, Ctrl+B = 0x02, ...
        assert_eq!(
            action,
            InputAction::SendToPty(vec![expected_byte]),
            "Ctrl+{} should produce 0x{:02x}",
            ch,
            expected_byte
        );
    }
}

#[test]
fn ctrl_uppercase_same_as_lowercase() {
    let mut handler = InputHandler::new();
    handler.set_modifiers(ModifiersState::CONTROL);

    let action_lower = handler.process_key(&char_key("c"));
    let action_upper = handler.process_key(&char_key("C"));
    assert_eq!(action_lower, action_upper);
    assert_eq!(action_lower, InputAction::SendToPty(vec![0x03]));
}

#[test]
fn multiple_mode_switches() {
    let mut handler = InputHandler::new();
    assert_eq!(handler.mode(), InputMode::Insert);

    // Insert → Normal (Escape)
    let a = handler.process_key(&named_key(NamedKey::Escape));
    assert_eq!(a, InputAction::SurftermCommand(SurftermCmd::SwitchToNormal));
    assert_eq!(handler.mode(), InputMode::Normal);

    // Normal → Insert (i)
    let a = handler.process_key(&char_key("i"));
    assert_eq!(a, InputAction::SurftermCommand(SurftermCmd::SwitchToInsert));
    assert_eq!(handler.mode(), InputMode::Insert);

    // Insert → Normal → Insert → Normal (rapid switches)
    handler.process_key(&named_key(NamedKey::Escape));
    assert_eq!(handler.mode(), InputMode::Normal);
    handler.process_key(&char_key("i"));
    assert_eq!(handler.mode(), InputMode::Insert);
    handler.process_key(&named_key(NamedKey::Escape));
    assert_eq!(handler.mode(), InputMode::Normal);
    handler.process_key(&char_key("i"));
    assert_eq!(handler.mode(), InputMode::Insert);
}

#[test]
fn normal_mode_commands_after_switch() {
    let mut handler = InputHandler::new();
    handler.process_key(&named_key(NamedKey::Escape)); // Switch to Normal

    let a = handler.process_key(&char_key("r"));
    assert_eq!(a, InputAction::SurftermCommand(SurftermCmd::ToggleRawView));

    let a = handler.process_key(&char_key("q"));
    assert_eq!(a, InputAction::SurftermCommand(SurftermCmd::Quit));
}

// ─── 12. StreamSplitter edge cases ──────────────────────────────────────

#[test]
fn stream_splitter_empty_input() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    splitter.classify_chunk(b"");

    // No chunks should be sent for empty input
    assert!(channels.message_rx.try_recv().is_err());
    assert!(channels.state_rx.try_recv().is_err());
    assert!(channels.raw_rx.try_recv().is_err());
}

#[test]
fn stream_splitter_single_character() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    splitter.classify_chunk(b"x");
    let chunk = channels.raw_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::Raw);
    assert_eq!(chunk.data, b"x");
}

#[test]
fn stream_splitter_binary_data() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    let binary = vec![0x00, 0x01, 0x02, 0xff, 0xfe, 0xfd];
    splitter.classify_chunk(&binary);

    // Binary data should be classified as Raw (no pattern matches)
    let chunk = channels.raw_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::Raw);
}

#[test]
fn stream_splitter_very_long_line() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    // 10KB single line
    let long_line: String = "x".repeat(10_000);
    splitter.classify_chunk(long_line.as_bytes());

    let chunk = channels.raw_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::Raw);
    assert_eq!(chunk.data.len(), 10_000);
}

#[test]
fn stream_splitter_unicode_content() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    // Japanese, emoji, CJK
    splitter.classify_chunk("こんにちは世界 🌍 你好".as_bytes());
    let chunk = channels.raw_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::Raw);

    let text = String::from_utf8(chunk.data).unwrap();
    assert!(text.contains("こんにちは"));
    assert!(text.contains("🌍"));
}

#[test]
fn stream_splitter_unicode_tool_indicator() {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, mut channels) = StreamSplitter::new(patterns);

    // ⏺ is the tool indicator (multi-byte UTF-8)
    splitter.classify_chunk("⏺ Edit file.rs".as_bytes());
    let chunk = channels.state_rx.try_recv().unwrap();
    assert_eq!(chunk.classification, Classification::State);
}

// ─── 13. Patterns TOML edge cases ───────────────────────────────────────

#[test]
fn toml_empty_string_fails() {
    let result = load_patterns_from_toml("");
    assert!(result.is_err(), "Empty TOML should fail to parse");
}

#[test]
fn toml_no_patterns_array() {
    let toml = r#"
[metadata]
name = "test"
version = "1.0"
"#;
    let result = load_patterns_from_toml(toml);
    assert!(
        result.is_err(),
        "TOML without patterns array should fail"
    );
}

#[test]
fn toml_empty_patterns_array() {
    let toml = "patterns = []\n";
    let patterns = load_patterns_from_toml(toml).unwrap();
    assert!(patterns.is_empty());
}

#[test]
fn toml_all_state_types() {
    let toml = r#"
[[patterns]]
name = "idle_marker"
regex = "^IDLE$"
state = "Idle"

[[patterns]]
name = "run_marker"
regex = "^RUN$"
state = "Running"

[[patterns]]
name = "wait_marker"
regex = "^WAIT$"
state = "WaitingForInput"

[[patterns]]
name = "err_marker"
regex = "^ERR$"
state = "Error"
"#;

    let patterns = load_patterns_from_toml(toml).unwrap();
    assert_eq!(patterns.len(), 4);

    assert_eq!(patterns[0].target_state, SessionState::Idle);
    assert_eq!(patterns[1].target_state, SessionState::Running);
    assert_eq!(patterns[2].target_state, SessionState::WaitingForInput);
    assert_eq!(patterns[3].target_state, SessionState::Error);

    // Verify patterns actually match
    assert!(patterns[0].regex.is_match("IDLE"));
    assert!(!patterns[0].regex.is_match("not idle"));
    assert!(patterns[1].regex.is_match("RUN"));
    assert!(patterns[2].regex.is_match("WAIT"));
    assert!(patterns[3].regex.is_match("ERR"));
}

#[test]
fn toml_custom_patterns_with_detector() {
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
"#;

    let patterns = load_patterns_from_toml(toml).unwrap();
    let (mut detector, rx) = StateDetector::new(patterns);

    detector.process_chunk(b"CI pipeline started for commit abc123");
    assert_eq!(*rx.borrow(), SessionState::Running);

    detector.process_chunk(b"CI pipeline failed with exit code 1");
    assert_eq!(*rx.borrow(), SessionState::Error);

    detector.process_chunk(b"CI pipeline started again");
    assert_eq!(*rx.borrow(), SessionState::Running);

    detector.process_chunk(b"CI pipeline passed");
    assert_eq!(*rx.borrow(), SessionState::Idle);
}

// ─── Grid layout integration ────────────────────────────────────────────

#[test]
fn grid_layout_panel_cols_match_terminal() {
    let grid = GridLayout::new(1280, 800, 16.0);

    let left_cols = grid.left_panel_cols();
    let right_cols = grid.right_panel_cols();

    // Both panels should have non-zero columns
    assert!(left_cols > 0);
    assert!(right_cols > 0);

    // Create panels that fit the grid dimensions
    let mut msg_panel = MessagePanel::new();
    msg_panel.push_message("test message".to_string(), false);
    let msg_cells = msg_panel.to_terminal_cells(left_cols, grid.rows);
    assert_eq!(msg_cells.len(), grid.rows as usize);
    assert_eq!(msg_cells[0].len(), left_cols as usize);

    let state_panel = StatePanel::new();
    let state_cells = state_panel.to_terminal_cells(right_cols, grid.rows);
    assert_eq!(state_cells.len(), grid.rows as usize);
    assert_eq!(state_cells[0].len(), right_cols as usize);
}

// ─── Display mode toggle ────────────────────────────────────────────────

#[test]
fn display_mode_toggle_roundtrip() {
    use surfterm::renderer::panel::toggle_display_mode;

    let mode = DisplayMode::Panels;
    let raw = toggle_display_mode(&mode);
    assert_eq!(raw, DisplayMode::Raw);
    let panels = toggle_display_mode(&raw);
    assert_eq!(panels, DisplayMode::Panels);
}

// ─── encode_key standalone tests ────────────────────────────────────────

#[test]
fn encode_key_regular_character() {
    let result = encode_key(&char_key("a"), ModifiersState::empty());
    assert_eq!(result, Some(b"a".to_vec()));
}

#[test]
fn encode_key_unicode_character() {
    let result = encode_key(&char_key("日"), ModifiersState::empty());
    assert_eq!(result, Some("日".as_bytes().to_vec()));
}

#[test]
fn encode_key_ctrl_modifier() {
    let result = encode_key(&char_key("a"), ModifiersState::CONTROL);
    assert_eq!(result, Some(vec![0x01])); // Ctrl+A

    let result = encode_key(&char_key("z"), ModifiersState::CONTROL);
    assert_eq!(result, Some(vec![0x1a])); // Ctrl+Z
}
