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
- ⊕ Columns from takes columns of the area each point lies in, counts points per area, looks up another table by key or finds the nearest point of another layer.
- Σ Summarise makes a new table with one row per group; ⚖ Score combines columns into one weighted score.",
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
- Categories: a text column gives each value its own colour; the most frequent ones get colours, the rest is \"Other\".",
    },
    Topic {
        id: "symbols",
        title: "Circles, charts, heat maps and flows",
        body: "- Circles sized by value: circle areas grow with the number; they can take their colour from a category.
- Pie and bar charts: several numbers per area, as shares of a pie sized by the total, or bars on one scale.
- Heat map: where points are dense, optionally weighted by a number; set how far each point spreads.
- Catchment rings: rings of given distances around points or territory sites, with figures within each ring.
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
- Sites place each territory's office or rep; territories can be built around sites and distances to the site measured.
- Regions group territories with wider borders; they can be formed automatically or by hand.
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
- Rows selected in a table are selected on the map.",
    },
    Topic {
        id: "views",
        title: "Views, projection and presenting",
        body: "- Keep a view (Map → Keep This View) to come back to it; views are kept with the project.
- Projection: Web Mercator (like web maps) or an equal-area or national projection by EPSG code.
- Background: map tiles from OpenStreetMap or another provider.
- Presentation (F11) shows the map alone, full screen.",
    },
    Topic {
        id: "export",
        title: "Exports and reports",
        body: "- Map → Export Map: PNG, SVG or PDF with title, legend and scale bar; Copy Map puts it on the clipboard.
- Export as Web Page: one HTML file that zooms, pans and shows hover cards, to send to anyone.
- Territory report: a PDF with an overview and a page per territory.
- Plan exports: the assignment as CSV or Excel, changes between scenarios, territory figures.
- Command line: territoria project.territoria --export-map map.png (see --help).",
    },
];

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
        let text = format!("{} {}", t.title, t.body).to_lowercase();
        self.search
            .to_lowercase()
            .split_whitespace()
            .all(|w| text.contains(w))
    }

    pub fn show(&mut self, ctx: &egui::Context, keymap: &crate::commands::Keymap, open: &mut bool) {
        egui::Window::new("Help")
            .open(open)
            .default_size([720.0, 520.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("Search help")
                                .desired_width(180.0),
                        );
                        ui.add_space(4.0);
                        let found: Vec<&Topic> =
                            TOPICS.iter().filter(|t| self.matches(t)).collect();
                        for t in found {
                            if ui.selectable_label(self.topic == t.id, t.title).clicked() {
                                self.topic = t.id;
                            }
                        }
                        if self.matches(&SHORTCUTS)
                            && ui
                                .selectable_label(self.topic == SHORTCUTS.id, SHORTCUTS.title)
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
                                    ui.heading(SHORTCUTS.title);
                                    shortcuts(ui, keymap);
                                } else if let Some(t) = TOPICS.iter().find(|t| t.id == self.topic) {
                                    ui.heading(t.title);
                                    ui.add_space(4.0);
                                    body(ui, t.body);
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
    ui.label(RichText::new("Single keys work when no text field is being edited.").weak());
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
