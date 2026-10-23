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
    AddBoundaries => "File: Add Boundaries (Shapefile, GeoJSON, GeoPackage, KML)…",
    AddData => "File: Add Data (CSV, Excel)…",
    DownloadBoundaries => "File: Download Open Boundaries…",
    Settings => "File: Settings…",
    Quit => "File: Exit",
    Undo => "Edit: Undo",
    Redo => "Edit: Redo",
    LinkData => "Data: Link Table to Map…",
    MergeAreas => "Data: New Layer by Merging Areas…",
    ReplaceLayer => "Data: Replace Boundaries with a Newer Version…",
    CalculatedColumn => "Data: Calculated Column…",
    HexGrid => "Data: New Hexagon Grid…",
    Catchment => "Map: Catchment Rings Around Points…",
    FlowLines => "Map: Flow Lines Between Places…",
    ColorByValue => "Map: Colour Areas by Value…",
    NewMap => "Map: New Map",
    ExportMap => "Map: Export Map (PNG, SVG, PDF)…",
    CopyMap => "Map: Copy Map as Image",
    ExportHtml => "Map: Export as Web Page (zoom, hover)…",
    ExportAtlas => "Map: Export Atlas (a page per region, rep or area)…",
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
    Help => "Help: Help Topics",
    About => "Help: About Territoria",
}

impl Command {
    /// A stable name for settings files.
    pub fn id(self) -> String {
        format!("{self:?}")
    }

    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.iter().copied().find(|c| c.id() == id)
    }

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
                (Command::Help, k(none, Key::F1)),
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

/// Keys as text for settings files: `Ctrl+Shift+S` (Ctrl is Cmd on macOS).
pub fn shortcut_to_text(s: &KeyboardShortcut) -> String {
    let mut out = String::new();
    if s.modifiers.command || s.modifiers.ctrl {
        out.push_str("Ctrl+");
    }
    if s.modifiers.alt {
        out.push_str("Alt+");
    }
    if s.modifiers.shift {
        out.push_str("Shift+");
    }
    out.push_str(s.logical_key.name());
    out
}

/// Keys from text like `Ctrl+Shift+S`.
pub fn shortcut_from_text(text: &str) -> Option<KeyboardShortcut> {
    if text.trim().is_empty() {
        return None;
    }
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => modifiers |= Modifiers::COMMAND,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "" => key = Key::from_name("+").or(key),
            _ => key = Some(Key::from_name(part)?),
        }
    }
    Some(KeyboardShortcut::new(modifiers, key?))
}

impl Keymap {
    /// The default keys with the user's changes: a command named there
    /// gets just that shortcut (none when empty).
    pub fn with_changes(changes: &std::collections::BTreeMap<String, String>) -> Keymap {
        let mut map = Keymap::default();
        for (id, keys) in changes {
            let Some(c) = Command::from_id(id) else {
                continue;
            };
            map.bindings.retain(|(b, _)| *b != c);
            if let Some(s) = shortcut_from_text(keys) {
                map.bindings.push((c, s));
            }
        }
        map
    }

    /// The command that has these keys, if any.
    pub fn command_of(&self, shortcut: &KeyboardShortcut) -> Option<Command> {
        self.bindings
            .iter()
            .find(|(_, s)| s == shortcut)
            .map(|(c, _)| *c)
    }

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
    fn shortcuts_as_text_and_changed() {
        let s = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        assert_eq!(shortcut_to_text(&s), "Ctrl+Shift+S");
        assert_eq!(shortcut_from_text("Ctrl+Shift+S"), Some(s));
        assert_eq!(
            shortcut_from_text("F1"),
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::F1))
        );
        assert_eq!(shortcut_from_text("Ctrl+Nope"), None);
        let mut changes = std::collections::BTreeMap::new();
        changes.insert("Find".to_owned(), "Ctrl+K".to_owned());
        changes.insert("Undo".to_owned(), String::new());
        let map = Keymap::with_changes(&changes);
        let k = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
        assert_eq!(map.command_of(&k), Some(Command::Find));
        assert!(map.shortcut(Command::Undo).is_none());
        assert!(map.shortcut(Command::Redo).is_some(), "others stay");
        assert_eq!(Command::from_id("SaveProject"), Some(Command::SaveProject));
    }

    #[test]
    fn labels_have_categories() {
        for c in Command::ALL {
            assert!(c.label().contains(": "), "{c:?}");
            assert!(!c.short_label().is_empty());
        }
    }
}
