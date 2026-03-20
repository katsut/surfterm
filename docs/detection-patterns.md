# Detection Patterns

Orchesterm uses regex-based pattern matching to classify PTY output and detect
session state transitions. Patterns are defined in TOML files so they can be
updated without recompiling.

## TOML Format

Pattern files live under `~/.config/orchesterm/detectors/` and use the
`[[patterns]]` array-of-tables syntax. Each entry has three required fields:

| Field   | Type   | Description                                           |
|---------|--------|-------------------------------------------------------|
| `name`  | String | Human-readable identifier for the pattern             |
| `regex` | String | Rust-compatible regular expression                    |
| `state` | String | Target state or classification (see sections below)   |

### Stream Patterns (Classification)

Stream patterns control how PTY output lines are routed to the three display
channels. The `classification` field accepts:

| Value     | Description                                              |
|-----------|----------------------------------------------------------|
| `Message` | Conversation text from the AI tool (left chat panel)     |
| `State`   | Status information: tool execution, cost, tokens (right) |
| `Raw`     | Unclassified VT output (toggle-visible)                  |

Lines that do not match any pattern default to `Raw`.

#### Stream pattern examples

```toml
# Matches Claude Code tool-use indicator
[[patterns]]
name = "tool_use_indicator"
regex = "⏺"
classification = "State"

# Matches cost output
[[patterns]]
name = "cost_line"
regex = "(?i)cost:\\s*\\$"
classification = "State"

# Matches AI greeting lines
[[patterns]]
name = "ai_greeting"
regex = "(?i)^(hello|hi|hey|I'll help|I can help|let me|sure|certainly)"
classification = "Message"

# Matches markdown headers in AI output
[[patterns]]
name = "ai_markdown_header"
regex = "^#{1,6}\\s"
classification = "Message"

# Matches bullet lists
[[patterns]]
name = "ai_bullet_list"
regex = "^\\s*[-*]\\s"
classification = "Message"
```

### State Patterns (SessionState)

State patterns determine the session lifecycle state. The `state` field accepts:

| Value              | Description                                         |
|--------------------|-----------------------------------------------------|
| `Idle`             | Session has no activity                              |
| `Running`          | AI tool is actively processing                       |
| `WaitingForInput`  | AI tool is waiting for a user decision               |
| `Error`            | Session encountered an error                         |

When multiple lines in a chunk match different states, the last matching line
wins. If no line matches, the state does not change.

#### State pattern examples

```toml
# Prompt character indicates waiting for input
[[patterns]]
name = "prompt_char"
regex = "^>\\s"
state = "WaitingForInput"

# Y/n confirmation prompt
[[patterns]]
name = "yn_prompt"
regex = "(?i)Y/n"
state = "WaitingForInput"

# Tool indicator means the AI is working
[[patterns]]
name = "tool_indicator"
regex = "⏺"
state = "Running"

# Active file operations
[[patterns]]
name = "reading"
regex = "(?i)\\bReading\\b"
state = "Running"

# Error messages
[[patterns]]
name = "error_upper"
regex = "Error:"
state = "Error"

# Panic output
[[patterns]]
name = "panic"
regex = "\\bpanic\\b"
state = "Error"
```

## Adding a New AI Tool Definition

To add detection patterns for a new AI coding tool:

1. Create a new file under `~/.config/orchesterm/detectors/`, e.g.
   `~/.config/orchesterm/detectors/my-tool.toml`.

2. Define `[[patterns]]` entries that match the tool's output. Consider:
   - **WaitingForInput**: prompts, confirmation dialogs, input cursors
   - **Running**: progress indicators, file operations, build commands
   - **Error**: error messages, failures, panics, permission denials

3. Use `(?i)` for case-insensitive matching where appropriate.

4. Use word boundaries (`\b`) to avoid false positives on partial matches.

5. Put more specific patterns before general ones. Patterns are evaluated in
   order and the first match wins (for stream classification) or the last
   match wins (for state detection).

## Complete Example: Hypothetical "CodePilot" Tool

Below is a full TOML file for a hypothetical AI tool called "CodePilot":

```toml
# ~/.config/orchesterm/detectors/codepilot.toml
#
# Detection patterns for CodePilot AI coding assistant.

# --- WaitingForInput patterns ---

[[patterns]]
name = "codepilot_prompt"
regex = "^codepilot>"
state = "WaitingForInput"

[[patterns]]
name = "codepilot_confirm"
regex = "(?i)confirm\\? \\[y/n\\]"
state = "WaitingForInput"

[[patterns]]
name = "codepilot_choose"
regex = "(?i)please choose an option"
state = "WaitingForInput"

# --- Running patterns ---

[[patterns]]
name = "codepilot_thinking"
regex = "\\[thinking\\.\\.\\.]"
state = "Running"

[[patterns]]
name = "codepilot_applying"
regex = "(?i)applying changes to"
state = "Running"

[[patterns]]
name = "codepilot_analyzing"
regex = "(?i)analyzing \\d+ files"
state = "Running"

[[patterns]]
name = "codepilot_building"
regex = "(?i)\\bbuilding\\b.*\\bproject\\b"
state = "Running"

# --- Error patterns ---

[[patterns]]
name = "codepilot_error"
regex = "^\\[ERROR\\]"
state = "Error"

[[patterns]]
name = "codepilot_fatal"
regex = "(?i)fatal:"
state = "Error"

[[patterns]]
name = "codepilot_crash"
regex = "(?i)codepilot crashed"
state = "Error"

# --- Idle patterns ---

[[patterns]]
name = "codepilot_ready"
regex = "(?i)codepilot is ready"
state = "Idle"
```

## Regex Tips

- Escape backslashes in TOML strings: `\\b` for the regex `\b`.
- Use raw-ish patterns: `"^>\\s"` matches a `>` at line start followed by
  whitespace.
- Test patterns with `regex` crate syntax (Rust flavor, no lookahead).
- Keep patterns fast: avoid excessive backtracking with nested quantifiers.
- Multi-byte UTF-8 characters (e.g. `⏺`) work as literal matches.
