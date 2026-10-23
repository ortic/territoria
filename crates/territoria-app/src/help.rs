//! Help (F1): short topics that explain what things are for and how to
//! get them done, opened on the topic of what is selected.

use egui::{RichText, Ui};

pub struct Topic {
    pub id: &'static str,
    pub title: &'static str,
    /// Paragraphs; `- ` starts a bullet, `## ` a heading.
    pub body: &'static str,
}

pub const TOPICS: &[Topic] = &[
    Topic {
        id: "start",
        title: "Getting started",
        body: "Territoria shows your data on maps of areas (postcodes, municipalities, regions) and plans sales territories on them.

## The usual way
- Add boundaries: a Shapefile, GeoJSON, GeoPackage or KML, or download open boundaries for many countries.
- Add data: a CSV or Excel table with a column of area keys (postcodes), addresses or coordinates.
- Link the table to the boundaries by its key column; Territoria finds the best way to match (leading zeros, case).
- Colour by value: pick a number to colour the areas, or show circles, charts or a heat map.
- New territories: split the areas into balanced territories, then adjust by painting.

Everything can be undone (Ctrl+Z). The command palette (Ctrl+Shift+P) finds every command by name.",
    },
    Topic {
        id: "boundaries",
        title: "Boundaries and layers",
        body: "A layer is a set of areas, lines or points with a table of attributes. Each area has a key (e.g. its postcode) used to link data and assign territories.

- Formats: Shapefile (also zipped), GeoJSON, GeoPackage, KML/KMZ. The coordinate system comes from the file or can be given as an EPSG code.
- File → Download Open Boundaries fetches administrative areas for many countries.
- Data → New Layer by Merging Areas joins areas by a column or a key prefix (postcode districts from postcodes).
- Data → New Hexagon Grid covers an area with equal-size hexagons, for analyses independent of administrative borders.
- Data → Replace Boundaries with a Newer Version keeps links and territory assignments, matching areas by key and place.",
    },
    Topic {
        id: "data",
        title: "Data, links and addresses",
        body: "Tables bring the numbers: customers, sales, potential.

- A table with an area key column is linked to a layer; several rows per area are added up (or counted, averaged…).
- Addresses become points by geocoding (Settings → geocoder); coordinates in one column (\"47.37, 8.54\") or two are read directly.
- The table view edits cells, adds and deletes rows, and sorts; Reload reads a changed file again.
- ⊕ Columns from takes columns of the area each point lies in, counts points per area, looks up another table by key or finds the nearest point of another layer, also by road: minutes and km to the nearest branch from a routing server (Settings → Routing).
- Σ Summarise makes a new table with one row per group; ⚖ Score combines columns into one weighted score.
- Segments are yes/no columns, e.g. a calculated [Key account] = [Sales] > 10000: the table shows and exports only a segment's rows, and \"Only where\" in analyses picks segments or keeps a typed filter as a new one.",
    },
    Topic {
        id: "formulas",
        title: "Calculated columns and formulas",
        body: "A calculated column computes a value per row from other columns, like a spreadsheet formula, and updates when the data changes.

- Columns are written in brackets: [Revenue] / [Customers].
- Text in quotes, dates as date(\"2026-01-31\"); if(cond, a, b) chooses.
- Column functions look at the whole column: share([Sales]) is each row's part of the total, rank([Sales]) its position; most take an optional group, e.g. share([Sales], [Region]).
- Templates in labels and hover cards put formulas in braces: {Name}: {round([Sales] / 1000)} k.

## Functions",
    },
    Topic {
        id: "colour",
        title: "Colouring areas by value",
        body: "Colour by value classifies a number and colours each area by its class.

- Methods: equal count (quantiles), equal intervals, natural breaks, standard deviation, equal sums, or your own breaks; drag the limits in the histogram of the class editor.
- Relative to: per km², per another value (e.g. sales per inhabitant) or as a share of the total.
- Only where: count or add up only the rows that pass a filter, e.g. [Segment] = \"Retail\".
- Two values at once: a grid of colours shows where both are high, or one but not the other.
- Categories: a text column gives each value its own colour; the most frequent ones get colours, the rest is \"Other\".
- Quickest of all: drag a column's heading from a table onto the map (or the drop strip in Contents); \"Show as\" in Properties switches between colours, circles and a heat map.",
    },
    Topic {
        id: "symbols",
        title: "Circles, charts, heat maps and flows",
        body: "- Circles sized by value: circle areas grow with the number; they can take their colour from a category; also squares, diamonds and triangles.
- Pie and bar charts: several numbers per area, as shares of a pie sized by the total, or bars on one scale.
- Heat map: where points are dense, optionally weighted by a number; set how far each point spreads.
- Catchment rings: rings of given distances around points or territory sites, with figures within each ring.
- Drive-time zones: areas coloured by their minutes by road to the nearest site, in bands you choose (15, 30, 45 minutes).
- Flow lines: lines from where each row starts to where it goes (customers to their branch), as wide as their value.
- Every map layer can be limited to zoom levels: Shown → only when zoomed in or out.",
    },
    Topic {
        id: "territories",
        title: "Territory plans",
        body: "A plan assigns the areas of a layer to territories, e.g. one per sales rep.

- New plan: start empty, from a table of assignments, or balanced automatically by a measure (customers, potential) with a tolerance.
- Paint (B): drag over areas to give them to the active territory; Shift+click picks the territory under the pointer.
- Balance improves sizes while keeping territories contiguous and compact; locked territories stay as they are.
- Sites place each territory's office or rep; territories can be built around sites, distances to the site measured, and \"Coverage from the sites\" shows what the sites can cover up to their capacity.
- Regions group territories with wider borders; they can be formed automatically or by hand, and territories dragged between regions. ➕ Level above groups the regions in turn (regions into divisions, and so on, any number of levels), each level with its own name, figures, wider borders and a column in exports.
- On plans of customer points, \"Rounds for all territories\" plans a round by road for each.
- Before balancing, a check points out what would get in the way: areas without data, negative values, areas cut off from their neighbours.",
    },
    Topic {
        id: "scenarios",
        title: "Scenarios and comparing plans",
        body: "Duplicate a plan to try changes without losing the original. Compare shows which areas moved and how each territory changed; Adopt makes a scenario the plan again.",
    },
    Topic {
        id: "selection",
        title: "Selecting areas",
        body: "- Select (S): click, drag a rectangle, or hold Alt to draw a lasso; with Shift or Ctrl the selection grows.
- Map → Select Areas by List takes keys pasted from a spreadsheet.
- The selection bar shows the count and the plan's measure, assigns the selection to the active territory, zooms to it or fades everything else (Focus).
- Select in: carries the selection to another layer, e.g. the postcodes inside selected districts or the customers in them.
- Round: for selected points (a rep's customers), the quickest round by road from the routing service, drawn on the map with its km and time, and each point's place in it as a column.
- Rows selected in a table are selected on the map.",
    },
    Topic {
        id: "views",
        title: "Views, projection and presenting",
        body: "- Keep a view (Map → Keep This View) to come back to it; views are kept with the project.
- Projection: Web Mercator (like web maps) or an equal-area or national projection by EPSG code.
- Background: map tiles from OpenStreetMap or another provider; or a vector map, drawn by a MapLibre style (Positron, Liberty, or your own) from tiles online or from a .pmtiles or .mbtiles file, which works offline. Vector maps draw fills and lines, no labels yet.
- Presentation (F11) shows the map alone, full screen.",
    },
    Topic {
        id: "export",
        title: "Exports and reports",
        body: "- Map → Export Map: PNG, SVG or PDF with title, legend and scale bar, as on screen or on an A4/A3 page with the legend beside the map, a north arrow, an overview map and a footer ({project}, {map}, {date}); Copy Map puts it on the clipboard.
- Arrange the page…: drag the map, title, legend, figures, overview and logo where you like, size them by their corner, add texts and images; the arrangement is kept with the map.
- Export as Web Page: one HTML file that zooms, pans and shows hover cards, to send to anyone.
- Territory report: a PDF with an overview and a page per territory; Excel with a sheet per territory.
- Map → Export Atlas: the map as a PDF with a page per region, rep or area, each zoomed to it with the rest veiled; also per two columns together (region and segment), as a PDF per page to hand out, and with an Excel workbook of each page's areas.
- Plan exports: the assignment as CSV or Excel, changes between scenarios, territory figures.
- Settings → Company puts your company's name and logo on exported maps, web pages and reports.
- Services that need an API key take it in Settings under their address; it is kept in the system keychain, not the settings file, and sent where the address says {key}.
- Command line: territoria project.territoria --export-map map.png (see --help).",
    },
];

/// The topics in German: (id, title, body).
const GERMAN: &[(&str, &str, &str)] = &[
    (
        "start",
        "Erste Schritte",
        "Territoria zeigt Ihre Daten auf Karten von Gebieten (Postleitzahlen, Gemeinden, Regionen) und plant darauf Verkaufsgebiete.

## Der übliche Weg
- Grenzen hinzufügen: ein Shapefile, GeoJSON, GeoPackage oder KML, oder offene Grenzen vieler Länder herunterladen.
- Daten hinzufügen: eine CSV- oder Excel-Tabelle mit einer Spalte von Gebietsschlüsseln (PLZ), Adressen oder Koordinaten.
- Die Tabelle über ihre Schlüsselspalte mit den Grenzen verknüpfen; Territoria findet die beste Zuordnung (führende Nullen, Gross-/Kleinschreibung).
- Nach Wert einfärben: eine Zahl wählen und die Gebiete einfärben, oder Kreise, Diagramme oder eine Heatmap zeigen.
- Neue Gebiete: die Flächen in ausgeglichene Gebiete aufteilen und dann durch Malen anpassen.

Alles lässt sich rückgängig machen (Ctrl+Z). Die Befehlspalette (Ctrl+Shift+P) findet jeden Befehl nach Namen.",
    ),
    (
        "boundaries",
        "Grenzen und Ebenen",
        "Eine Ebene ist eine Menge von Flächen, Linien oder Punkten mit einer Attributtabelle. Jede Fläche hat einen Schlüssel (z. B. ihre PLZ), über den Daten verknüpft und Gebiete zugeteilt werden.

- Formate: Shapefile (auch gezippt), GeoJSON, GeoPackage, KML/KMZ. Das Koordinatensystem kommt aus der Datei oder wird als EPSG-Code angegeben.
- Datei → Offene Grenzen herunterladen holt Verwaltungsgebiete vieler Länder.
- Daten → Neue Ebene aus zusammengelegten Gebieten fasst Gebiete nach einer Spalte oder einem Schlüsselanfang zusammen (PLZ-Bezirke aus PLZ).
- Daten → Neues Sechseckraster deckt eine Fläche mit gleich grossen Sechsecken ab, für Analysen unabhängig von Verwaltungsgrenzen.
- Daten → Grenzen durch neuere Version ersetzen behält Verknüpfungen und Gebietszuteilungen, zugeordnet über Schlüssel und Lage.",
    ),
    (
        "data",
        "Daten, Verknüpfungen und Adressen",
        "Tabellen bringen die Zahlen: Kunden, Umsatz, Potenzial.

- Eine Tabelle mit einer Spalte von Gebietsschlüsseln wird mit einer Ebene verknüpft; mehrere Zeilen pro Gebiet werden addiert (oder gezählt, gemittelt…).
- Adressen werden durch Geokodierung zu Punkten (Einstellungen → Geokodierung); Koordinaten in einer Spalte (\"47.37, 8.54\") oder zwei werden direkt gelesen.
- Die Tabellenansicht bearbeitet Zellen, fügt Zeilen hinzu, löscht und sortiert; Neu laden liest eine geänderte Datei wieder ein.
- ⊕ Spalten aus übernimmt Spalten des Gebiets, in dem ein Punkt liegt, zählt Punkte pro Gebiet, schlägt in einer anderen Tabelle nach oder findet den nächsten Punkt einer anderen Ebene, auch über die Strasse: Minuten und Kilometer zur nächsten Filiale vom Routing-Dienst (Einstellungen → Routing).
- Σ Zusammenfassen erstellt eine Tabelle mit einer Zeile pro Gruppe; ⚖ Punktwert verbindet Spalten zu einer gewichteten Kennzahl.
- Segmente sind Ja/Nein-Spalten, z. B. eine berechnete [Schlüsselkunde] = [Sales] > 10000: die Tabelle zeigt und exportiert nur die Zeilen eines Segments, und «Nur wo» in Analysen wählt Segmente oder behält einen eingegebenen Filter als neues.",
    ),
    (
        "formulas",
        "Berechnete Spalten und Formeln",
        "Eine berechnete Spalte rechnet pro Zeile einen Wert aus anderen Spalten aus, wie eine Tabellenkalkulation, und wird nachgeführt, wenn sich die Daten ändern.

- Spalten stehen in eckigen Klammern: [Revenue] / [Customers].
- Text in Anführungszeichen, Daten als date(\"2026-01-31\"); if(Bedingung, a, b) wählt.
- Spaltenfunktionen betrachten die ganze Spalte: share([Sales]) ist der Anteil jeder Zeile am Total, rank([Sales]) ihr Rang; die meisten nehmen eine Gruppe, z. B. share([Sales], [Region]).
- Vorlagen in Beschriftungen und Infokarten setzen Formeln in geschweifte Klammern: {Name}: {round([Sales] / 1000)} k.

## Funktionen",
    ),
    (
        "colour",
        "Gebiete nach Wert einfärben",
        "Nach Wert einfärben teilt eine Zahl in Klassen ein und färbt jedes Gebiet nach seiner Klasse.

- Methoden: gleiche Anzahl (Quantile), gleiche Abstände, natürliche Brüche, Standardabweichung, gleiche Summen oder eigene Grenzen; die Grenzen lassen sich im Histogramm des Klasseneditors ziehen.
- Bezogen auf: pro km², pro einem anderen Wert (z. B. Umsatz pro Einwohner) oder als Anteil am Total.
- Nur wo: nur die Zeilen zählen oder addieren, die einen Filter erfüllen, z. B. [Segment] = \"Retail\".
- Zwei Werte zugleich: ein Farbraster zeigt, wo beide hoch sind oder nur einer.
- Kategorien: eine Textspalte gibt jedem Wert eine eigene Farbe; die häufigsten bekommen Farben, der Rest ist «Andere».
- Am schnellsten: eine Spaltenüberschrift aus einer Tabelle auf die Karte ziehen (oder auf den Ablagestreifen im Inhalt); «Zeigen als» in den Eigenschaften wechselt zwischen Farben, Kreisen und Heatmap.",
    ),
    (
        "symbols",
        "Kreise, Diagramme, Heatmaps und Flüsse",
        "- Kreise nach Wert: die Kreisflächen wachsen mit der Zahl; ihre Farbe kann eine Kategorie zeigen; auch Quadrate, Rauten und Dreiecke.
- Kuchen- und Balkendiagramme: mehrere Zahlen pro Gebiet, als Anteile eines Kuchens nach der Summe oder als Balken auf einer Skala.
- Heatmap: wo Punkte dicht liegen, wahlweise nach einer Zahl gewichtet; die Reichweite jedes Punkts ist einstellbar.
- Einzugsringe: Ringe in bestimmten Abständen um Punkte oder Standorte, mit den Kennzahlen innerhalb jedes Rings.
- Fahrzeitzonen: Gebiete nach ihren Fahrminuten zum nächsten Standort eingefärbt, in selbst gewählten Stufen (15, 30, 45 Minuten).
- Flusslinien: Linien von wo jede Zeile beginnt bis wohin sie geht (Kunden zu ihrer Filiale), so breit wie ihr Wert.
- Jede Kartenebene lässt sich auf Zoomstufen beschränken: Sichtbar → nur beim Hinein- oder Herauszoomen.",
    ),
    (
        "territories",
        "Gebietsplanungen",
        "Eine Planung teilt die Flächen einer Ebene Gebieten zu, z. B. eines pro Vertreter.

- Neue Planung: leer beginnen, aus einer Zuordnungstabelle oder automatisch ausgeglichen nach einer Kennzahl (Kunden, Potenzial) mit einer Toleranz.
- Malen (B): über Flächen ziehen gibt sie dem aktiven Gebiet; Shift+Klick wählt das Gebiet unter dem Mauszeiger.
- Ausgleichen verbessert die Grössen und hält die Gebiete zusammenhängend und kompakt; gesperrte Gebiete bleiben, wie sie sind.
- Standorte setzen das Büro oder den Vertreter jedes Gebiets; Gebiete lassen sich um Standorte aufbauen, Distanzen zum Standort messen, und «Abdeckung von den Standorten» zeigt, was die Standorte bis zu ihrer Kapazität abdecken.
- Regionen gruppieren Gebiete mit breiteren Grenzen; automatisch gebildet oder von Hand, Gebiete lassen sich zwischen Regionen ziehen. ➕ Ebene darüber gruppiert die Regionen wiederum (Regionen zu Divisionen und so weiter, beliebig viele Ebenen), jede Ebene mit eigenem Namen, Kennzahlen, breiteren Grenzen und einer Spalte in Exporten.
- Bei Planungen auf Kundenpunkten plant «Touren für alle Gebiete» je eine Tour über die Strasse.
- Vor dem Ausgleichen zeigt eine Prüfung, was im Weg stünde: Gebiete ohne Daten, negative Werte, von ihren Nachbarn abgeschnittene Flächen.",
    ),
    (
        "scenarios",
        "Szenarien und Vergleiche",
        "Eine Planung duplizieren, um Änderungen auszuprobieren, ohne das Original zu verlieren. Der Vergleich zeigt, welche Flächen wechselten und wie sich jedes Gebiet veränderte; Übernehmen macht ein Szenario wieder zur Planung.",
    ),
    (
        "selection",
        "Gebiete auswählen",
        "- Auswählen (S): klicken, ein Rechteck ziehen oder mit gedrückter Alt-Taste ein Lasso zeichnen; mit Shift oder Ctrl wächst die Auswahl.
- Karte → Gebiete per Liste auswählen nimmt aus einer Tabellenkalkulation eingefügte Schlüssel.
- Die Auswahlleiste zeigt die Anzahl und die Kennzahl der Planung, teilt die Auswahl dem aktiven Gebiet zu, zoomt hin oder blendet alles andere ab (Fokus).
- Auswählen in: überträgt die Auswahl auf eine andere Ebene, z. B. die PLZ in ausgewählten Bezirken oder die Kunden darin.
- Tour: für ausgewählte Punkte (die Kunden eines Vertreters) die schnellste Tour über die Strasse vom Routing-Dienst, auf der Karte mit Kilometern und Zeit, und die Reihenfolge als Spalte.
- In einer Tabelle ausgewählte Zeilen werden auf der Karte ausgewählt.",
    ),
    (
        "views",
        "Ansichten, Projektion und Präsentation",
        "- Eine Ansicht behalten (Karte → Diese Ansicht behalten), um darauf zurückzukommen; Ansichten werden mit dem Projekt gespeichert.
- Projektion: Web Mercator (wie Webkarten) oder eine flächentreue oder nationale Projektion über ihren EPSG-Code.
- Hintergrund: Kartenkacheln von OpenStreetMap oder einem anderen Anbieter; oder eine Vektorkarte, gezeichnet nach einem MapLibre-Stil (Positron, Liberty oder ein eigener) aus Kacheln online oder aus einer .pmtiles- oder .mbtiles-Datei, die auch offline funktioniert. Vektorkarten zeichnen Flächen und Linien, noch keine Beschriftungen.
- Präsentation (F11) zeigt nur die Karte im Vollbild.",
    ),
    (
        "export",
        "Exporte und Berichte",
        "- Karte → Karte exportieren: PNG, SVG oder PDF mit Titel, Legende und Massstab, wie am Bildschirm oder auf einer A4/A3-Seite mit der Legende neben der Karte, Nordpfeil, Übersichtskarte und Fusszeile ({project}, {map}, {date}); Karte als Bild kopieren legt sie in die Zwischenablage.
- Seite anordnen…: Karte, Titel, Legende, Kennzahlen, Übersicht und Logo frei hinziehen, an der Ecke in der Grösse ändern, Texte und Bilder hinzufügen; die Anordnung bleibt bei der Karte.
- Als Webseite exportieren: eine HTML-Datei, die zoomt, verschiebt und Infokarten zeigt, zum Versenden an alle.
- Gebietsbericht: ein PDF mit Übersicht und einer Seite pro Gebiet; Excel mit einem Blatt pro Gebiet.
- Karte → Atlas exportieren: die Karte als PDF mit einer Seite pro Region, Vertreter oder Gebiet, jede darauf gezoomt und der Rest abgeblendet; auch pro zwei Spalten zusammen (Region und Segment), als ein PDF pro Seite zum Weitergeben und mit einer Excel-Mappe der Flächen jeder Seite.
- Exporte der Planung: die Zuteilung als CSV oder Excel, Änderungen zwischen Szenarien, Kennzahlen der Gebiete.
- Einstellungen → Firma setzt Firmenname und Logo auf exportierte Karten, Webseiten und Berichte.
- Dienste, die einen API-Schlüssel brauchen, erhalten ihn in den Einstellungen unter ihrer Adresse; er bleibt im Schlüsselbund des Systems, nicht in der Einstellungsdatei, und wird dort gesendet, wo die Adresse {key} enthält.
- Befehlszeile: territoria projekt.territoria --export-map karte.png (siehe --help).",
    ),
    (
        "shortcuts",
        "Tastenkürzel",
        "tasten kürzel tastatur",
    ),
];

/// A topic's title and body in the interface language.
fn localized(t: &Topic) -> (&'static str, &'static str) {
    if crate::i18n::language() == crate::i18n::Language::German
        && let Some((_, title, body)) = GERMAN.iter().find(|(id, _, _)| *id == t.id)
    {
        return (title, body);
    }
    (t.title, t.body)
}

/// The topic for what is selected, if any fits.
pub fn topic_for(
    selection: crate::app::Selection,
    project: &territoria_model::Project,
    territories_tab: bool,
) -> &'static str {
    use crate::app::Selection;
    use territoria_model::MapLayerKind;
    if territories_tab {
        return "territories";
    }
    match selection {
        Selection::MapLayer(map, id) => {
            match project.map(map).and_then(|m| m.layer(id)).map(|l| &l.kind) {
                Some(
                    MapLayerKind::Choropleth { .. }
                    | MapLayerKind::Categories { .. }
                    | MapLayerKind::Bivariate { .. },
                ) => "colour",
                Some(
                    MapLayerKind::Symbols { .. }
                    | MapLayerKind::Charts { .. }
                    | MapLayerKind::Heatmap { .. }
                    | MapLayerKind::Rings { .. }
                    | MapLayerKind::Flows { .. },
                ) => "symbols",
                Some(MapLayerKind::Territories { .. }) => "territories",
                _ => "boundaries",
            }
        }
        Selection::Data(crate::app::DataRef::Table(_)) => "data",
        Selection::Data(crate::app::DataRef::Layer(_)) => "boundaries",
        Selection::None => "start",
    }
}

pub struct HelpWindow {
    pub topic: &'static str,
    pub search: String,
}

impl HelpWindow {
    pub fn new(topic: &'static str) -> HelpWindow {
        HelpWindow {
            topic,
            search: String::new(),
        }
    }

    /// Topics whose title or text has all words of the search.
    fn matches(&self, t: &Topic) -> bool {
        let (title, body) = localized(t);
        let text = format!("{title} {body}").to_lowercase();
        self.search
            .to_lowercase()
            .split_whitespace()
            .all(|w| text.contains(w))
    }

    pub fn show(&mut self, ctx: &egui::Context, keymap: &crate::commands::Keymap, open: &mut bool) {
        egui::Window::new(crate::i18n::tr("Help"))
            .open(open)
            .default_size([720.0, 520.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text(crate::i18n::tr("Search help"))
                                .desired_width(180.0),
                        );
                        ui.add_space(4.0);
                        let found: Vec<&Topic> =
                            TOPICS.iter().filter(|t| self.matches(t)).collect();
                        for t in found {
                            if ui
                                .selectable_label(self.topic == t.id, localized(t).0)
                                .clicked()
                            {
                                self.topic = t.id;
                            }
                        }
                        if self.matches(&SHORTCUTS)
                            && ui
                                .selectable_label(
                                    self.topic == SHORTCUTS.id,
                                    localized(&SHORTCUTS).0,
                                )
                                .clicked()
                        {
                            self.topic = SHORTCUTS.id;
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("help-body")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width(480.0);
                                if self.topic == SHORTCUTS.id {
                                    ui.heading(localized(&SHORTCUTS).0);
                                    shortcuts(ui, keymap);
                                } else if let Some(t) = TOPICS.iter().find(|t| t.id == self.topic) {
                                    let (title, text) = localized(t);
                                    ui.heading(title);
                                    ui.add_space(4.0);
                                    body(ui, text);
                                    if t.id == "formulas" {
                                        functions(ui);
                                    }
                                }
                            });
                        });
                });
            });
    }
}

