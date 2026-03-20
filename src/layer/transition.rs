use crate::layer::{Layer, LayerController};
use crate::session::state::SessionState;
use crate::session::SessionId;

/// Events emitted by automatic layer transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionEvent {
    /// Session was moved to the foreground layer.
    MovedToForeground(SessionId),
    /// Session was moved to the background layer.
    MovedToBackground(SessionId),
    /// No transition occurred.
    NoChange,
}

/// Apply an automatic layer transition based on a session state change.
///
/// Pinned sessions are never moved by automatic transitions.
///
/// Rules:
/// - `WaitingForInput` → move to foreground
/// - `Error` → move to foreground
/// - `Running` (from `WaitingForInput`) → move to background
/// - `Idle` → no change
#[allow(dead_code)]
pub fn apply_state_change(
    controller: &mut LayerController,
    id: &SessionId,
    old_state: SessionState,
    new_state: SessionState,
) -> TransitionEvent {
    // Pinned sessions are exempt from automatic transitions.
    if controller.get_layer(id) == Some(Layer::Pinned) {
        return TransitionEvent::NoChange;
    }

    match new_state {
        SessionState::WaitingForInput | SessionState::Error => {
            controller.move_to_foreground(id);
            TransitionEvent::MovedToForeground(*id)
        }
        SessionState::Running if old_state == SessionState::WaitingForInput => {
            controller.move_to_background(id);
            TransitionEvent::MovedToBackground(*id)
        }
        _ => TransitionEvent::NoChange,
    }
}

/// Apply an automatic layer transition when the user sends input to a session.
///
/// The session is moved to background unless it is pinned.
#[allow(dead_code)]
pub fn apply_user_input(
    controller: &mut LayerController,
    id: &SessionId,
) -> TransitionEvent {
    if controller.get_layer(id) == Some(Layer::Pinned) {
        return TransitionEvent::NoChange;
    }

    controller.move_to_background(id);
    TransitionEvent::MovedToBackground(*id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_waiting_for_input_triggers_foreground() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Background);

        let event = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Running,
            SessionState::WaitingForInput,
        );

        assert_eq!(ctrl.get_layer(&id), Some(Layer::Foreground));
        assert_eq!(event, TransitionEvent::MovedToForeground(id));
    }

    #[test]
    fn test_error_triggers_foreground() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Background);

        let event = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Running,
            SessionState::Error,
        );

        assert_eq!(ctrl.get_layer(&id), Some(Layer::Foreground));
        assert_eq!(event, TransitionEvent::MovedToForeground(id));
    }

    #[test]
    fn test_user_input_triggers_background() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Foreground);

        let event = apply_user_input(&mut ctrl, &id);

        assert_eq!(ctrl.get_layer(&id), Some(Layer::Background));
        assert_eq!(event, TransitionEvent::MovedToBackground(id));
    }

    #[test]
    fn test_pinned_not_moved_by_state_change() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Pinned);

        let event = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Running,
            SessionState::WaitingForInput,
        );

        assert_eq!(ctrl.get_layer(&id), Some(Layer::Pinned));
        assert_eq!(event, TransitionEvent::NoChange);
    }

    #[test]
    fn test_pinned_not_moved_by_user_input() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Pinned);

        let event = apply_user_input(&mut ctrl, &id);

        assert_eq!(ctrl.get_layer(&id), Some(Layer::Pinned));
        assert_eq!(event, TransitionEvent::NoChange);
    }

    #[test]
    fn test_multiple_waiting_for_input_queued() {
        let mut ctrl = LayerController::new();
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        let id3 = SessionId::new();
        ctrl.assign(id1, Layer::Background);
        ctrl.assign(id2, Layer::Background);
        ctrl.assign(id3, Layer::Background);

        // All three transition to WaitingForInput
        apply_state_change(
            &mut ctrl,
            &id1,
            SessionState::Running,
            SessionState::WaitingForInput,
        );
        apply_state_change(
            &mut ctrl,
            &id2,
            SessionState::Running,
            SessionState::WaitingForInput,
        );
        apply_state_change(
            &mut ctrl,
            &id3,
            SessionState::Running,
            SessionState::WaitingForInput,
        );

        let fg = ctrl.foreground_sessions();
        assert!(fg.contains(&id1));
        assert!(fg.contains(&id2));
        assert!(fg.contains(&id3));
        assert_eq!(fg.len(), 3);

        // Most recently promoted is primary (id3 was last)
        assert_eq!(ctrl.primary_foreground(), Some(id3));
    }

    #[test]
    fn test_running_after_waiting_moves_to_background() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Background);

        // First go to WaitingForInput
        apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Running,
            SessionState::WaitingForInput,
        );
        assert_eq!(ctrl.get_layer(&id), Some(Layer::Foreground));

        // Then transition to Running (from WaitingForInput)
        let event = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::WaitingForInput,
            SessionState::Running,
        );

        assert_eq!(ctrl.get_layer(&id), Some(Layer::Background));
        assert_eq!(event, TransitionEvent::MovedToBackground(id));
    }

    #[test]
    fn test_idle_causes_no_change() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Background);

        let event = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Running,
            SessionState::Idle,
        );

        assert_eq!(ctrl.get_layer(&id), Some(Layer::Background));
        assert_eq!(event, TransitionEvent::NoChange);
    }

    #[test]
    fn test_transition_events_are_correct() {
        let mut ctrl = LayerController::new();
        let id = SessionId::new();
        ctrl.assign(id, Layer::Background);

        // WaitingForInput → MovedToForeground
        let e1 = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Idle,
            SessionState::WaitingForInput,
        );
        assert_eq!(e1, TransitionEvent::MovedToForeground(id));

        // User input → MovedToBackground
        let e2 = apply_user_input(&mut ctrl, &id);
        assert_eq!(e2, TransitionEvent::MovedToBackground(id));

        // Error → MovedToForeground
        let e3 = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Running,
            SessionState::Error,
        );
        assert_eq!(e3, TransitionEvent::MovedToForeground(id));

        // Idle → NoChange
        let e4 = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Error,
            SessionState::Idle,
        );
        assert_eq!(e4, TransitionEvent::NoChange);

        // Running from non-WaitingForInput → NoChange
        let e5 = apply_state_change(
            &mut ctrl,
            &id,
            SessionState::Idle,
            SessionState::Running,
        );
        assert_eq!(e5, TransitionEvent::NoChange);
    }
}
