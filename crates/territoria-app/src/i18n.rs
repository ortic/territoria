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
    tr_str(text)
}

/// Like [`tr`], for texts made at run time.
pub fn tr_str(text: &str) -> &str {
    match language() {
        Language::English => text,
        Language::German => GERMAN.get(text).copied().unwrap_or(text),
    }
}

/// [`tr`] of a text with `{name}` placeholders, filled in after
/// translating so each language can put them where it needs them.
pub fn trf(template: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut text = tr(template).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), &value.to_string());
    }
    text
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
        // Dialog titles.
        ("Add boundaries", "Grenzen hinzufügen"),
        ("Add columns from…", "Spalten hinzufügen aus…"),
        ("Add data", "Daten hinzufügen"),
        ("Catchment rings", "Einzugsringe"),
        ("Download open boundaries", "Offene Grenzen herunterladen"),
        ("Drive-time zones", "Fahrzeitzonen"),
        ("Export map", "Karte exportieren"),
        ("Flow lines", "Flusslinien"),
        ("Map atlas", "Kartenatlas"),
        ("New hexagon grid", "Neues Sechseckraster"),
        (
            "New layer by merging areas",
            "Neue Ebene aus zusammengelegten Gebieten",
        ),
        ("New territory plan", "Neue Gebietsplanung"),
        ("Score", "Punktwert"),
        ("Select areas by list", "Gebiete per Liste auswählen"),
        ("Settings", "Einstellungen"),
        ("Show values on the map", "Werte auf der Karte zeigen"),
        ("Sites from points", "Standorte aus Punkten"),
        ("Summarise", "Zusammenfassen"),
        ("Templates", "Vorlagen"),
        ("Link table to map", "Tabelle mit Karte verknüpfen"),
        // Dialog labels.
        ("Areas", "Gebiete"),
        ("Table", "Tabelle"),
        ("Pause", "Pause"),
        ("Count", "Zählen"),
        ("Columns", "Spalten"),
        ("Around", "Um"),
        ("minutes", "Minuten"),
        ("every", "alle"),
        ("characters", "Zeichen"),
        ("and", "und"),
        ("Zones up to", "Zonen bis"),
        ("Widths follow", "Breiten nach"),
        ("Values", "Werte"),
        ("Town", "Ort"),
        ("Title", "Titel"),
        ("Territory name", "Gebietsname"),
        ("Territory", "Gebiet"),
        ("Sum of areas", "Summe der Gebiete"),
        ("Street", "Strasse"),
        ("Start", "Beginn"),
        ("Sheet", "Blatt"),
        ("Several rows", "Mehrere Zeilen"),
        ("Separator", "Trennzeichen"),
        ("Rows", "Zeilen"),
        ("Result", "Ergebnis"),
        ("Resolution", "Auflösung"),
        ("Radii", "Radien"),
        ("Postcode", "PLZ"),
        ("Points", "Punkte"),
        ("Per", "Pro"),
        ("Page", "Seite"),
        ("Over", "Über"),
        ("Or all in", "Oder alles in"),
        ("Numbers", "Zahlen"),
        ("Merge by", "Zusammenlegen nach"),
        ("Longitude / x", "Länge / x"),
        ("Latitude / y", "Breite / y"),
        (
            "Loading the list of countries…",
            "Länderliste wird geladen…",
        ),
        ("Key there", "Schlüssel dort"),
        ("Key in the table", "Schlüssel in der Tabelle"),
        ("Key here", "Schlüssel hier"),
        ("From", "Von"),
        ("To", "Nach"),
        ("Format", "Format"),
        ("Footer", "Fusszeile"),
        ("Encoding", "Zeichensatz"),
        ("Each as", "Jeder als"),
        ("Country for all", "Land für alle"),
        ("Country", "Land"),
        ("Coordinate system", "Koordinatensystem"),
        ("Coordinate system:", "Koordinatensystem:"),
        ("Compare keys", "Schlüssel vergleichen"),
        ("Column", "Spalte"),
        ("Cell size", "Zellgrösse"),
        ("Built from", "Aufgebaut aus"),
        ("Area key", "Gebietsschlüssel"),
        ("A page per", "Eine Seite pro"),
        ("Rows of", "Zeilen von"),
        // Panels: properties, territories, contents, tables.
        ("of", "von"),
        (
            "Also the zones' outlines as a layer",
            "Auch die Umrisse der Zonen als Ebene",
        ),
        (
            "The areas of each zone merged into one: drive-time zones as shapes, for printing or other analyses",
            "Die Gebiete jeder Zone zu einem zusammengelegt: Fahrzeitzonen als Formen, für den Druck oder weitere Analysen",
        ),
        ("Share with", "Teilen mit"),
        ("Stop sharing", "Nicht mehr teilen"),
        ("Shared with", "Geteilt mit"),
        (
            "The selected areas stay in their territory and are also served by this one: half of their figures count here; shown striped",
            "Die ausgewählten Gebiete bleiben in ihrem Gebiet und werden auch von diesem betreut: die Hälfte ihrer Kennzahlen zählt hier; gestreift gezeigt",
        ),
        ("Territory figures", "Gebietskennzahlen"),
        ("Regions like…", "Regionen wie…"),
        (
            "Each territory joins the area of this layer most of its areas lie in; existing regions are replaced",
            "Jedes Gebiet kommt zur Fläche dieser Ebene, in der die meisten seiner Flächen liegen; bestehende Regionen werden ersetzt",
        ),
        ("Rounds for all territories", "Touren für alle Gebiete"),
        (
            "Each territory's quickest round by road through its customers, from its site. The workload adds the time at each customer to the time on the road.",
            "Die schnellste Tour jedes Gebiets über die Strasse durch seine Kunden, ab seinem Standort. Die Arbeitslast zählt die Zeit bei jedem Kunden zur Zeit unterwegs.",
        ),
        ("Each visit takes", "Jeder Besuch dauert"),
        ("the same for all", "für alle gleich"),
        ("minutes from", "Minuten aus"),
        ("Plan rounds", "Touren planen"),
        (
            "Distances to the sites by road",
            "Distanzen zu den Standorten über die Strasse",
        ),
        (
            "Travel times from the routing service (Settings) instead of straight lines: areas go to the site they reach fastest. Asks the service first.",
            "Fahrzeiten vom Routing-Dienst (Einstellungen) statt Luftlinien: Gebiete gehen zum Standort, den sie am schnellsten erreichen. Fragt zuerst den Dienst.",
        ),
        (
            "⛔ Make this line a barrier",
            "⛔ Diese Linie zur Barriere machen",
        ),
        (
            "Areas aren't neighbours across this line (a ridge, a river without a bridge): balancing and contiguity follow it",
            "Gebiete sind über diese Linie hinweg keine Nachbarn (ein Grat, ein Fluss ohne Brücke): Ausgleich und Zusammenhang richten sich danach",
        ),
        ("barriers", "Barrieren"),
        ("barrier", "Barriere"),
        ("Remove all", "Alle entfernen"),
        (
            "Areas along them are neighbours again",
            "Gebiete entlang davon sind wieder Nachbarn",
        ),
        ("Search help", "Hilfe durchsuchen"),
        ("Overview map", "Übersichtskarte"),
        ("Coverage from the sites", "Abdeckung von den Standorten"),
        ("outside", "ausserhalb"),
        ("areas", "Gebiete"),
        ("area", "Gebiet"),
        ("lines", "Linien"),
        ("line", "Linie"),
        ("points", "Punkte"),
        ("point", "Punkt"),
        (
            "Select a map layer or data in the Contents panel to see its settings.",
            "Eine Kartenebene oder Daten im Inhalt wählen, um ihre Einstellungen zu sehen.",
        ),
        ("▦ Open table", "▦ Tabelle öffnen"),
        ("Linked to", "Verknüpft mit"),
        (
            "Not linked to any areas yet.",
            "Noch mit keinen Gebieten verknüpft.",
        ),
        ("🔗 Link to map…", "🔗 Mit Karte verknüpfen…"),
        ("↻ Reload", "↻ Neu laden"),
        (
            "Read the file again; links, types and calculated columns stay",
            "Die Datei neu lesen; Verknüpfungen, Typen und berechnete Spalten bleiben",
        ),
        (
            "A new column computed from the others with a formula",
            "Eine neue, mit einer Formel aus den anderen berechnete Spalte",
        ),
        (
            "Data is matched to areas by this column",
            "Daten werden über diese Spalte den Gebieten zugeordnet",
        ),
        ("Row number", "Zeilennummer"),
        ("Credit", "Quelle"),
        ("🎨 Colour by value…", "🎨 Nach Wert einfärben…"),
        ("➕ Insert", "➕ Einfügen"),
        (
            "Your own text with values, e.g. the name and the revenue below it",
            "Eigener Text mit Werten, z. B. der Name und darunter der Umsatz",
        ),
        ("All alike", "Alle gleich"),
        ("to", "bis"),
        ("Label template", "Beschriftungsvorlage"),
        (
            "What the card under the mouse shows for this layer",
            "Was die Infokarte unter der Maus für diese Ebene zeigt",
        ),
        (
            "Use these classes, colours and labels here",
            "Diese Klassen, Farben und Beschriftungen hier verwenden",
        ),
        ("Keep these classes as", "Diese Klassen behalten als"),
        ("Name, e.g. Revenue bands", "Name, z. B. Umsatzstufen"),
        ("Round class limits", "Klassengrenzen runden"),
        (
            "Open the map to set zoom levels.",
            "Die Karte öffnen, um Zoomstufen zu setzen.",
        ),
        ("hidden now", "jetzt verborgen"),
        ("The row's own place", "Der eigene Ort der Zeile"),
        ("in", "in"),
        ("Number of rows", "Anzahl Zeilen"),
        ("Widest", "Breiteste"),
        ("Curved", "Gebogen"),
        ("Arrows", "Pfeile"),
        ("⇄ Swap", "⇄ Tauschen"),
        ("Swap the two values", "Die beiden Werte tauschen"),
        ("Weight", "Gewicht"),
        ("Each point counts 1", "Jeder Punkt zählt 1"),
        ("Spread", "Reichweite"),
        ("How far each point reaches", "Wie weit jeder Punkt reicht"),
        ("Remove this value", "Diesen Wert entfernen"),
        ("➕ Add a value", "➕ Wert hinzufügen"),
        ("Chart", "Diagramm"),
        ("Tallest", "Höchster"),
        ("Sized by the total", "Grösse nach der Summe"),
        (
            "Figures within the rings…",
            "Kennzahlen innerhalb der Ringe…",
        ),
        ("One colour", "Eine Farbe"),
        ("Most frequent", "Häufigste"),
        ("Territory borders", "Gebietsgrenzen"),
        ("Region borders", "Regionsgrenzen"),
        (
            "Show area borders inside territories",
            "Gebietsgrenzen innerhalb der Gebiete zeigen",
        ),
        ("Territory names", "Gebietsnamen"),
        ("Open the Territories panel", "Das Gebietsfenster öffnen"),
        (
            "No values to show: check the link to the data.",
            "Keine Werte zu zeigen: die Verknüpfung zu den Daten prüfen.",
        ),
        (
            "Keep this filter as a segment",
            "Diesen Filter als Segment behalten",
        ),
        ("Name, e.g. Key accounts", "Name, z. B. Schlüsselkunden"),
        ("all rows", "alle Zeilen"),
        ("No territory plan yet.", "Noch keine Gebietsplanung."),
        (
            "A plan groups areas into territories: sales regions, service areas, delivery districts.",
            "Eine Planung fasst Gebiete zu Verkaufsgebieten zusammen: Verkaufsregionen, Servicegebiete, Lieferbezirke.",
        ),
        ("⬣ New territory plan…", "⬣ Neue Gebietsplanung…"),
        ("✏ Rename", "✏ Umbenennen"),
        ("▦ Open as table", "▦ Als Tabelle öffnen"),
        ("Export (Excel)…", "Export (Excel)…"),
        (
            "Excel, a sheet per territory…",
            "Excel, ein Blatt pro Gebiet…",
        ),
        ("Report (PDF)…", "Bericht (PDF)…"),
        (
            "An overview and a page per territory",
            "Eine Übersicht und eine Seite pro Gebiet",
        ),
        ("🗑 Delete plan…", "🗑 Planung löschen…"),
        ("Zoom to territory", "Zum Gebiet zoomen"),
        ("Paint with it", "Damit malen"),
        ("Unlock", "Entsperren"),
        ("Place site on the map", "Standort auf der Karte setzen"),
        ("Remove site", "Standort entfernen"),
        ("Split into", "Aufteilen in"),
        ("Merge into", "Zusammenlegen mit"),
        ("Delete", "Löschen"),
        (
            "Add a territory and paint it (Ctrl+Shift+T)",
            "Ein Gebiet hinzufügen und malen (Ctrl+Shift+T)",
        ),
        (
            "Delete the active territory; its areas become unassigned",
            "Das aktive Gebiet löschen; seine Flächen werden frei",
        ),
        (
            "Assign areas by dragging over them (B)",
            "Flächen durch Darüberziehen zuteilen (B)",
        ),
        (
            "Give unassigned areas to the neighbouring territory they share most border with",
            "Freie Flächen dem Nachbargebiet mit der längsten gemeinsamen Grenze geben",
        ),
        (
            "Different colours for neighbouring territories",
            "Verschiedene Farben für benachbarte Gebiete",
        ),
        (
            "Name after the largest place",
            "Nach dem grössten Ort benennen",
        ),
        (
            "Renumber north to south",
            "Von Nord nach Süd neu nummerieren",
        ),
        ("Sites at centres", "Standorte in der Mitte"),
        (
            "Move all sites to the centres",
            "Alle Standorte in die Mitte setzen",
        ),
        ("Sites from points…", "Standorte aus Punkten…"),
        ("Remove all sites", "Alle Standorte entfernen"),
        ("Show", "Zeigen"),
        (
            "Select these areas on the map",
            "Diese Gebiete auf der Karte auswählen",
        ),
        (
            "Move as few areas as needed",
            "So wenige Flächen wie nötig verschieben",
        ),
        (
            "New territories from scratch (locked ones stay)",
            "Neue Gebiete von Grund auf (gesperrte bleiben)",
        ),
        ("Balancing…", "Ausgleichen…"),
        ("Proposal", "Vorschlag"),
        (
            "Stopped early: the result may improve with another run.",
            "Früh beendet: ein weiterer Lauf kann das Ergebnis verbessern.",
        ),
        (
            "Group territories into regions: sales regions above the territories, with their own borders and figures.",
            "Gebiete zu Regionen gruppieren: Verkaufsregionen über den Gebieten, mit eigenen Grenzen und Kennzahlen.",
        ),
        (
            "Remove the region (its territories stay)",
            "Die Region entfernen (ihre Gebiete bleiben)",
        ),
        ("Group into", "Gruppieren in"),
        ("regions", "Regionen"),
        ("Code", "Code"),
        ("Region", "Region"),
        ("Locked", "Gesperrt"),
        ("keeps its areas", "behält seine Flächen"),
        ("Site", "Standort"),
        ("none", "keiner"),
        ("📍 Place on the map", "📍 Auf der Karte setzen"),
        ("Notes", "Notizen"),
        ("Compared with", "Verglichen mit"),
        ("End the comparison", "Den Vergleich beenden"),
        (
            "No changes yet. Paint or balance this plan to try changes.",
            "Noch keine Änderungen. Diese Planung malen oder ausgleichen, um Änderungen zu versuchen.",
        ),
        ("Mark them on the map", "Auf der Karte markieren"),
        ("Export changes (Excel)…", "Änderungen exportieren (Excel)…"),
        ("Drag onto another region", "Auf eine andere Region ziehen"),
        ("Show or hide", "Zeigen oder verbergen"),
        ("Remove from map", "Von der Karte entfernen"),
        ("Total", "Summe"),
        ("fewer", "weniger"),
        ("more", "mehr"),
        (
            "🎨 Colour areas by value…",
            "🎨 Gebiete nach Wert einfärben…",
        ),
        ("Show on map", "Auf der Karte zeigen"),
        (
            "Merge areas into a new layer…",
            "Gebiete zu einer neuen Ebene zusammenlegen…",
        ),
        (
            "Replace with a newer version…",
            "Durch eine neuere Version ersetzen…",
        ),
        ("Link to map…", "Mit Karte verknüpfen…"),
        ("↻ Reload from file", "↻ Aus der Datei neu laden"),
        ("Custom", "Eigene"),
        (
            "Background maps need the Web Mercator projection.",
            "Hintergrundkarten brauchen die Web-Mercator-Projektion.",
        ),
        ("Go to this view", "Zu dieser Ansicht"),
        ("Forget this view", "Diese Ansicht vergessen"),
        ("➕ Keep this view", "➕ Diese Ansicht behalten"),
        ("Equal area (local)", "Flächentreu (lokal)"),
        (
            "Background maps show in Web Mercator only.",
            "Hintergrundkarten erscheinen nur in Web Mercator.",
        ),
        (
            "Fade everything else, e.g. to present one territory or region",
            "Alles andere abblenden, z. B. um ein Gebiet oder eine Region zu zeigen",
        ),
        ("Clear the selection (Esc)", "Die Auswahl aufheben (Esc)"),
        ("This layer was removed.", "Diese Ebene wurde entfernt."),
        ("This table was removed.", "Diese Tabelle wurde entfernt."),
        ("New segment…", "Neues Segment…"),
        (
            "Rows of the areas selected on the map",
            "Zeilen der auf der Karte ausgewählten Gebiete",
        ),
        ("Excel…", "Excel…"),
        ("CSV…", "CSV…"),
        ("✏ Edit formula…", "✏ Formel bearbeiten…"),
        ("🗑 Delete column", "🗑 Spalte löschen"),
        ("🗑 Delete row", "🗑 Zeile löschen"),
        // Dialog explanations.
        (
            "A column computed from the others, e.g. revenue per customer: [Revenue] / [Customers]. It updates when you edit the formula.",
            "Eine aus den anderen berechnete Spalte, z. B. Umsatz pro Kunde: [Revenue] / [Customers]. Sie wird nachgeführt, wenn Sie die Formel ändern.",
        ),
        (
            "A new table with one row per group: e.g. sales per rep, customers per segment.",
            "Eine neue Tabelle mit einer Zeile pro Gruppe: z. B. Umsatz pro Vertreter, Kunden pro Segment.",
        ),
        (
            "Administrative areas of about 230 countries from geoBoundaries (William & Mary geoLab). Each set has its own licence; the map credits it.",
            "Verwaltungsgebiete von rund 230 Ländern aus geoBoundaries (William & Mary geoLab). Jeder Datensatz hat seine eigene Lizenz; die Karte nennt sie.",
        ),
        (
            "Areas with the same value become one area: municipalities into districts, postcodes into postcode regions (by their first digits), sales areas into regions.",
            "Gebiete mit gleichem Wert werden zu einem: Gemeinden zu Bezirken, PLZ zu PLZ-Regionen (nach den ersten Ziffern), Verkaufsgebiete zu Regionen.",
        ),
        (
            "Background maps aren't part of exports yet.",
            "Hintergrundkarten sind noch nicht Teil der Exporte.",
        ),
        (
            "Check the column types: codes such as postcodes should be “Key / code” so leading zeros survive.",
            "Spaltentypen prüfen: Codes wie Postleitzahlen sollten «Schlüssel / Code» sein, damit führende Nullen erhalten bleiben.",
        ),
        (
            "Choose the areas and the table's key column.",
            "Gebiete und die Schlüsselspalte der Tabelle wählen.",
        ),
        (
            "Equal hexagons as areas: for density maps, and as units for territories where no boundaries fit.",
            "Gleich grosse Sechsecke als Gebiete: für Dichtekarten und als Einheiten für Gebiete, wo keine Grenzen passen.",
        ),
        (
            "Keep this project as a template",
            "Dieses Projekt als Vorlage behalten",
        ),
        (
            "Keys of areas, separated by commas, semicolons or lines. Ranges like 8000-8099 and wildcards like 81* work too.",
            "Schlüssel der Gebiete, durch Kommas, Strichpunkte oder Zeilen getrennt. Bereiche wie 8000-8099 und Platzhalter wie 81* gehen auch.",
        ),
        (
            "Many candidates: choosing may take a moment.",
            "Viele Kandidaten: die Wahl kann einen Moment dauern.",
        ),
        (
            "Neighbouring areas don't share their borders exactly. Territories still work; their borders may look busier.",
            "Benachbarte Gebiete teilen ihre Grenzen nicht genau. Gebiete funktionieren trotzdem; ihre Grenzen wirken vielleicht unruhiger.",
        ),
        (
            "New columns with values from the map or another table, as they are now (add them again after changes).",
            "Neue Spalten mit Werten aus der Karte oder einer anderen Tabelle, so wie sie jetzt sind (nach Änderungen neu hinzufügen).",
        ),
        (
            "No templates yet: keep this project as one below.",
            "Noch keine Vorlagen: dieses Projekt unten als eine behalten.",
        ),
        (
            "Numbers in the attributes are added up; an “Areas” column counts what was merged.",
            "Zahlen in den Attributen werden addiert; eine Spalte «Areas» zählt, was zusammengelegt wurde.",
        ),
        (
            "One figure from several, weighted: e.g. 60 % revenue and 40 % customers. The weights are made to add up to 1.",
            "Eine Kennzahl aus mehreren, gewichtet: z. B. 60 % Umsatz und 40 % Kunden. Die Gewichte werden auf 1 gebracht.",
        ),
        (
            "Pick the values that make up each chart, e.g. sales per product line.",
            "Die Werte jedes Diagramms wählen, z. B. Umsatz pro Produktlinie.",
        ),
        (
            "Rings around branches, reps or sites, and what lies within them: customers, their sales, the areas' values.",
            "Ringe um Filialen, Vertreter oder Standorte und was darin liegt: Kunden, ihr Umsatz, die Werte der Gebiete.",
        ),
        (
            "Rows of the table belong to areas by a key, such as a postcode or a municipality number.",
            "Zeilen der Tabelle gehören über einen Schlüssel zu Gebieten, etwa eine Postleitzahl oder Gemeindenummer.",
        ),
        (
            "The area's key always; tick more:",
            "Immer den Schlüssel des Gebiets; weitere ankreuzen:",
        ),
        (
            "When several rows belong to one area, their values are combined.",
            "Gehören mehrere Zeilen zu einem Gebiet, werden ihre Werte zusammengefasst.",
        ),
        (
            "Exports what the map shows now, with title, legend, scale bar and credits, as on screen or on a printed page.",
            "Exportiert, was die Karte jetzt zeigt, mit Titel, Legende, Massstab und Quellen, wie am Bildschirm oder auf einer Druckseite.",
        ),
        (
            "A PDF of this map with a page per region, rep or area: each zoomed to its areas, the others veiled, with legend and scale bar.",
            "Ein PDF dieser Karte mit einer Seite pro Region, Vertreter oder Gebiet: jede auf ihre Gebiete gezoomt, die übrigen abgeblendet, mit Legende und Massstab.",
        ),
        (
            "Lines from where each row starts to where it goes, as wide as its value: customers to their branch, deliveries, commuters.",
            "Linien von wo jede Zeile beginnt bis wohin sie geht, so breit wie ihr Wert: Kunden zu ihrer Filiale, Lieferungen, Pendler.",
        ),
        (
            "A template is a project to start from: its boundaries, tables, maps with their colours and labels, plans and settings.",
            "Eine Vorlage ist ein Projekt zum Beginnen: seine Grenzen, Tabellen, Karten mit Farben und Beschriftungen, Planungen und Einstellungen.",
        ),
        (
            "Each area coloured by the minutes by road to the nearest branch, rep or site: who is within 15, 30, 45 minutes. Travel times come from the routing service in Settings.",
            "Jedes Gebiet nach den Fahrminuten zur nächsten Filiale, zum nächsten Vertreter oder Standort eingefärbt: wer liegt in 15, 30, 45 Minuten. Die Fahrzeiten kommen vom Routing-Dienst in den Einstellungen.",
        ),
        ("New project from", "Neues Projekt aus"),
        // Dialog options and buttons.
        (
            "Another table, by key",
            "Eine andere Tabelle, über den Schlüssel",
        ),
        ("Circles sized by value", "Kreise nach Wert"),
        ("Colour the areas by", "Gebiete einfärben nach"),
        (
            "First row has column names",
            "Erste Zeile enthält Spaltennamen",
        ),
        (
            "Name the territories after them",
            "Gebiete nach ihnen benennen",
        ),
        ("Legend", "Legende"),
        ("Scale bar", "Massstab"),
        ("Two values at once", "Zwei Werte zugleich"),
        ("Pie charts", "Kuchendiagramme"),
        ("Bar charts", "Balkendiagramme"),
        ("Stacked bars", "Gestapelte Balken"),
        ("Add columns", "Spalten hinzufügen"),
        ("Continue", "Weiter"),
        ("Create PDF…", "PDF erstellen…"),
        ("Create table", "Tabelle erstellen"),
        ("Download", "Herunterladen"),
        ("Draw rings", "Ringe zeichnen"),
        ("Link", "Verknüpfen"),
        ("Place sites", "Standorte setzen"),
        ("Select", "Auswählen"),
        ("Export…", "Exportieren…"),
        ("PNG image", "PNG-Bild"),
        ("As on screen", "Wie am Bildschirm"),
        ("A4 landscape", "A4 quer"),
        ("A4 portrait", "A4 hoch"),
        ("A3 landscape", "A3 quer"),
        ("A3 portrait", "A3 hoch"),
        ("Screen", "Bildschirm"),
        ("Sharp (2×)", "Scharf (2×)"),
        ("Print (4×)", "Druck (4×)"),
        (
            "With empty territories to paint",
            "Mit leeren Gebieten zum Malen",
        ),
        (
            "From an assignment table (area → territory)",
            "Aus einer Zuordnungstabelle (Gebiet → Gebiet)",
        ),
        (
            "Rows have addresses: find their coordinates (geocoding)",
            "Zeilen haben Adressen: Koordinaten suchen (Geokodierung)",
        ),
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
        (
            "Regions ({n}) · {levels} levels",
            "Regionen ({n}) · {levels} Ebenen",
        ),
        ("➕ Level above: {name}", "➕ Ebene darüber: {name}"),
        (
            "Neighbouring regions of the highest level are grouped in turn, e.g. regions into divisions; any number of levels",
            "Benachbarte Regionen der obersten Ebene werden wiederum gruppiert, z. B. Regionen zu Divisionen; beliebig viele Ebenen",
        ),
        ("Level {level}", "Ebene {level}"),
        (
            "What this level's regions are called, in tables and exports",
            "Wie die Regionen dieser Ebene heissen, in Tabellen und Exporten",
        ),
        (
            "Remove this level (and any above); the regions below stay",
            "Diese Ebene (und alle darüber) entfernen; die Regionen darunter bleiben",
        ),
        ("{n} in no {level}", "{n} in keiner {level}"),
        ("➕ {name}", "➕ {name}"),
        (
            "Neighbouring regions of the level below are grouped, of about equal size by the measure; this level is replaced",
            "Benachbarte Regionen der Ebene darunter werden gruppiert, etwa gleich gross nach der Kennzahl; diese Ebene wird ersetzt",
        ),
        ("{name} like…", "{name} wie…"),
        (
            "Drag regions between the groups above them.",
            "Regionen zwischen den Gruppen darüber ziehen.",
        ),
        ("Rename level", "Ebene umbenennen"),
        ("Remove level", "Ebene entfernen"),
        ("Division", "Division"),
        ("Zone", "Zone"),
        (
            "Couldn't read an API key from the system keychain ({problem}).",
            "Ein API-Schlüssel konnte nicht aus dem Schlüsselbund des Systems gelesen werden ({problem}).",
        ),
        (
            "Couldn't keep an API key in the system keychain ({problem}); it works until Territoria closes.",
            "Ein API-Schlüssel konnte nicht im Schlüsselbund des Systems behalten werden ({problem}); er gilt, bis Territoria geschlossen wird.",
        ),
        ("Forget", "Vergessen"),
        ("Service address", "Adresse des Dienstes"),
        (
            "Your e-mail (public services ask for it)",
            "Ihre E-Mail (öffentliche Dienste verlangen sie)",
        ),
        ("OSRM server address", "Adresse des OSRM-Servers"),
        ("{mb} MB stored", "{mb} MB gespeichert"),
        ("Clear", "Leeren"),
        (
            "For tile servers that need one (MapTiler, Thunderforest, …): put {key} in the tile address where the key goes",
            "Für Kachelserver, die einen brauchen (MapTiler, Thunderforest, …): {key} in der Kacheladresse dort einsetzen, wo der Schlüssel hingehört",
        ),
        (
            "API key, if the service needs one",
            "API-Schlüssel, falls der Dienst einen braucht",
        ),
        (
            "Kept in the system keychain, not in the settings file. Sent in place of {key} in the address, else as the key parameter",
            "Im Schlüsselbund des Systems behalten, nicht in der Einstellungsdatei. Wird anstelle von {key} in der Adresse gesendet, sonst als Parameter key",
        ),
        ("and per", "und pro"),
        (
            "Within each, a page per value of a second column, e.g. per region and customer segment",
            "Darin eine Seite pro Wert einer zweiten Spalte, z. B. pro Region und Kundensegment",
        ),
        ("Files", "Dateien"),
        ("One PDF", "Ein PDF"),
        ("A PDF per page", "Ein PDF pro Seite"),
        (
            "Named after the file and the page, e.g. “Atlas – Region 01.pdf”, to hand out one by one",
            "Benannt nach Datei und Seite, z. B. «Atlas – Region 01.pdf», zum einzeln Weitergeben",
        ),
        (
            "and an Excel workbook with each page's areas",
            "und eine Excel-Mappe mit den Flächen jeder Seite",
        ),
        (
            "A sheet per page with its areas and their columns, beside the PDF",
            "Ein Blatt pro Seite mit seinen Flächen und deren Spalten, neben dem PDF",
        ),
        ("{n} pages", "{n} Seiten"),
        ("{text}: at most 300", "{text}: höchstens 300"),
        ("Version {version}", "Version {version}"),
        (
            "{column}: drop on the map or its layer list",
            "{column}: auf die Karte oder ihre Ebenenliste ziehen",
        ),
        ("… {n} more", "… {n} weitere"),
        ("within {km} km", "innerhalb {km} km"),
        (
            "⤵ Drop here to show {column} on the map",
            "⤵ Hier ablegen, um {column} auf der Karte zu zeigen",
        ),
        (
            "{n} rows have no key and were skipped.",
            "{n} Zeilen haben keinen Schlüssel und wurden übersprungen.",
        ),
        ("Not found: {keys}", "Nicht gefunden: {keys}"),
        ("{n} rows", "{n} Zeilen"),
        (
            "{n} of {rows} rows have coordinates",
            "{n} von {rows} Zeilen haben Koordinaten",
        ),
        ("{n} territories", "{n} Gebiete"),
        (
            "{n} areas are listed for more than one territory; the first wins.",
            "{n} Flächen sind für mehr als ein Gebiet aufgeführt; das erste gilt.",
        ),
        (
            "{areas} areas become {groups}",
            "{areas} Flächen werden zu {groups}",
        ),
        (
            "The list couldn't be loaded: {e}",
            "Die Liste konnte nicht geladen werden: {e}",
        ),
        ("→ {kind} column", "→ Spalte vom Typ {kind}"),
        (
            "There is already a column “{name}”.",
            "Es gibt schon eine Spalte «{name}».",
        ),
        ("{n} areas match.", "{n} Flächen passen."),
        (
            "{rows} rows · {columns} columns",
            "{rows} Zeilen · {columns} Spalten",
        ),
        (
            "{layer} by “{column}” ({mode})",
            "{layer} über «{column}» ({mode})",
        ),
        (
            "Rows of {table} · {n} lines",
            "Zeilen von {table} · {n} Linien",
        ),
        ("Around the {centres}", "Um die {centres}"),
        ("{n} areas", "{n} Flächen"),
        ("{n} of {rows} rows", "{n} von {rows} Zeilen"),
        ("{n} {noun} selected", "{n} {noun} ausgewählt"),
        (
            "{n} territories · click a heading to sort",
            "{n} Gebiete · Klick auf eine Überschrift sortiert",
        ),
        ("{n} areas unassigned", "{n} Flächen nicht zugeteilt"),
        ("{n} in pieces", "{n} in Teilen"),
        (
            "Largest deviation {before} → {after}",
            "Grösste Abweichung {before} → {after}",
        ),
        (
            "Outside tolerance {before} → {after}",
            "Ausserhalb der Toleranz {before} → {after}",
        ),
        (
            "Border length {before} → {after} km",
            "Grenzlänge {before} → {after} km",
        ),
        (
            "{n} territories in no region",
            "{n} Gebiete in keiner Region",
        ),
        (
            "{n} areas change territory",
            "{n} Flächen wechseln das Gebiet",
        ),
        ("{n} territories unchanged", "{n} Gebiete unverändert"),
        ("Adopt in “{plan}”", "In «{plan}» übernehmen"),
        ("This plan was removed.", "Diese Planung wurde entfernt."),
        ("Read again", "Nochmals lesen"),
        ("Only the first", "Nur die ersten"),
        ("Try again", "Nochmals versuchen"),
        ("Choose…", "Wählen…"),
        ("Nothing", "Nichts"),
        ("share of the total", "Anteil am Ganzen"),
        ("rank (0–1)", "Rang (0–1)"),
        ("as it is", "wie er ist"),
        ("names a feature of", "nennt ein Objekt von"),
        ("Assignment (CSV)…", "Zuteilung (CSV)…"),
        ("Territory outlines (GeoJSON)…", "Gebietsumrisse (GeoJSON)…"),
        ("GeoJSON (with shapes)…", "GeoJSON (mit Formen)…"),
        ("📂 Open a project…", "📂 Projekt öffnen…"),
        (
            "Geomarketing and territory planning.",
            "Geomarketing und Gebietsplanung.",
        ),
        ("This map was removed.", "Diese Karte wurde entfernt."),
        // Messages.
        (
            "{file} isn't a PNG, JPEG or SVG image.",
            "{file} ist kein PNG-, JPEG- oder SVG-Bild.",
        ),
        (
            "Territoria can't open “{ext}” files.",
            "Territoria kann keine «{ext}»-Dateien öffnen.",
        ),
        (
            "This table didn't come from a file.",
            "Diese Tabelle stammt nicht aus einer Datei.",
        ),
        ("{file} is no longer there.", "{file} ist nicht mehr da."),
        (
            "The file has no {kind} to replace “{name}” with.",
            "Die Datei enthält keine {kind}, um «{name}» zu ersetzen.",
        ),
        (
            "Undo brings the old ones back.",
            "Rückgängig bringt die alten zurück.",
        ),
        (
            "Couldn't read {file}: {e}",
            "{file} konnte nicht gelesen werden: {e}",
        ),
        (
            "New project from the template {name}.",
            "Neues Projekt aus der Vorlage {name}.",
        ),
        (
            "Couldn't open the template {name}: {e}",
            "Die Vorlage {name} konnte nicht geöffnet werden: {e}",
        ),
        (
            "Kept as the template {name}.",
            "Als Vorlage {name} behalten.",
        ),
        (
            "Couldn't keep the template {name}: {e}",
            "Die Vorlage {name} konnte nicht behalten werden: {e}",
        ),
        (
            "Couldn't open {file}: {e}",
            "{file} konnte nicht geöffnet werden: {e}",
        ),
        ("Saved {file}", "{file} gespeichert"),
        (
            "Couldn't save {file}: {e}",
            "{file} konnte nicht gespeichert werden: {e}",
        ),
        (
            "Couldn't download {country} {level}: {e}",
            "{country} {level} konnte nicht heruntergeladen werden: {e}",
        ),
        (
            "Couldn't get the travel times: {e}",
            "Die Fahrzeiten konnten nicht ermittelt werden: {e}",
        ),
        (
            "Couldn't plan the round: {e}",
            "Die Tour konnte nicht geplant werden: {e}",
        ),
        ("{n} columns added.", "{n} Spalten hinzugefügt."),
        (
            "Couldn't add the columns: {e}",
            "Die Spalten konnten nicht hinzugefügt werden: {e}",
        ),
        ("Wrote {file}", "{file} geschrieben"),
        (
            "Couldn't write {file}: {e}",
            "{file} konnte nicht geschrieben werden: {e}",
        ),
        (
            "Stopped: {found} of {rows} addresses found so far. Add the table again to go on; found ones are remembered.",
            "Angehalten: bisher {found} von {rows} Adressen gefunden. Die Tabelle nochmals hinzufügen, um weiterzumachen; gefundene bleiben gespeichert.",
        ),
        (
            "{found} of {rows} addresses found ({cached} remembered from before). The Geocoding column tells how precisely.",
            "{found} von {rows} Adressen gefunden ({cached} von früher gespeichert). Die Spalte Geokodierung sagt, wie genau.",
        ),
        (
            "Couldn't find the addresses: {e}",
            "Die Adressen konnten nicht gefunden werden: {e}",
        ),
        ("Reload table", "Tabelle neu laden"),
        (
            "{n} links lost their key column: link the table again.",
            "{n} Verknüpfungen haben ihre Schlüsselspalte verloren: die Tabelle neu verknüpfen.",
        ),
        (
            "Couldn't reload the table: {e}",
            "Die Tabelle konnte nicht neu geladen werden: {e}",
        ),
        (
            "The map is on the clipboard: paste it into a mail or slide.",
            "Die Karte ist in der Zwischenablage: in eine Mail oder Folie einfügen.",
        ),
        (
            "Couldn't copy the map: {e}",
            "Die Karte konnte nicht kopiert werden: {e}",
        ),
        ("Balancing failed: {e}", "Ausgleichen fehlgeschlagen: {e}"),
        ("Nothing to add.", "Nichts hinzuzufügen."),
        (
            "Some of the selected points have no place.",
            "Einige der gewählten Punkte haben keinen Ort.",
        ),
        (
            "There are no places to travel to.",
            "Es gibt keine Orte, die angefahren werden können.",
        ),
        (
            "Rounds need a plan on points, e.g. customers.",
            "Touren brauchen eine Planung auf Punkten, z. B. Kunden.",
        ),
        (
            "{n} territories have more than 100 stops and get no round.",
            "{n} Gebiete haben mehr als 100 Halte und erhalten keine Tour.",
        ),
        (
            "No round could be planned: is the routing service in Settings reachable?",
            "Es konnte keine Tour geplant werden: Ist der Routing-Dienst aus den Einstellungen erreichbar?",
        ),
        (
            "Rounds planned: {km} km, {h} h {min} min in all{failed}",
            "Touren geplant: insgesamt {km} km, {h} h {min} min{failed}",
        ),
        ("; {n} territories failed", "; {n} Gebiete fehlgeschlagen"),
        ("Round planned: {summary}.", "Tour geplant: {summary}."),
        (
            "{name} has no points to travel to.",
            "{name} hat keine Punkte zum Anfahren.",
        ),
        (
            "{n} sites chosen. Balance → Start over draws territories around them.",
            "{n} Standorte gewählt. Ausgleichen → Neu aufteilen zieht Gebiete um sie.",
        ),
        (
            "{n} territories got a site.",
            "{n} Gebiete haben einen Standort erhalten.",
        ),
        (
            "Choose the column with the coordinates.",
            "Die Spalte mit den Koordinaten wählen.",
        ),
        (
            "Choose the coordinate columns.",
            "Die Koordinatenspalten wählen.",
        ),
        (
            "Couldn't convert the coordinates: {e}",
            "Die Koordinaten konnten nicht umgerechnet werden: {e}",
        ),
        (
            "{missing} of {rows} rows have no usable coordinates and aren't on the map.",
            "{missing} von {rows} Zeilen haben keine brauchbaren Koordinaten und sind nicht auf der Karte.",
        ),
        (
            "Added {name}. Add boundaries to put it on a map.",
            "{name} hinzugefügt. Grenzen hinzufügen, um es auf eine Karte zu bringen.",
        ),
        (
            "Link the table to boundaries first to show {column} on the map.",
            "Zuerst die Tabelle mit Grenzen verknüpfen, um {column} auf der Karte zu zeigen.",
        ),
        (
            "{column} can't be shown on the map.",
            "{column} kann nicht auf der Karte gezeigt werden.",
        ),
        (
            "Paint territories: drag over areas. Shift+click picks a territory, Alt+drag erases.",
            "Gebiete malen: über Flächen ziehen. Umschalt+Klick wählt ein Gebiet, Alt+Ziehen radiert.",
        ),
        (
            "These calculated columns no longer work and keep their last values: {columns}. Edit their formulas to fix them.",
            "Diese berechneten Spalten funktionieren nicht mehr und behalten ihre letzten Werte: {columns}. Ihre Formeln bearbeiten, um sie zu reparieren.",
        ),
        (
            "Place the territories' sites first (Sites menu).",
            "Zuerst die Standorte der Gebiete setzen (Menü Standorte).",
        ),
        (
            "“{name}”: the sites cover every area.",
            "«{name}»: Die Standorte decken alle Flächen ab.",
        ),
        (
            "“{name}”: {n} areas are beyond what the sites can take (their target +{pct} %).",
            "«{name}»: {n} Flächen liegen über dem, was die Standorte aufnehmen können (ihr Ziel +{pct} %).",
        ),
        (
            "“{name}” is a copy to try changes on. The Territories panel compares it with the original.",
            "«{name}» ist eine Kopie zum Ausprobieren. Der Bereich Gebiete vergleicht sie mit dem Original.",
        ),
        ("Undid: {label}", "Rückgängig: {label}"),
        ("Nothing to undo", "Nichts rückgängig zu machen"),
        ("Redid: {label}", "Wiederholt: {label}"),
        ("Nothing to redo", "Nichts zu wiederholen"),
        (
            "Add a table first: File → Add Data.",
            "Zuerst eine Tabelle hinzufügen: Datei → Daten hinzufügen.",
        ),
        (
            "There's no map to export yet.",
            "Es gibt noch keine Karte zum Exportieren.",
        ),
        (
            "Presentation: F11 or Esc to leave.",
            "Präsentation: F11 oder Esc zum Verlassen.",
        ),
        ("Keep view", "Ansicht behalten"),
        ("Show a map first.", "Zuerst eine Karte zeigen."),
        (
            "Add points (branches, reps) or place territory sites first.",
            "Zuerst Punkte (Filialen, Aussendienst) hinzufügen oder Standorte der Gebiete setzen.",
        ),
        (
            "The map needs areas for an atlas.",
            "Die Karte braucht Flächen für einen Atlas.",
        ),
        ("There's no map yet.", "Es gibt noch keine Karte."),
        (
            "Add data first: points or a table whose rows name where they start and go.",
            "Zuerst Daten hinzufügen: Punkte oder eine Tabelle, deren Zeilen Start und Ziel nennen.",
        ),
        (
            "Add points or boundaries first.",
            "Zuerst Punkte oder Grenzen hinzufügen.",
        ),
        (
            "Add data or boundaries first.",
            "Zuerst Daten oder Grenzen hinzufügen.",
        ),
        ("Add boundaries first.", "Zuerst Grenzen hinzufügen."),
        (
            "Create a territory plan first: Territories → New Territory Plan.",
            "Zuerst eine Gebietsplanung erstellen: Gebiete → Neue Gebietsplanung.",
        ),
        (
            "Add boundaries first: the areas territories are made of.",
            "Zuerst Grenzen hinzufügen: die Flächen, aus denen Gebiete bestehen.",
        ),
        ("New territory", "Neues Gebiet"),
        ("Assign unassigned areas", "Freie Flächen zuteilen"),
        (
            "Assigned {n} areas to their neighbours' territories.",
            "{n} Flächen den Gebieten ihrer Nachbarn zugeteilt.",
        ),
        ("Recolour territories", "Gebiete neu einfärben"),
        (
            "The plan needs territories and areas to balance.",
            "Die Planung braucht Gebiete und Flächen zum Ausgleichen.",
        ),
        (
            "Kept the classes as {name}.",
            "Klassen als {name} behalten.",
        ),
        ("Balance territories", "Gebiete ausgleichen"),
        ("Moved {n} areas.", "{n} Flächen verschoben."),
        ("Edit cell", "Zelle bearbeiten"),
        (
            "“{text}” isn't a {kind} for {column}.",
            "«{text}» ist kein Wert vom Typ {kind} für {column}.",
        ),
        ("Add row", "Zeile hinzufügen"),
        ("Delete row", "Zeile löschen"),
        ("Change projection", "Projektion ändern"),
        (
            "This row isn't on the map: its key matched no area.",
            "Diese Zeile ist nicht auf der Karte: Ihr Schlüssel passt zu keiner Fläche.",
        ),
        (
            "Link this table to areas to see its rows on the map.",
            "Diese Tabelle mit Flächen verknüpfen, um ihre Zeilen auf der Karte zu sehen.",
        ),
        (
            "{changed} of {n} areas changed; the others are locked or already there.",
            "{changed} von {n} Flächen geändert; die übrigen sind gesperrt oder schon dort.",
        ),
        ("Add barrier", "Barriere hinzufügen"),
        (
            "Barrier added: {n} pairs of areas are no longer neighbours.",
            "Barriere hinzugefügt: {n} Flächenpaare sind keine Nachbarn mehr.",
        ),
        (
            "Nothing in {layer} is {relation}.",
            "Nichts in {layer} ist {relation}.",
        ),
        ("{n} of {layer} selected", "{n} von {layer} ausgewählt"),
        ("Remove layer", "Ebene entfernen"),
        ("Delete plan", "Planung löschen"),
        ("Remove table", "Tabelle entfernen"),
        (
            "To paste a table, copy cells with a header row from a spreadsheet.",
            "Um eine Tabelle einzufügen, Zellen mit Kopfzeile aus einer Tabellenkalkulation kopieren.",
        ),
        (
            "Couldn't keep the pasted table: {e}",
            "Die eingefügte Tabelle konnte nicht behalten werden: {e}",
        ),
        // Hover hints.
        (
            "Also has a sheet for each linked table and point layer, with each row's territory",
            "Mit einem Blatt für jede verknüpfte Tabelle und Punktebene, mit dem Gebiet jeder Zeile",
        ),
        (
            "An overview and a page per territory with its map, figures and largest areas",
            "Eine Übersicht und eine Seite pro Gebiet mit Karte, Kennzahlen und grössten Flächen",
        ),
        (
            "Drag to move, scroll to zoom, click an area to inspect it (V)",
            "Ziehen verschiebt, Scrollen zoomt, Klick auf eine Fläche zeigt sie an (V)",
        ),
        (
            "Click points to measure the distance along them; right-click takes the last one back, Esc starts over",
            "Punkte anklicken, um die Strecke entlang zu messen; Rechtsklick nimmt den letzten zurück, Esc beginnt neu",
        ),
        (
            "Click or drag a rectangle over areas, Alt+drag for a lasso, Shift adds. Then assign them in one go (S)",
            "Klicken oder ein Rechteck über Flächen ziehen, Alt+Ziehen für ein Lasso, Umschalt fügt hinzu. Dann alle auf einmal zuteilen (S)",
        ),
        (
            "Drag over areas to assign them to the active territory. Shift+click picks a territory, Alt+drag erases, right-drag moves the map (B)",
            "Über Flächen ziehen teilt sie dem aktiven Gebiet zu. Umschalt+Klick wählt ein Gebiet, Alt+Ziehen radiert, Ziehen mit rechts verschiebt die Karte (B)",
        ),
        ("Nothing found", "Nichts gefunden"),
        ("not shown at this zoom", "bei diesem Zoom nicht sichtbar"),
        ("key", "Schlüssel"),
        ("row number", "Zeilennummer"),
        (
            "New boundaries from a file; data, colourings and territories carry over by key or by position",
            "Neue Grenzen aus einer Datei; Daten, Einfärbungen und Gebiete gehen nach Schlüssel oder Lage über",
        ),
        (
            "Read the file again (e.g. this month's figures); links and colourings stay",
            "Die Datei nochmals lesen (z. B. die Zahlen dieses Monats); Verknüpfungen und Einfärbungen bleiben",
        ),
        (
            "{territories} territories · {assigned} of {areas} areas assigned",
            "{territories} Gebiete · {assigned} von {areas} Flächen zugeteilt",
        ),
        (
            "Any XYZ tile server in Web Mercator: a URL with {z}, {x} and {y}",
            "Ein beliebiger XYZ-Kachelserver in Web Mercator: eine URL mit {z}, {x} und {y}",
        ),
        (
            "As web maps: needed for background maps; areas far from the equator look bigger",
            "Wie Webkarten: nötig für Hintergrundkarten; Flächen fern vom Äquator wirken grösser",
        ),
        (
            "Lambert equal-area around your data: true sizes, for print and comparisons",
            "Flächentreu nach Lambert um die Daten: wahre Grössen, für Druck und Vergleiche",
        ),
        (
            "Any EPSG code, e.g. 2056 (Switzerland), 25832 (Germany), 2154 (France); Enter applies",
            "Ein beliebiger EPSG-Code, z. B. 2056 (Schweiz), 25832 (Deutschland), 2154 (Frankreich); Enter übernimmt",
        ),
        ("Close (Esc)", "Schliessen (Esc)"),
        (
            "The column that identifies each area: what your data's codes are matched against (postcode, municipality number, …).",
            "Die Spalte, die jede Fläche bezeichnet: Damit werden die Codes Ihrer Daten abgeglichen (PLZ, Gemeindenummer, …).",
        ),
        (
            "A readable name for lists, labels and tooltips.",
            "Ein lesbarer Name für Listen, Beschriftungen und Hinweise.",
        ),
        (
            "One area per key: data for a key then colours all its parts. Without merging, data for such a key goes to the first area only.",
            "Eine Fläche pro Schlüssel: Daten zu einem Schlüssel färben dann alle seine Teile. Ohne Zusammenlegen gehen Daten zu einem solchen Schlüssel nur an die erste Fläche.",
        ),
        (
            "Each address is looked up with the geocoding service set in the settings; results are kept, so repeated addresses cost nothing",
            "Jede Adresse wird beim Geokodierdienst aus den Einstellungen nachgeschlagen; Ergebnisse werden gespeichert, wiederholte Adressen kosten nichts",
        ),
        (
            "For all rows: helps the service where the table has no country",
            "Für alle Zeilen: hilft dem Dienst, wo die Tabelle kein Land enthält",
        ),
        (
            "Each area coloured by its class",
            "Jede Fläche in der Farbe ihrer Klasse",
        ),
        (
            "A circle on each area or point; its area grows with the value",
            "Ein Kreis auf jeder Fläche oder jedem Punkt; seine Fläche wächst mit dem Wert",
        ),
        (
            "Where the points are dense, weighted by the value",
            "Wo die Punkte dicht liegen, gewichtet mit dem Wert",
        ),
        (
            "Colours that mix two values: see where both are high, or one but not the other",
            "Farben, die zwei Werte mischen: sehen, wo beide hoch sind oder nur einer",
        ),
        (
            "The areas territories consist of: postcodes, municipalities, …",
            "Die Flächen, aus denen Gebiete bestehen: PLZ, Gemeinden, …",
        ),
        (
            "Fine for overview maps; neighbouring borders may not fit exactly.",
            "Gut für Übersichtskarten; benachbarte Grenzen passen eventuell nicht genau.",
        ),
        (
            "A printed page puts the legend beside the map, with a north arrow and a footer",
            "Eine Druckseite setzt die Legende neben die Karte, mit Nordpfeil und Fusszeile",
        ),
        (
            "A small map of everything in the corner, with the page's part marked",
            "Eine kleine Karte von allem in der Ecke, mit dem Ausschnitt der Seite markiert",
        ),
        (
            "A table of the territories on the map with their measure and deviation, beside the legend",
            "Eine Tabelle der Gebiete auf der Karte mit ihrer Kennzahl und Abweichung, neben der Legende",
        ),
        (
            "{project}, {map} and {date} are filled in",
            "{project}, {map} und {date} werden eingesetzt",
        ),
        ("Insert at the cursor", "An der Schreibmarke einfügen"),
        (
            "Ctrl + and Ctrl − change it too",
            "Ctrl + und Ctrl − ändern sie auch",
        ),
        (
            "Unsaved changes are kept in a recovery copy, in case something goes wrong",
            "Ungespeicherte Änderungen werden in einer Sicherungskopie behalten, falls etwas schiefgeht",
        ),
        (
            "The public service or your own installation",
            "Der öffentliche Dienst oder eine eigene Installation",
        ),
        (
            "Between requests: the public Nominatim allows one per second; your own server may allow 0",
            "Zwischen Anfragen: Das öffentliche Nominatim erlaubt eine pro Sekunde; ein eigener Server erlaubt vielleicht 0",
        ),
        (
            "A PNG, JPEG or SVG image for exported maps and reports",
            "Ein PNG-, JPEG- oder SVG-Bild für exportierte Karten und Berichte",
        ),
        (
            "For new territory plans; a plan's Colours menu changes its own",
            "Für neue Gebietsplanungen; das Menü Farben einer Planung ändert ihre eigenen",
        ),
        (
            "An OSRM server for travel times by road: the public demo is for trying only; your own server has no limits",
            "Ein OSRM-Server für Fahrzeiten auf der Strasse: Die öffentliche Demo ist nur zum Ausprobieren; ein eigener Server hat keine Grenzen",
        ),
        (
            "Background map tiles are kept on disk: maps load fast and also show offline",
            "Kacheln der Hintergrundkarte werden auf der Festplatte behalten: Karten laden schnell und erscheinen auch offline",
        ),
        (
            "Click, then press the new keys",
            "Klicken, dann die neuen Tasten drücken",
        ),
        (
            "Points counted per cell, their numbers summed",
            "Punkte pro Zelle gezählt, ihre Zahlen summiert",
        ),
        (
            "Matched so the distances from the territories' centres are least overall",
            "So zugeordnet, dass die Distanzen von den Zentren der Gebiete insgesamt am kleinsten sind",
        ),
        (
            "Those that serve the areas best, weighted by the plan's measure; Balance → Start over then draws territories around them",
            "Die, welche die Flächen am besten bedienen, gewichtet mit der Kennzahl der Planung; Ausgleichen → Neu aufteilen zieht dann Gebiete um sie",
        ),
        (
            "Points within each ring, and the sums of their numbers",
            "Punkte in jedem Ring und die Summen ihrer Zahlen",
        ),
        (
            "Summed over the areas whose centre lies within the ring",
            "Summiert über die Flächen, deren Zentrum im Ring liegt",
        ),
        (
            "Minutes and km by road from each area's centre or point, from the routing service in Settings",
            "Minuten und km auf der Strasse vom Zentrum jeder Fläche oder jedem Punkt, vom Routing-Dienst in den Einstellungen",
        ),
        (
            "When several rows have the same key: numbers are combined this way, texts come from the first",
            "Wenn mehrere Zeilen denselben Schlüssel haben: Zahlen werden so zusammengefasst, Texte kommen von der ersten",
        ),
        (
            "A new, untitled project like this template",
            "Ein neues, unbenanntes Projekt wie diese Vorlage",
        ),
        ("Delete this template", "Diese Vorlage löschen"),
        (
            "Click to edit the formula",
            "Klicken, um die Formel zu bearbeiten",
        ),
        (
            "Columns of the layer, of linked tables and of points inside",
            "Spalten der Ebene, verknüpfter Tabellen und der Punkte darin",
        ),
        (
            "Classes and colours kept by name, to use on other maps and in other projects",
            "Klassen und Farben unter einem Namen behalten, für andere Karten und Projekte",
        ),
        (
            "Between the classes, e.g. 1000, 5000, 20000; or drag them in the histogram",
            "Zwischen den Klassen, z. B. 1000, 5000, 20000; oder im Histogramm ziehen",
        ),
        (
            "Fill areas that have no data in a colour of their own (otherwise they stay empty)",
            "Flächen ohne Daten in einer eigenen Farbe füllen (sonst bleiben sie leer)",
        ),
        (
            "Hide it when zoomed out farther, e.g. postcode labels on a country map",
            "Ausblenden, wenn weiter herausgezoomt, z. B. PLZ-Beschriftungen auf einer Landeskarte",
        ),
        (
            "Hide it when zoomed in closer, e.g. a summary layer that gives way to details",
            "Ausblenden, wenn näher hineingezoomt, z. B. eine Übersichtsebene, die Details Platz macht",
        ),
        (
            "Each value in equal-count classes (quantiles)",
            "Jeder Wert in gleich grossen Klassen (Quantile)",
        ),
        (
            "Pie areas grow with the sum of the values; else all pies are alike",
            "Kuchenflächen wachsen mit der Summe der Werte; sonst sind alle Kuchen gleich",
        ),
        ("Kilometres, e.g. 5, 10, 20", "Kilometer, z. B. 5, 10, 20"),
        (
            "Radius of the circle for the largest value; circle areas grow with the value",
            "Radius des Kreises für den grössten Wert; Kreisflächen wachsen mit dem Wert",
        ),
        (
            "By area is true to the data; by width makes differences look bigger; logarithmic suits values over many magnitudes",
            "Nach Fläche ist datengetreu; nach Breite lässt Unterschiede grösser wirken; logarithmisch passt zu Werten über viele Grössenordnungen",
        ),
        ("drag to move", "ziehen zum Verschieben"),
        (
            "A label of your own for the legend; empty shows the limits",
            "Eine eigene Beschriftung für die Legende; leer zeigt die Grenzen",
        ),
        (
            "Named filters (yes/no columns) to reuse in tables, analyses and exports",
            "Benannte Filter (Ja/Nein-Spalten) für Tabellen, Analysen und Exporte",
        ),
        (
            "Count or add up only some rows, e.g. [Segment] = \"Retail\" or [Orders] > 10",
            "Nur einige Zeilen zählen oder summieren, z. B. [Segment] = \"Retail\" oder [Orders] > 10",
        ),
        (
            "Sums grow with the size of areas: per km² or per another value (customers, population) makes areas comparable",
            "Summen wachsen mit der Grösse der Flächen: pro km² oder pro anderem Wert (Kunden, Bevölkerung) werden Flächen vergleichbar",
        ),
        ("Assign to {name}", "{name} zuteilen"),
        (
            "The selected areas join the active territory (locked ones stay)",
            "Die gewählten Flächen kommen zum aktiven Gebiet (gesperrte bleiben)",
        ),
        (
            "The quickest round by road visiting the selected points, from the routing service",
            "Die schnellste Tour auf der Strasse durch die gewählten Punkte, vom Routing-Dienst",
        ),
        (
            "Select what lies in the selection on another layer, e.g. the postcodes of selected districts",
            "Auf einer anderen Ebene wählen, was in der Auswahl liegt, z. B. die PLZ gewählter Bezirke",
        ),
        (
            "A yes/no calculated column, e.g. [Sales] > 10000: it filters tables and analyses",
            "Eine berechnete Ja/Nein-Spalte, z. B. [Sales] > 10000: Sie filtert Tabellen und Analysen",
        ),
        (
            "Only the rows of a segment: a yes/no column",
            "Nur die Zeilen eines Segments: einer Ja/Nein-Spalte",
        ),
        (
            "Values from the map (the area a point lies in, points in an area, the nearest point) or another table by key",
            "Werte von der Karte (die Fläche, in der ein Punkt liegt, Punkte in einer Fläche, der nächste Punkt) oder aus einer anderen Tabelle per Schlüssel",
        ),
        (
            "A new table with one row per group, e.g. sales per rep",
            "Eine neue Tabelle mit einer Zeile pro Gruppe, z. B. Umsatz pro Aussendienst",
        ),
        (
            "A weighted mix of several figures, as a calculated column",
            "Eine gewichtete Mischung mehrerer Kennzahlen, als berechnete Spalte",
        ),
        (
            "An empty row at the end; double-click cells to fill them",
            "Eine leere Zeile am Ende; Zellen doppelklicken, um sie zu füllen",
        ),
        (
            "This table with each row's territories (only the segment's rows when one is chosen)",
            "Diese Tabelle mit den Gebieten jeder Zeile (nur die Zeilen des Segments, wenn eines gewählt ist)",
        ),
        (
            "Click to sort, right-click to edit",
            "Klicken sortiert, Rechtsklick bearbeitet",
        ),
        (
            "click to sort, drag onto the map to show it",
            "klicken sortiert, auf die Karte ziehen zeigt sie",
        ),
        (
            "Territory in “{plan}”: follows the plan as you change it · click to sort",
            "Gebiet in «{plan}»: folgt der Planung, wenn sie sich ändert · klicken sortiert",
        ),
        (
            "A copy to try changes on. The copy is compared with this plan.",
            "Eine Kopie zum Ausprobieren. Die Kopie wird mit dieser Planung verglichen.",
        ),
        (
            "Each territory's quickest round by road through its customers, from its site: km and hours per territory and each customer's visit order",
            "Die schnellste Tour jedes Gebiets auf der Strasse durch seine Kunden, ab seinem Standort: km und Stunden pro Gebiet und die Besuchsreihenfolge jedes Kunden",
        ),
        (
            "All territories in a table: edit codes, names, regions, sizes; sort by any figure",
            "Alle Gebiete in einer Tabelle: Codes, Namen, Regionen, Grössen bearbeiten; nach jeder Kennzahl sortieren",
        ),
        (
            "The territories, and each territory's areas with their figures on a sheet of its own, e.g. to hand to each rep",
            "Die Gebiete und die Flächen jedes Gebiets mit ihren Kennzahlen auf einem eigenen Blatt, z. B. für jeden Aussendienst",
        ),
        (
            "The figure territories should have about equally of (weighted by each territory's size).",
            "Die Kennzahl, von der Gebiete etwa gleich viel haben sollen (gewichtet mit der Grösse jedes Gebiets).",
        ),
        (
            "{name} · code {code}\nClick: paint with it · double-click: rename · right-click: more",
            "{name} · Code {code}\nKlick: damit malen · Doppelklick: umbenennen · Rechtsklick: mehr",
        ),
        (
            "Locked: keeps its areas when painting and balancing",
            "Gesperrt: behält seine Flächen beim Malen und Ausgleichen",
        ),
        (
            "\nfarthest area {km} km from the site",
            "\nfernste Fläche {km} km vom Standort",
        ),
        (
            "{areas} areas · {km2} km²\ntarget {target}{reach}",
            "{areas} Flächen · {km2} km²\nZiel {target}{reach}",
        ),
        (
            "The site lies outside the territory",
            "Der Standort liegt ausserhalb des Gebiets",
        ),
        ("In {n} separate pieces", "In {n} getrennten Teilen"),
        (
            "Its areas become contiguous parts of about equal size by the measure",
            "Seine Flächen werden zu zusammenhängenden Teilen etwa gleicher Grösse nach der Kennzahl",
        ),
        (
            "Each territory takes the name of its largest area by the measure (or by size). Locked territories keep theirs.",
            "Jedes Gebiet erhält den Namen seiner grössten Fläche nach der Kennzahl (oder nach Grösse). Gesperrte Gebiete behalten ihren.",
        ),
        (
            "Codes 01, 02, … in reading order on the map; default names follow",
            "Codes 01, 02, … in Leserichtung auf der Karte; Standardnamen folgen",
        ),
        (
            "A site (rep home, branch) for each territory without one, in the area nearest the centre of its {what}. Start over then builds territories around the sites.",
            "Ein Standort (Wohnort, Filiale) für jedes Gebiet ohne einen, in der Fläche nächst dem Zentrum seiner {what}. Neu aufteilen baut dann Gebiete um die Standorte.",
        ),
        (
            "Every site goes to the area nearest the centre of its territory's {what}",
            "Jeder Standort kommt in die Fläche nächst dem Zentrum der {what} seines Gebiets",
        ),
        (
            "A scenario where each site takes its nearest areas until its territory holds its target plus the tolerance: what the sites can cover, and what is left over",
            "Ein Szenario, in dem jeder Standort seine nächsten Flächen nimmt, bis sein Gebiet sein Ziel plus Toleranz hat: was die Standorte abdecken und was übrig bleibt",
        ),
        (
            "Rep homes or branches from a point layer, matched to the territories; or the best of many possible locations",
            "Wohnorte oder Filialen aus einer Punktebene, den Gebieten zugeordnet; oder die besten vieler möglicher Orte",
        ),
        (
            "Neighbouring territories become regions of about equal size by the measure; existing regions are replaced",
            "Benachbarte Gebiete werden zu Regionen etwa gleicher Grösse nach der Kennzahl; bestehende Regionen werden ersetzt",
        ),
        (
            "What people call it: T01, a rep number, a region code",
            "Wie man es nennt: T01, eine Aussendienstnummer, ein Regionscode",
        ),
        (
            "Its share of the total: 1 is a normal territory, 0.5 one for a part-time rep, 2 a double one",
            "Sein Anteil am Ganzen: 1 ist ein normales Gebiet, 0.5 eines für Teilzeit, 2 ein doppeltes",
        ),
        (
            "Painting and balancing leave it as it is",
            "Malen und Ausgleichen lassen es, wie es ist",
        ),
        (
            "Points in the areas that change territory",
            "Punkte in den Flächen, die das Gebiet wechseln",
        ),
        (
            "The original plan takes over this scenario's territories",
            "Die ursprüngliche Planung übernimmt die Gebiete dieses Szenarios",
        ),
        (
            "The colours territories take; Recolour then keeps neighbours apart",
            "Die Farben der Gebiete; Neu einfärben hält dann Nachbarn auseinander",
        ),
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

    #[test]
    fn placeholders_filled_after_translating() {
        set_language(Language::German);
        assert_eq!(
            trf(
                "Couldn't save {file}: {e}",
                &[("file", &"a.territoria"), ("e", &"full")]
            ),
            "a.territoria konnte nicht gespeichert werden: full"
        );
        set_language(Language::English);
        assert_eq!(trf("Moved {n} areas.", &[("n", &3)]), "Moved 3 areas.");
    }

    #[test]
    fn translations_keep_their_placeholders() {
        let names = |t: &str| {
            let mut v: Vec<String> = t
                .split('{')
                .skip(1)
                .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_owned()))
                .collect();
            v.sort();
            v
        };
        for (k, v) in GERMAN.iter() {
            assert_eq!(names(k), names(v), "{k}");
        }
    }
}
