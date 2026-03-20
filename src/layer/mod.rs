/// Layer assignment for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Layer {
    /// Active session displayed prominently.
    Foreground,
    /// Running session collapsed to one line.
    Background,
    /// Manually pinned to foreground regardless of state.
    Pinned,
}

#[cfg(test)]
mod tests {}
