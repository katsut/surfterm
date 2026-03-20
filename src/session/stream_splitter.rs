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

#[cfg(test)]
mod tests {}
