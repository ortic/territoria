//! All user actions as commands.
//!
//! Menus, toolbar, keyboard shortcuts and the command palette dispatch a
//! [`Command`], so every action is reachable from each of them.

use egui::{Key, KeyboardShortcut, Modifiers};

macro_rules! commands {
    ($($variant:ident => $label:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Command {
            $($variant,)*
        }

        impl Command {
            pub const ALL: &[Command] = &[$(Command::$variant,)*];

            pub fn label(self) -> &'static str {
                match self {
                    $(Command::$variant => $label,)*
                }
            }
        }
    };
}

commands! {
    NewProject => "File: New Project",
    OpenProject => "File: Open Project…",
    SaveProject => "File: Save",
    SaveProjectAs => "File: Save As…",
    AddBoundaries => "File: Add Boundaries (Shapefile, GeoJSON, GeoPackage)…",
    AddData => "File: Add Data (CSV, Excel)…",
    DownloadBoundaries => "File: Download Open Boundaries…",
    Settings => "File: Settings…",
    Quit => "File: Exit",
    Undo => "Edit: Undo",
    Redo => "Edit: Redo",
    LinkData => "Data: Link Table to Map…",
    MergeAreas => "Data: New Layer by Merging Areas…",
    CalculatedColumn => "Data: Calculated Column…",
    HexGrid => "Data: New Hexagon Grid…",
    Catchment => "Map: Catchment Rings Around Points…",
    ColorByValue => "Map: Colour Areas by Value…",
    NewMap => "Map: New Map",
    ExportMap => "Map: Export Map (PNG, SVG, PDF)…",
    CopyMap => "Map: Copy Map as Image",
    ZoomToAll => "Map: Zoom to Everything",
    ToolNavigate => "Map: Navigate Tool",
    ToolPaint => "Map: Paint Territories Tool",
    ToolSelect => "Map: Select Tool",
    SelectByList => "Map: Select Areas by List…",
    NewPlan => "Territories: New Territory Plan…",
    NewTerritory => "Territories: New Territory",
    FillGaps => "Territories: Assign Unassigned Areas to Neighbours",
    Recolor => "Territories: Recolour Territories",
    Balance => "Territories: Balance Territories…",
    ShowPalette => "View: Command Palette",
    Find => "View: Find an Area or Territory",
    Presentation => "View: Presentation (full screen, map only)",
    SaveView => "Map: Keep This View…",
    ToggleContents => "View: Contents Panel",
    ToggleInspector => "View: Properties Panel",
    ThemeDark => "View: Dark Theme",
    ThemeLight => "View: Light Theme",
    ThemeSystem => "View: System Theme",
    ZoomIn => "View: Zoom In (UI)",
    ZoomOut => "View: Zoom Out (UI)",
    ZoomReset => "View: Reset Zoom (UI)",
    About => "Help: About Territoria",
}

impl Command {
    /// Label without the category prefix, for menus.
    pub fn short_label(self) -> &'static str {
        let label = self.label();
        label.split_once(": ").map_or(label, |(_, rest)| rest)
    }
}

/// Maps commands to keyboard shortcuts.
pub struct Keymap {
    bindings: Vec<(Command, KeyboardShortcut)>,
}

impl Default for Keymap {
    fn default() -> Self {
        let ctrl = Modifiers::COMMAND;
        let shift = Modifiers::SHIFT;
        let none = Modifiers::NONE;
        let k = KeyboardShortcut::new;
        Keymap {
            bindings: vec![
                (Command::NewProject, k(ctrl, Key::N)),
                (Command::OpenProject, k(ctrl, Key::O)),
                (Command::SaveProject, k(ctrl, Key::S)),
                (Command::SaveProjectAs, k(ctrl | shift, Key::S)),
                (Command::AddBoundaries, k(ctrl | shift, Key::B)),
                (Command::AddData, k(ctrl | shift, Key::D)),
                (Command::Undo, k(ctrl, Key::Z)),
                (Command::Redo, k(ctrl | shift, Key::Z)),
                (Command::Redo, k(ctrl, Key::Y)),
                (Command::ShowPalette, k(ctrl | shift, Key::P)),
                (Command::Settings, k(ctrl, Key::Comma)),
                (Command::Find, k(ctrl, Key::F)),
                (Command::Presentation, k(none, Key::F11)),
                (Command::CopyMap, k(ctrl | shift, Key::C)),
                (Command::ZoomToAll, k(none, Key::Home)),
                (Command::ToolNavigate, k(none, Key::V)),
                (Command::ToolPaint, k(none, Key::B)),
                (Command::ToolSelect, k(none, Key::S)),
                (Command::NewTerritory, k(ctrl | shift, Key::T)),
                (Command::ZoomIn, k(ctrl, Key::Plus)),
                (Command::ZoomIn, k(ctrl, Key::Equals)),
                (Command::ZoomOut, k(ctrl, Key::Minus)),
                (Command::ZoomReset, k(ctrl, Key::Num0)),
            ],
        }
    }
}

impl Keymap {
    pub fn shortcut(&self, command: Command) -> Option<&KeyboardShortcut> {
        self.bindings
            .iter()
            .find(|(c, _)| *c == command)
            .map(|(_, s)| s)
    }

    /// Formatted shortcut for menus and tooltips (empty if none).
    pub fn shortcut_text(&self, ctx: &egui::Context, command: Command) -> String {
        self.shortcut(command)
            .map(|s| ctx.format_shortcut(s))
            .unwrap_or_default()
    }

    /// Consumes pressed shortcuts and returns the triggered commands. Single
    /// keys without modifiers only count when no text field has focus.
    pub fn triggered(&self, ctx: &egui::Context) -> Vec<Command> {
        let typing = ctx.egui_wants_keyboard_input();
        let mut bindings: Vec<_> = self
            .bindings
            .iter()
            .filter(|(_, s)| !(typing && s.modifiers.is_none()))
            .collect();
        // Most specific first, so Ctrl+Shift+S isn't eaten by Ctrl+S.
        bindings.sort_by_key(|(_, s)| {
            std::cmp::Reverse(
                u8::from(s.modifiers.ctrl || s.modifiers.command)
                    + u8::from(s.modifiers.shift)
                    + u8::from(s.modifiers.alt),
            )
        });
        ctx.input_mut(|input| {
            bindings
                .into_iter()
                .filter(|(_, s)| input.consume_shortcut(s))
                .map(|(c, _)| *c)
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_have_categories() {
        for c in Command::ALL {
            assert!(c.label().contains(": "), "{c:?}");
            assert!(!c.short_label().is_empty());
        }
    }
}
