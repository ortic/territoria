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
