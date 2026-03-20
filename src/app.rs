/// Application event types for inter-component communication.
#[derive(Debug)]
pub enum AppEvent {
    /// Request a redraw of the window.
    RequestRedraw,
}

#[cfg(test)]
mod tests {}
