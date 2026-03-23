//! macOS menu bar setup using the `muda` crate.
//!
//! Provides standard macOS menus: Surfterm (app), File, Edit, View, Window.
//! Custom menu actions are mapped to [`MenuAction`] variants which the
//! application event loop translates into the appropriate commands.

use muda::{
    accelerator::{Accelerator, Code, Modifiers},
    AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tracing::instrument;

/// Actions triggered by custom (non-predefined) menu items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    NewSession,
    CloseSession,
    ToggleRawView,
    ToggleWs,
}

/// Holds the menu bar and IDs needed to map events to actions.
pub struct AppMenu {
    /// The top-level menu bar (must be kept alive).
    #[allow(dead_code)]
    menu: Menu,
    // Custom menu item IDs
    new_session_id: MenuId,
    close_session_id: MenuId,
    toggle_raw_id: MenuId,
    toggle_ws_id: MenuId,
}

impl Default for AppMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl AppMenu {
    /// Build the full menu bar and attach it to the macOS application.
    ///
    /// Must be called after the window has been created (during `resumed`).
    #[instrument(skip_all)]
    pub fn new() -> Self {
        let menu = Menu::new();

        // ── Surfterm (app) menu ──────────────────────────────────────
        let app_menu = Submenu::new("Surfterm", true);
        app_menu
            .append_items(&[
                &PredefinedMenuItem::about(
                    Some("About Surfterm"),
                    Some(AboutMetadata {
                        name: Some("Surfterm".to_string()),
                        version: Some(env!("CARGO_PKG_VERSION").to_string()),
                        ..Default::default()
                    }),
                ),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(Some("Quit Surfterm")),
            ])
            .expect("failed to build app menu");

        // ── File menu ────────────────────────────────────────────────
        let file_menu = Submenu::new("File", true);
        let new_session = MenuItem::new(
            "New Session",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyN)),
        );
        let close_session = MenuItem::new(
            "Close Session",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyW)),
        );
        file_menu
            .append_items(&[&new_session, &close_session])
            .expect("failed to build file menu");

        // ── Edit menu ────────────────────────────────────────────────
        let edit_menu = Submenu::new("Edit", true);
        edit_menu
            .append_items(&[
                &PredefinedMenuItem::undo(None),
                &PredefinedMenuItem::redo(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::cut(None),
                &PredefinedMenuItem::copy(None),
                &PredefinedMenuItem::paste(None),
                &PredefinedMenuItem::select_all(None),
            ])
            .expect("failed to build edit menu");

        // ── View menu ────────────────────────────────────────────────
        let view_menu = Submenu::new("View", true);
        let toggle_raw = MenuItem::new(
            "Toggle Raw/Panels",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyR)),
        );
        let toggle_ws = MenuItem::new(
            "Toggle WebSocket Server",
            true,
            Some(Accelerator::new(
                Some(Modifiers::SUPER | Modifiers::SHIFT),
                Code::KeyW,
            )),
        );
        view_menu
            .append_items(&[
                &toggle_raw,
                &PredefinedMenuItem::separator(),
                &toggle_ws,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::fullscreen(None),
            ])
            .expect("failed to build view menu");

        // ── Window menu ──────────────────────────────────────────────
        let window_menu = Submenu::new("Window", true);
        window_menu
            .append_items(&[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::close_window(None),
            ])
            .expect("failed to build window menu");

        // ── Assemble the top-level menu bar ──────────────────────────
        menu.append_items(&[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu])
            .expect("failed to assemble menu bar");

        // On macOS, init_for_nsapp sets this as the application main menu.
        #[cfg(target_os = "macos")]
        {
            menu.init_for_nsapp();
        }

        let new_session_id = new_session.id().clone();
        let close_session_id = close_session.id().clone();
        let toggle_raw_id = toggle_raw.id().clone();
        let toggle_ws_id = toggle_ws.id().clone();

        tracing::info!("macOS menu bar initialized");

        Self {
            menu,
            new_session_id,
            close_session_id,
            toggle_raw_id,
            toggle_ws_id,
        }
    }

    /// Map a [`MenuEvent`] to an application-level [`MenuAction`], if it
    /// corresponds to one of our custom menu items. Returns `None` for
    /// predefined items (About, Quit, Copy, etc.) which are handled by the OS.
    pub fn action_for_event(&self, event: &MenuEvent) -> Option<MenuAction> {
        let id = event.id();
        if *id == self.new_session_id {
            Some(MenuAction::NewSession)
        } else if *id == self.close_session_id {
            Some(MenuAction::CloseSession)
        } else if *id == self.toggle_raw_id {
            Some(MenuAction::ToggleRawView)
        } else if *id == self.toggle_ws_id {
            Some(MenuAction::ToggleWs)
        } else {
            None
        }
    }
}
