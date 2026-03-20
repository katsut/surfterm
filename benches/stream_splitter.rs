use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use regex::Regex;
use surfterm::detector::patterns::default_claude_code_state_patterns;
use surfterm::detector::StateDetector;
use surfterm::session::stream_splitter::StreamSplitter;

/// Build a small chunk (~100 bytes) of mixed PTY output.
fn small_chunk() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"Hello, I'll help you with that\n");
    data.extend_from_slice("⏺ Read src/main.rs\n".as_bytes());
    data.extend_from_slice(b"Cost: $0.05\n");
    data.extend_from_slice(b"\x1b[32mraw output\x1b[0m\n");
    data
}

/// Build a large chunk (~4KB) of mixed PTY output.
fn large_chunk() -> Vec<u8> {
    let mut data = Vec::new();
    for i in 0..100 {
        match i % 4 {
            0 => data.extend_from_slice(b"Hello, I'll help you with that task\n"),
            1 => data.extend_from_slice("⏺ Read file.rs\n".as_bytes()),
            2 => data.extend_from_slice(b"Cost: $0.01\n"),
            _ => data.extend_from_slice(b"\x1b[0mrandom ansi output\x1b[0m\n"),
        }
    }
    data
}

/// Build a multiline chunk that contains many different pattern types.
fn multiline_chunk() -> Vec<u8> {
    let lines = [
        "## Architecture Overview\n",
        "- First item in list\n",
        "1. Numbered step\n",
        "Hello, I'll help you with that\n",
        "⏺ Read src/main.rs\n",
        "Reading file contents...\n",
        "Cost: $0.05\n",
        "Token count: 1234\n",
        "Write src/lib.rs\n",
        "Error: something went wrong\n",
        "\x1b[32mcolored output\x1b[0m\n",
        "some random text\n",
        "Would you like to proceed?\n",
        "Running cargo build\n",
        "Permission denied: /etc/shadow\n",
    ];
    lines.concat().into_bytes()
}

fn bench_classify_small_chunk(c: &mut Criterion) {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, _channels) = StreamSplitter::new(patterns);
    let data = small_chunk();

    c.bench_with_input(
        BenchmarkId::new("stream_splitter/classify", "small_~100B"),
        &data,
        |b, data| {
            b.iter(|| {
                splitter.classify_chunk(std::hint::black_box(data));
            })
        },
    );
}

fn bench_classify_large_chunk(c: &mut Criterion) {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, _channels) = StreamSplitter::new(patterns);
    let data = large_chunk();

    c.bench_with_input(
        BenchmarkId::new("stream_splitter/classify", "large_~4KB"),
        &data,
        |b, data| {
            b.iter(|| {
                splitter.classify_chunk(std::hint::black_box(data));
            })
        },
    );
}

fn bench_classify_multiline(c: &mut Criterion) {
    let patterns = StreamSplitter::default_claude_code_patterns();
    let (splitter, _channels) = StreamSplitter::new(patterns);
    let data = multiline_chunk();

    c.bench_with_input(
        BenchmarkId::new("stream_splitter/classify", "multiline_mixed"),
        &data,
        |b, data| {
            b.iter(|| {
                splitter.classify_chunk(std::hint::black_box(data));
            })
        },
    );
}

fn bench_regex_pattern_matching(c: &mut Criterion) {
    // Benchmark raw regex matching performance against typical inputs
    let regexes: Vec<(&str, Regex)> = vec![
        ("tool_indicator", Regex::new(r"⏺").unwrap()),
        ("cost_line", Regex::new(r"(?i)cost:\s*\$").unwrap()),
        ("ai_greeting", Regex::new(r"(?i)^(hello|hi|hey|I'll help|I can help|let me|sure|certainly)").unwrap()),
        ("permission_prompt", Regex::new(r"(?i)allow|deny|permission").unwrap()),
    ];

    let test_lines = [
        "Hello, I'll help you with that",
        "⏺ Read src/main.rs",
        "Cost: $0.05",
        "Permission denied: /etc/shadow",
        "\x1b[32msome ansi output\x1b[0m",
        "just ordinary text with no match at all",
    ];

    c.bench_function("regex/pattern_matching_all", |b| {
        b.iter(|| {
            for line in &test_lines {
                for (_name, re) in &regexes {
                    std::hint::black_box(re.is_match(line));
                }
            }
        })
    });
}

fn bench_state_detector_process_chunk(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_detector/process_chunk");

    // Small chunk: a single state-triggering line
    let small = "⏺ Read src/main.rs".as_bytes();
    group.bench_with_input(
        BenchmarkId::new("single_line", "running"),
        &small,
        |b, data| {
            let patterns = default_claude_code_state_patterns();
            let (mut detector, _rx) = StateDetector::new(patterns);
            b.iter(|| {
                detector.process_chunk(std::hint::black_box(data));
            })
        },
    );

    // Multi-state chunk: triggers multiple state transitions
    let multi = "⏺ Read src/main.rs\nsome output\nError: something broke\nWould you like to proceed?".as_bytes();
    group.bench_with_input(
        BenchmarkId::new("multi_transition", "run_err_wait"),
        &multi,
        |b, data| {
            let patterns = default_claude_code_state_patterns();
            let (mut detector, _rx) = StateDetector::new(patterns);
            b.iter(|| {
                detector.process_chunk(std::hint::black_box(data));
            })
        },
    );

    // Large chunk: 100 lines of mixed content
    let large = large_chunk();
    group.bench_with_input(
        BenchmarkId::new("large_mixed", "~4KB"),
        &large,
        |b, data| {
            let patterns = default_claude_code_state_patterns();
            let (mut detector, _rx) = StateDetector::new(patterns);
            b.iter(|| {
                detector.process_chunk(std::hint::black_box(data));
            })
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_classify_small_chunk,
    bench_classify_large_chunk,
    bench_classify_multiline,
    bench_regex_pattern_matching,
    bench_state_detector_process_chunk,
);
criterion_main!(benches);
