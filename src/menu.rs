//! The native macOS menu bar.

use std::sync::mpsc::{Receiver, channel};

use eframe::egui;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

#[derive(Clone, Copy)]
pub enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
}

pub struct NativeMenu {
    // Dropping the menu removes it from the menu bar.
    _menu: Menu,
    items: Vec<(MenuId, Command)>,
    undo: MenuItem,
    redo: MenuItem,
    events: Receiver<MenuEvent>,
}

impl NativeMenu {
    pub fn install(ctx: &egui::Context) -> Option<Self> {
        let cmd = Modifiers::META;
        let cmd_shift = Modifiers::META | Modifiers::SHIFT;
        let entries = [
            ("New", cmd, Code::KeyN, Command::New),
            ("Open…", cmd, Code::KeyO, Command::Open),
            ("Save", cmd, Code::KeyS, Command::Save),
            ("Save As…", cmd_shift, Code::KeyS, Command::SaveAs),
        ];
        let file = Submenu::new("File", true);
        let mut items = Vec::new();
        for (i, (label, mods, code, command)) in entries.into_iter().enumerate() {
            let item = MenuItem::new(label, true, Some(Accelerator::new(mods, code)));
            items.push((item.id().clone(), command));
            if i == 2 {
                file.append(&PredefinedMenuItem::separator()).ok()?;
            }
            file.append(&item).ok()?;
        }

        // The first submenu becomes the app menu on macOS.
        let app = Submenu::with_items(
            "Skwiggle",
            true,
            &[
                &PredefinedMenuItem::about(None, None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ],
        )
        .ok()?;
        let undo = MenuItem::new("Undo", false, Some(Accelerator::new(cmd, Code::KeyZ)));
        let redo = MenuItem::new("Redo", false, Some(Accelerator::new(cmd_shift, Code::KeyZ)));
        items.push((undo.id().clone(), Command::Undo));
        items.push((redo.id().clone(), Command::Redo));
        let edit = Submenu::with_items("Edit", true, &[&undo, &redo]).ok()?;

        let menu = Menu::with_items(&[&app, &file, &edit]).ok()?;
        menu.init_for_nsapp();

        let (tx, events) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));
        Some(Self { _menu: menu, items, undo, redo, events })
    }

    /// Greys out Undo and Redo when there's nothing to step to.
    pub fn set_history(&self, can_undo: bool, can_redo: bool) {
        if self.undo.is_enabled() != can_undo {
            self.undo.set_enabled(can_undo);
        }
        if self.redo.is_enabled() != can_redo {
            self.redo.set_enabled(can_redo);
        }
    }

    /// Commands picked since the last call.
    pub fn poll(&self) -> Vec<Command> {
        self.events
            .try_iter()
            .filter_map(|e| self.items.iter().find(|(id, _)| *id == e.id).map(|&(_, c)| c))
            .collect()
    }
}
