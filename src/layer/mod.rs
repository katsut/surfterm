use std::collections::HashMap;

use crate::session::SessionId;

/// Layer assignment for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Active session displayed prominently.
    Foreground,
    /// Running session collapsed to one line.
    Background,
    /// Manually pinned to foreground regardless of state.
    Pinned,
}

/// Manages layer assignments for all tracked sessions.
#[allow(dead_code)]
pub struct LayerController {
    /// Maps each session to its current layer.
    assignments: HashMap<SessionId, Layer>,
    /// Ordered queue of foreground candidates (most recently promoted first).
    foreground_queue: Vec<SessionId>,
}

#[allow(dead_code)]
impl LayerController {
    /// Create a new empty controller.
    pub fn new() -> Self {
        Self {
            assignments: HashMap::new(),
            foreground_queue: Vec::new(),
        }
    }

    /// Assign a session to a layer.
    pub fn assign(&mut self, id: SessionId, layer: Layer) {
        self.assignments.insert(id, layer);
        match layer {
            Layer::Foreground | Layer::Pinned => {
                // Add to front of queue if not already present
                if !self.foreground_queue.contains(&id) {
                    self.foreground_queue.insert(0, id);
                }
            }
            Layer::Background => {
                self.foreground_queue.retain(|sid| sid != &id);
            }
        }
    }

    /// Remove a session from tracking entirely.
    pub fn remove(&mut self, id: &SessionId) {
        self.assignments.remove(id);
        self.foreground_queue.retain(|sid| sid != id);
    }

    /// Get the layer assignment for a session.
    pub fn get_layer(&self, id: &SessionId) -> Option<Layer> {
        self.assignments.get(id).copied()
    }

    /// Pin a session (set to Pinned layer). If the session is not tracked, this is a no-op.
    pub fn pin(&mut self, id: &SessionId) {
        if self.assignments.contains_key(id) {
            self.assignments.insert(*id, Layer::Pinned);
            if !self.foreground_queue.contains(id) {
                self.foreground_queue.insert(0, *id);
            }
        }
    }

    /// Unpin a session (move from Pinned back to Foreground). No-op if not Pinned.
    pub fn unpin(&mut self, id: &SessionId) {
        if self.assignments.get(id) == Some(&Layer::Pinned) {
            self.assignments.insert(*id, Layer::Foreground);
        }
    }

    /// Return all sessions in Foreground or Pinned layers.
    pub fn foreground_sessions(&self) -> Vec<SessionId> {
        self.foreground_queue
            .iter()
            .filter(|id| {
                matches!(
                    self.assignments.get(id),
                    Some(Layer::Foreground) | Some(Layer::Pinned)
                )
            })
            .copied()
            .collect()
    }

    /// Return all sessions in the Background layer.
    pub fn background_sessions(&self) -> Vec<SessionId> {
        self.assignments
            .iter()
            .filter(|(_, layer)| **layer == Layer::Background)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Return the primary foreground session: first Pinned in queue, or first Foreground.
    pub fn primary_foreground(&self) -> Option<SessionId> {
        // Prefer pinned sessions first
        for id in &self.foreground_queue {
            if self.assignments.get(id) == Some(&Layer::Pinned) {
                return Some(*id);
            }
        }
        // Then first foreground in queue
        for id in &self.foreground_queue {
            if self.assignments.get(id) == Some(&Layer::Foreground) {
                return Some(*id);
            }
        }
        None
    }

    /// Move a session to the front of the foreground queue and set its layer to Foreground.
    pub fn move_to_foreground(&mut self, id: &SessionId) {
        if let Some(layer) = self.assignments.get(id) {
            // Don't demote Pinned sessions; just move them to front of queue
            if *layer != Layer::Pinned {
                self.assignments.insert(*id, Layer::Foreground);
            }
            self.foreground_queue.retain(|sid| sid != id);
            self.foreground_queue.insert(0, *id);
        }
    }

    /// Move a session to Background layer.
    pub fn move_to_background(&mut self, id: &SessionId) {
        if self.assignments.contains_key(id) {
            self.assignments.insert(*id, Layer::Background);
            self.foreground_queue.retain(|sid| sid != id);
        }
    }
}

impl Default for LayerController {
    fn default() -> Self {
        Self::new()
    }
}

/// Layout computed from a `LayerController`, consumed by the renderer.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The session to render in the main area.
    pub primary: Option<SessionId>,
    /// All foreground/pinned sessions.
    pub foreground: Vec<SessionId>,
    /// All background sessions.
    pub background: Vec<SessionId>,
}

