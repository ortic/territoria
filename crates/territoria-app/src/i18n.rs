//! The interface language: English texts are the keys, other languages
//! map them to their own (Swiss spelling for German). Texts without a
//! translation stay English.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::LazyLock;

pub use territoria_config::Language;

// The interface runs on one thread; per thread, tests don't meet.
thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
}

pub fn set_language(language: Language) {
    CURRENT.with(|c| c.set(language));
}

pub fn language() -> Language {
    CURRENT.with(Cell::get)
}

/// `text` in the interface language.
pub fn tr(text: &'static str) -> &'static str {
    match language() {
        Language::English => text,
        Language::German => GERMAN.get(text).copied().unwrap_or(text),
    }
}

static GERMAN: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // Commands (category: name).
        ("File: New Project", "Datei: Neues Projekt"),
        ("File: Open Project…", "Datei: Projekt öffnen…"),
        ("File: Save", "Datei: Speichern"),
        ("File: Save As…", "Datei: Speichern unter…"),
        (
            "File: Templates (new from template, keep as template)…",
            "Datei: Vorlagen (neu aus Vorlage, als Vorlage behalten)…",
        ),
        (
            "File: Add Boundaries (Shapefile, GeoJSON, GeoPackage, KML)…",
            "Datei: Grenzen hinzufügen (Shapefile, GeoJSON, GeoPackage, KML)…",
        ),
        (
            "File: Add Data (CSV, Excel)…",
            "Datei: Daten hinzufügen (CSV, Excel)…",
        ),
        (
            "File: Download Open Boundaries…",
            "Datei: Offene Grenzen herunterladen…",
        ),
        ("File: Settings…", "Datei: Einstellungen…"),
        ("File: Exit", "Datei: Beenden"),
        ("Edit: Undo", "Bearbeiten: Rückgängig"),
        ("Edit: Redo", "Bearbeiten: Wiederholen"),
        (
            "Data: Link Table to Map…",
            "Daten: Tabelle mit Karte verknüpfen…",
        ),
        (
            "Data: New Layer by Merging Areas…",
            "Daten: Neue Ebene aus zusammengelegten Gebieten…",
        ),
        (
            "Data: Replace Boundaries with a Newer Version…",
            "Daten: Grenzen durch neuere Version ersetzen…",
        ),
        ("Data: Calculated Column…", "Daten: Berechnete Spalte…"),
        ("Data: New Hexagon Grid…", "Daten: Neues Sechseckraster…"),
        (
            "Map: Catchment Rings Around Points…",
            "Karte: Einzugsringe um Punkte…",
        ),
        (
            "Map: Drive-Time Zones Around Sites…",
            "Karte: Fahrzeitzonen um Standorte…",
        ),
        (
            "Map: Flow Lines Between Places…",
            "Karte: Flusslinien zwischen Orten…",
        ),
        (
            "Map: Colour Areas by Value…",
            "Karte: Gebiete nach Wert einfärben…",
        ),
        ("Map: New Map", "Karte: Neue Karte"),
        (
            "Map: Export Map (PNG, SVG, PDF)…",
            "Karte: Karte exportieren (PNG, SVG, PDF)…",
        ),
        ("Map: Copy Map as Image", "Karte: Karte als Bild kopieren"),
        (
            "Map: Export as Web Page (zoom, hover)…",
            "Karte: Als Webseite exportieren (Zoom, Infokarten)…",
        ),
        (
            "Map: Export Atlas (a page per region, rep or area)…",
            "Karte: Atlas exportieren (eine Seite pro Region, Vertreter oder Gebiet)…",
        ),
        ("Map: Zoom to Everything", "Karte: Alles zeigen"),
        ("Map: Navigate Tool", "Karte: Werkzeug Navigieren"),
        (
            "Map: Paint Territories Tool",
            "Karte: Werkzeug Gebiete malen",
        ),
        ("Map: Select Tool", "Karte: Werkzeug Auswählen"),
        (
            "Map: Select Areas by List…",
            "Karte: Gebiete per Liste auswählen…",
        ),
        ("Map: Keep This View…", "Karte: Diese Ansicht behalten…"),
        (
            "Territories: New Territory Plan…",
            "Gebiete: Neue Gebietsplanung…",
        ),
        ("Territories: New Territory", "Gebiete: Neues Gebiet"),
        (
            "Territories: Assign Unassigned Areas to Neighbours",
            "Gebiete: Freie Flächen den Nachbarn zuteilen",
        ),
        (
            "Territories: Recolour Territories",
            "Gebiete: Gebiete neu einfärben",
        ),
        (
            "Territories: Balance Territories…",
            "Gebiete: Gebiete ausgleichen…",
        ),
        ("View: Command Palette", "Ansicht: Befehlspalette"),
        (
            "View: Find an Area or Territory",
            "Ansicht: Gebiet oder Fläche suchen",
        ),
        (
            "View: Presentation (full screen, map only)",
            "Ansicht: Präsentation (Vollbild, nur Karte)",
        ),
        ("View: Contents Panel", "Ansicht: Inhalt"),
        ("View: Properties Panel", "Ansicht: Eigenschaften"),
        ("View: Dark Theme", "Ansicht: Dunkles Design"),
        ("View: Light Theme", "Ansicht: Helles Design"),
        ("View: System Theme", "Ansicht: Design wie das System"),
        ("View: Zoom In (UI)", "Ansicht: Vergrössern (Oberfläche)"),
        ("View: Zoom Out (UI)", "Ansicht: Verkleinern (Oberfläche)"),
        (
            "View: Reset Zoom (UI)",
            "Ansicht: Originalgrösse (Oberfläche)",
        ),
        ("Help: Help Topics", "Hilfe: Hilfethemen"),
        ("Help: About Territoria", "Hilfe: Über Territoria"),
        // Menus.
        ("File", "Datei"),
        ("Edit", "Bearbeiten"),
        ("Map", "Karte"),
        ("Territories", "Gebiete"),
        ("View", "Ansicht"),
        ("Help", "Hilfe"),
        ("Open Recent", "Zuletzt geöffnet"),
        ("Export", "Exportieren"),
        // Toolbar.
        ("📂 Open", "📂 Öffnen"),
        ("💾 Save", "💾 Speichern"),
        ("🗺 Add boundaries", "🗺 Grenzen hinzufügen"),
        ("📊 Add data", "📊 Daten hinzufügen"),
        ("🎨 Colour by value", "🎨 Nach Wert einfärben"),
        ("⬣ New territories", "⬣ Neue Gebiete"),
        ("✋ Navigate", "✋ Navigieren"),
        ("📏 Measure", "📏 Messen"),
        ("⬚ Select", "⬚ Auswählen"),
        ("✏ Paint", "✏ Malen"),
        ("Open a project", "Ein Projekt öffnen"),
        ("Save the project", "Das Projekt speichern"),
        (
            "Plan sales or service territories",
            "Verkaufs- oder Servicegebiete planen",
        ),
        (
            "Colour areas, or draw circles sized by a number",
            "Gebiete einfärben oder Kreise nach einer Zahl zeichnen",
        ),
        (
            "🔍 Find an area or territory",
            "🔍 Gebiet oder Fläche suchen",
        ),
        ("All commands (Ctrl+Shift+P)", "Alle Befehle (Ctrl+Shift+P)"),
        // Status bar.
        (
            "Drag to move the map, scroll to zoom.",
            "Ziehen verschiebt die Karte, Scrollen zoomt.",
        ),
        (
            "Drag over areas to assign them. Shift+click picks a territory, Alt+drag erases.",
            "Über Gebiete ziehen teilt sie zu. Shift+Klick wählt ein Gebiet, Alt+Ziehen radiert.",
        ),
        (
            "Click on the map where the territory's site is (Esc cancels).",
            "Auf die Karte klicken, wo der Standort des Gebiets ist (Esc bricht ab).",
        ),
        (
            "Click or drag a rectangle to select, Alt+drag for a lasso, Shift adds. Right-drag moves the map, Esc clears.",
            "Klicken oder ein Rechteck ziehen wählt aus, Alt+Ziehen für ein Lasso, Shift fügt hinzu. Rechts ziehen verschiebt die Karte, Esc hebt auf.",
        ),
        (
            "Click points to measure; right-click takes the last one back, Esc starts over.",
            "Punkte anklicken zum Messen; Rechtsklick nimmt den letzten zurück, Esc beginnt neu.",
        ),
        ("MAP", "KARTE"),
        ("Details of", "Details zu"),
        ("➕ Boundaries", "➕ Grenzen"),
        ("➕ Data", "➕ Daten"),
        ("➕ New plan", "➕ Neue Planung"),
        ("None", "Keine"),
        ("Number of areas", "Anzahl Gebiete"),
        ("📍 Sites", "📍 Standorte"),
        ("Stop", "Stopp"),
        ("unsaved", "nicht gespeichert"),
        ("saved", "gespeichert"),
        // Contents.
        ("DATA", "DATEN"),
        ("TERRITORY PLANS", "GEBIETSPLANUNGEN"),
        ("➕ Add to map", "➕ Zur Karte hinzufügen"),
        ("Boundaries", "Grenzen"),
        ("Data", "Daten"),
        ("New plan", "Neue Planung"),
        ("Background", "Hintergrund"),
        ("Projection", "Projektion"),
        ("Views", "Ansichten"),
        ("None kept", "Keine behalten"),
        ("Open table", "Tabelle öffnen"),
        ("Move up", "Nach oben"),
        ("Move down", "Nach unten"),
        ("Remove", "Entfernen"),
        ("Remove…", "Entfernen…"),
        ("Rename", "Umbenennen"),
        // Properties.
        ("Properties", "Eigenschaften"),
        ("Opacity", "Deckkraft"),
        ("Shown", "Sichtbar"),
        ("At all zoom levels", "In allen Zoomstufen"),
        ("Show as", "Zeigen als"),
        ("Colours", "Farben"),
        ("Circles", "Kreise"),
        ("Heat map", "Heatmap"),
        ("Value", "Wert"),
        ("Per area", "Pro Gebiet"),
        ("Only where", "Nur wo"),
        ("Relative to", "Bezogen auf"),
        ("Segments", "Segmente"),
        ("Classes", "Klassen"),
        ("Smooth gradient", "Stufenloser Verlauf"),
        ("Class sets", "Klassensätze"),
        ("Method", "Methode"),
        ("Limits", "Grenzen"),
        ("Reverse colours", "Farben umkehren"),
        ("Areas without data", "Gebiete ohne Daten"),
        ("Borders", "Ränder"),
        ("Labels", "Beschriftungen"),
        ("Label size", "Schriftgrösse"),
        ("Line width", "Linienbreite"),
        (
            "Own text in the hover card",
            "Eigener Text in der Infokarte",
        ),
        ("Fill", "Füllung"),
        ("Key", "Schlüssel"),
        ("Name", "Name"),
        ("Coordinates", "Koordinaten"),
        ("Area", "Fläche"),
        ("Colour", "Farbe"),
        ("Outline", "Umriss"),
        ("Largest", "Grösster"),
        ("Shape", "Form"),
        ("Sizes", "Grössen"),
        ("Colour by value…", "Nach Wert einfärben…"),
        // Territories panel.
        ("Balance by", "Ausgleichen nach"),
        ("Tolerance ±", "Toleranz ±"),
        ("➕ Territory", "➕ Gebiet"),
        ("Recolour", "Neu einfärben"),
        ("Names", "Namen"),
        ("Sites", "Standorte"),
        ("Regions", "Regionen"),
        ("➕ Region", "➕ Region"),
        ("Balance automatically", "Automatisch ausgleichen"),
        ("Improve current", "Bestehende verbessern"),
        ("Start over", "Neu aufteilen"),
        ("Compact shapes", "Kompakte Formen"),
        ("Few changes", "Wenige Änderungen"),
        ("⚖ Balance", "⚖ Ausgleichen"),
        ("Apply", "Übernehmen"),
        ("Discard", "Verwerfen"),
        ("Fill gaps", "Lücken füllen"),
        ("⧉ Duplicate as scenario", "⧉ Als Szenario duplizieren"),
        ("⇄ Compare with", "⇄ Vergleichen mit"),
        (
            "🚗 Rounds for all territories",
            "🚗 Touren für alle Gebiete",
        ),
        (
            "Drag territories between regions.",
            "Gebiete zwischen Regionen ziehen.",
        ),
        // Selection bar.
        ("Zoom to", "Hinzoomen"),
        ("Focus", "Fokus"),
        ("Unassign", "Freigeben"),
        ("Select in", "Auswählen in"),
        ("Round", "Tour"),
        // Tables.
        ("All rows", "Alle Zeilen"),
        ("Find in all columns", "In allen Spalten suchen"),
        ("ƒ Calculated column…", "ƒ Berechnete Spalte…"),
        ("⊕ Columns from…", "⊕ Spalten aus…"),
        ("Σ Summarise…", "Σ Zusammenfassen…"),
        ("⚖ Score…", "⚖ Punktwert…"),
        ("➕ Row", "➕ Zeile"),
        ("Only the selection", "Nur die Auswahl"),
        // Start page.
        ("1  Add boundaries", "1  Grenzen hinzufügen"),
        ("2  Add your data", "2  Eigene Daten hinzufügen"),
        ("3  Plan territories", "3  Gebiete planen"),
        // Common buttons.
        ("Cancel", "Abbrechen"),
        ("Close", "Schliessen"),
        ("Create", "Erstellen"),
        ("OK", "OK"),
        ("Keep", "Behalten"),
        // Settings.
        ("Theme", "Design"),
        ("Size", "Grösse"),
        ("Autosave", "Automatisch sichern"),
        ("Language", "Sprache"),
        ("Company", "Firma"),
        ("Routing", "Routing"),
        ("Geocoding", "Geokodierung"),
        ("Map tiles", "Kartenkacheln"),
        ("Keyboard shortcuts", "Tastenkürzel"),
        ("Territory colours", "Gebietsfarben"),
        ("Recent projects", "Zuletzt geöffnete Projekte"),
        ("New maps", "Neue Karten"),
        ("Like the system", "Wie das System"),
        ("Light", "Hell"),
        ("Dark", "Dunkel"),
    ])
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_where_translated_english_else() {
        set_language(Language::German);
        assert_eq!(tr("File"), "Datei");
        assert_eq!(tr("Something new"), "Something new");
        // Commands keep their "category: name" form.
        assert!(
            GERMAN
                .iter()
                .filter(|(k, _)| k.contains(": "))
                .all(|(_, v)| v.contains(": "))
        );
        set_language(Language::English);
        assert_eq!(tr("File"), "File");
    }
}