const SHORTCUTS: Topic = Topic {
    id: "shortcuts",
    title: "Keyboard shortcuts",
    body: "keys shortcuts keyboard",
};

fn body(ui: &mut Ui, text: &str) {
    for para in text.split("\n\n") {
        for line in para.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                ui.add_space(4.0);
                ui.label(RichText::new(h).strong());
            } else if let Some(b) = line.strip_prefix("- ") {
                ui.horizontal_wrapped(|ui| {
                    ui.label("•");
                    ui.label(b);
                });
            } else {
                ui.label(line);
            }
        }
        ui.add_space(6.0);
    }
}

/// The formula functions, by group.
fn functions(ui: &mut Ui) {
    let mut group = "";
    for f in territoria_expr::FUNCTIONS {
        if f.group != group {
            group = f.group;
            ui.add_space(4.0);
            ui.label(RichText::new(group).weak());
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(f.signature).monospace());
            ui.label(RichText::new(f.doc).small());
        });
    }
}

fn shortcuts(ui: &mut Ui, keymap: &crate::commands::Keymap) {
    egui::Grid::new("help-keys")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for c in crate::commands::Command::ALL {
                let keys = keymap.shortcut_text(ui.ctx(), *c);
                if !keys.is_empty() {
                    ui.label(RichText::new(keys).monospace());
                    ui.label(c.label());
                    ui.end_row();
                }
            }
        });
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "Single keys work when no text field is being edited. Change them in Settings → Keyboard shortcuts.",
        )
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_are_distinct_and_found() {
        let mut ids: Vec<&str> = TOPICS.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TOPICS.len());
        let mut w = HelpWindow::new("start");
        w.search = "lasso".into();
        let found: Vec<&str> = TOPICS
            .iter()
            .filter(|t| w.matches(t))
            .map(|t| t.id)
            .collect();
        assert_eq!(found, vec!["selection"]);
    }
}