#[allow(dead_code)]
impl Layout {
    /// Build a `Layout` from the current state of a `LayerController`.
    pub fn from_controller(controller: &LayerController) -> Self {
        Self {
            primary: controller.primary_foreground(),
            foreground: controller.foreground_sessions(),
            background: controller.background_sessions(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assign_and_get_layer() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Foreground);
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Foreground));

        ctrl.assign(id, Layer::Background);
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Background));
    }

    #[test]
    fn test_pin_and_unpin() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Foreground);

        ctrl.pin(&id);
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Pinned));

        ctrl.unpin(&id);
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Foreground));
    }

    #[test]
    fn test_foreground_sessions_includes_pinned() {
        let mut ctrl = LayerController::new();
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        let id3 = SessionId::new();

        ctrl.assign(id1, Layer::Foreground);
        ctrl.assign(id2, Layer::Pinned);
        ctrl.assign(id3, Layer::Background);

        let fg = ctrl.foreground_sessions();
        assert!(fg.contains(&id1));
        assert!(fg.contains(&id2));
        assert!(!fg.contains(&id3));
    }

    #[test]
    fn test_background_sessions() {
        let mut ctrl = LayerController::new();
        let id1 = SessionId::new();
        let id2 = SessionId::new();

        ctrl.assign(id1, Layer::Foreground);
        ctrl.assign(id2, Layer::Background);

        let bg = ctrl.background_sessions();
        assert!(!bg.contains(&id1));
        assert!(bg.contains(&id2));
    }

    #[test]
    fn test_primary_foreground_prefers_pinned() {
        let mut ctrl = LayerController::new();
        let id1 = SessionId::new();
        let id2 = SessionId::new();

        ctrl.assign(id1, Layer::Foreground);
        ctrl.assign(id2, Layer::Pinned);

        // Pinned session should be primary even if added after foreground
        assert_eq!(ctrl.primary_foreground(), Some(id2));
    }

    #[test]
    fn test_primary_foreground_returns_first_foreground_when_no_pinned() {
        let mut ctrl = LayerController::new();
        let id1 = SessionId::new();
        let id2 = SessionId::new();

        ctrl.assign(id1, Layer::Foreground);
        ctrl.assign(id2, Layer::Foreground);

        // id2 was assigned last, so it's at front of queue
        assert_eq!(ctrl.primary_foreground(), Some(id2));
    }

    #[test]
    fn test_primary_foreground_empty() {
        let ctrl = LayerController::new();
        assert_eq!(ctrl.primary_foreground(), None);
    }

    #[test]
    fn test_move_to_foreground() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Background);

        ctrl.move_to_foreground(&id);
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Foreground));
        assert!(ctrl.foreground_sessions().contains(&id));
        assert!(!ctrl.background_sessions().contains(&id));
    }

    #[test]
    fn test_move_to_background() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Foreground);

        ctrl.move_to_background(&id);
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Background));
        assert!(!ctrl.foreground_sessions().contains(&id));
        assert!(ctrl.background_sessions().contains(&id));
    }

    #[test]
    fn test_remove_session() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Foreground);

        ctrl.remove(&id);
        assert_eq!(ctrl.get_layer(&id), None);
        assert!(!ctrl.foreground_sessions().contains(&id));
        assert!(!ctrl.background_sessions().contains(&id));
    }

    #[test]
    fn test_layout_construction() {
        let mut ctrl = LayerController::new();
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        let id3 = SessionId::new();

        ctrl.assign(id1, Layer::Pinned);
        ctrl.assign(id2, Layer::Foreground);
        ctrl.assign(id3, Layer::Background);

        let layout = Layout::from_controller(&ctrl);
        assert_eq!(layout.primary, Some(id1));
        assert!(layout.foreground.contains(&id1));
        assert!(layout.foreground.contains(&id2));
        assert!(!layout.foreground.contains(&id3));
        assert!(layout.background.contains(&id3));
        assert!(!layout.background.contains(&id1));
    }

    #[test]
    fn test_move_to_foreground_preserves_pinned() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Pinned);

        ctrl.move_to_foreground(&id);
        // Should stay Pinned, not demoted to Foreground
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Pinned));
    }

    #[test]
    fn test_pin_noop_for_untracked() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.pin(&id); // should not panic
        assert_eq!(ctrl.get_layer(&id), None);
    }

    #[test]
    fn test_unpin_noop_for_non_pinned() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Foreground);
        ctrl.unpin(&id);
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Foreground));
    }
}
