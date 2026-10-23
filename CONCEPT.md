# Territoria – geomarketing and territory planning in Rust

A native, fast, cross-platform desktop tool for putting business data on maps
and planning sales and service territories, written in Rust with `egui` on
the `wgpu` renderer (the stack of [Pythia](../pythia)). It tackles the same
problems as easymap (infas 360): from a spreadsheet to a map, analyses of
regional data, territory structures that are balanced, compact and easy to
change, and results that can be shared. It is not a copy: where easymap is
modal, Windows-bound, licence-metered or tied to proprietary map files,
Territoria takes the simpler road.

Items are checked off as they are done; a note in parentheses says what is
missing when an item is only partly there.

---

## 1. Vision & Goals

- **Easy**: a new user gets from an Excel file to a coloured map in under a
  minute — open boundaries, drop a table, the key is recognized, the match
  rate is shown, a sensible analysis appears with its legend.
- **Powerful**: multi-level territory structures, locks, scenarios, an
  optimizer that balances several measures with tolerances and keeps
  territories compact and contiguous, reports, exports.
- **Market-neutral**: any boundaries (postcodes, municipalities, bricks,
  customer points) from any country, any coordinate system, keys as text
  (leading zeros survive), no built-in assumptions about a market.
- **One live model**: territories, analyses, tables and KPIs are views of the
  same data; every change updates the map, tables and balance figures at
  once. Full undo/redo, autosave.
- **No modes where a tool will do**: no "planning mode" or "assignment mode";
  painting territories is a tool like any other, guarded by undo and locks.
- **Fast**: 100k units, millions of vertices, interactive. Long operations
  (import, optimization, geocoding) run in the background with progress and
  cancel; the UI thread never blocks.
- **Open formats, offline first**: Shapefile, GeoJSON, GeoPackage, CSV, Excel
  in; the same out. Online services (geocoding, tiles, routing, boundary
  downloads) are optional, configurable and self-hostable.
- **Cross-platform**: Linux, Windows, macOS from one codebase.

### Non-Goals (for now)

- A general GIS (editing vertices, topology repair, raster analysis).
- Selling or bundling commercial boundary or market data.
- A hosted web service (sharing works through exported files first).

---

## 2. Technology Stack

| Area | Choice | Notes |
|---|---|---|
| Language | Rust (stable, edition 2024) | Cargo workspace, like Pythia |
| UI | `egui` + `eframe` 0.36, `egui_dock` | Immediate mode, dockable tabs; wgpu with GL fallback |
| Charts | `egui_plot` | Histograms, balance charts |
| Geometry | `geo` / `geo-types`, `rstar` | Algorithms and spatial index |
| Coordinate systems | `proj4rs` + `crs-definitions` | Pure Rust; EPSG codes, proj strings, `.prj` WKT (own resolver for ESRI WKT) |
| Triangulation | `earcutr` | Polygon fills as GPU meshes |
| Vector formats | `shapefile` (+`dbase`), `geojson` | GeoPackage later (`rusqlite`) |
| Tables | `csv`, `calamine` (xlsx/xls/ods read), `rust_xlsxwriter` | |
| Expressions | own lexer (`logos`) and evaluator | One language for calculated columns, filters, labels |
| Project file | `zip` (deflate) with JSON + binary parts | One file, diff-friendly manifest |
| Parallelism | `rayon`, worker threads + channels | Optimizer, tessellation, import |
| HTTP | `ureq` (rustls) | Geocoding, tiles, routing, downloads |
| Config | `serde` + `toml`, `directories` | |
| Logging | `tracing` + `tracing-subscriber` | |
| Testing | `cargo test`, `insta`, `egui_kittest` (offscreen screenshots) | |

---

## 3. Architecture

```
territoria/
├── crates/
│   ├── territoria-app        # eframe entry point, docking, documents, commands, dialogs
│   ├── territoria-ui         # widgets: data grid, colour and ramp pickers, icons
│   ├── territoria-geo        # geometry, CRS, topology (shared arcs), simplification, spatial index
│   ├── territoria-data       # typed tables, expressions, aggregation, key matching, joins
│   ├── territoria-io         # Shapefile, GeoJSON, CSV, Excel in/out; boundary downloads
│   ├── territoria-model      # the project: layers, datasets, links, maps, analyses, plans; file format; undo
│   ├── territoria-analysis   # classification, colour ramps, symbol sizing, legends
│   ├── territoria-optimize   # territory and location optimization, colouring (no UI)
│   ├── territoria-render     # map scene: LOD meshes, styles, labels; egui painter and export backends
│   └── territoria-config     # settings, paths
```

### Core principles

- [x] UI thread never blocks: imports, tessellation, optimization, geocoding
      run on workers; results come back over channels
- [x] Every long-running operation shows progress and can be cancelled (progress and Stop for balancing and downloads; imports show a spinner)
- [x] Geometry is stored once in WGS84; maps project it into a display CRS
      (Web Mercator by default) and cache the result
- [x] Polygon layers are turned into a shared-border topology (arcs) on
      import: simplification without gaps, adjacency, territory borders and
      dissolve come from it
- [x] Command pattern for all user actions (menus, toolbar, shortcuts, command
      palette)
- [x] All project edits go through operations with undo/redo (snapshots; slider drags and paint strokes are one step each)
- [x] Keys are text; numeric-looking keys match regardless of leading zeros
      when the user allows it

---

## 4. Milestones

| Milestone | Scope |
|---|---|
| **M0 – Skeleton** | Workspace, eframe window, docking, settings, logging, commands, theme |
| **M1 – Boundaries & map view** | Import Shapefile/GeoJSON with CRS detection, topology, map view with pan/zoom/LOD, hover, selection, labels, layer list |
| **M2 – Data** | CSV/Excel import with type inference, data grid, key matching with diagnostics, calculated columns, filters, aggregation |
| **M3 – Analyses** | Choropleth with classification and legend, symbols, charts, heatmap, labels, tooltips |
| **M4 – Territories** | Plans from scratch or from an assignment table, painting, KPI panel, hierarchy, locks, sites, scenarios |
| **M5 – Optimization** | Balance, reorganize, districts for sites, top-down, location optimization, colour optimization |
| **M6 – Output** | PNG/SVG/PDF export, print layouts, territory reports, data and geodata export, interactive HTML map |
| **M7 – Geo services** | Geocoding, drive-time matrices and isochrones (OSRM/Valhalla), boundary downloads, background tiles |
| **M8 – Automation & polish** | CLI, templates, packaging, docs, German UI |

---

## 5. Features

### 5.1 Application Shell (M0)

- [x] Cargo workspace with the crates above
- [x] `eframe` app with the `wgpu` backend, GL fallback, crash log
- [x] Dockable layout (`egui_dock`): contents panel, map tabs, table tabs, inspector, territory panel (map and table tabs dock; Contents and Properties/Territories are side panels)
- [x] Save/restore layout and open tabs (tabs and their layout are saved with the project; panels with the app)
- [x] Main menu, toolbar, status bar (cursor coordinates, scale, selection, background tasks)
- [x] Command palette (Ctrl+Shift+P) with fuzzy search over all commands (word search)
- [x] Keyboard shortcuts for all commands, configurable (Settings → Keyboard shortcuts: every command, click and press the new keys, Delete for none; keys taken from another command move; kept in the settings file)
- [x] Light/dark themes (Pythia's palette)
- [x] Settings dialog (theme, size, autosave, background of new maps, recent projects, tile cache)
- [x] Welcome screen: recent projects, new project, open sample, getting started (no sample project yet)
- [x] Project: new, open, save, save as; recent projects
- [x] Autosave and crash recovery (autosave to the data folder; no restore prompt yet)
- [x] Undo/redo for every project change
- [x] Context help (F1) and tooltips that explain, not just name (F1 opens searchable help on the topic of the selection: getting started, layers, data, formulas with all functions, colouring, symbols, territories, scenarios, selecting, views, exports, keyboard shortcuts)
- [x] Remember where the user left off (map view, selected layer, open tables) (also the current map, active plan and territory)

### 5.2 Boundaries and base geometry (M1)

- [x] Import ESRI Shapefile (`.shp/.shx/.dbf`, `.prj`, `.cpg`), polygons, lines, points (also inside zip files)
- [x] Import GeoJSON (Feature, FeatureCollection, Geometry; mixed types split into layers)
- [x] Import GeoPackage (every feature table a layer, also inside zips; CRS from gpkg_spatial_ref_sys; 2D of Z/M geometries; no curves)
- [x] Import FlatGeobuf, KML (optional) (KML and KMZ: placemarks with name, description, folder and extended data, split into points, lines and areas; also inside zips; no FlatGeobuf yet)
- [x] Attribute encoding: `.cpg`, UTF-8 detection, legacy code pages (Windows-1252, 850, …)
- [x] CRS from `.prj` WKT: EPSG authority, name lookup against the EPSG table, WKT parameters
- [x] CRS from EPSG code or proj string; user override when unknown or wrong
- [x] Coordinates that look like longitude/latitude are recognized without a `.prj`
- [x] Key column choice on import, suggested from uniqueness (codes over names over row numbers)
- [x] Layer properties: name, key column, name column, source, CRS, vintage, feature count (no vintage)
- [x] Topology: shared arcs, neighbours, shared border lengths, enclaves (proximity fallback for files whose borders don't share vertices)
- [x] Multi-part areas, holes, enclaves handled everywhere
- [x] Derived layers: dissolve by a key prefix or an attribute (e.g. 4-digit codes → 2-digit regions); areas sharing a key merged on import
- [x] Point layers from tables (coordinates, geocoded addresses, area centroids) (coordinates in any CRS; no geocoding or centroids yet)
- [x] Voronoi areas for point layers (customers as planning units)
- [x] Download open boundaries (e.g. geoBoundaries ADM0–ADM2 for any country) with licence shown (all levels; credited on the map)
- [x] Switch a layer to a newer vintage, carrying data and assignments over (by key, or geometrically) (Data → Replace Boundaries: by key, else the old area under the new one's centre; pins and calculated columns too; one undo step)
- [x] Geometry embedded in the project (no "map not found" after moving the file)

### 5.3 Map View (M1)

- [x] Map tabs; several maps per project
- [x] Pan (drag), zoom (wheel, pinch, double-click, box zoom with Shift), zoom to layer/selection/all (no box zoom; zoom to all and to a territory)
- [x] Display projection: Web Mercator by default, any EPSG code per map (Web Mercator, local equal-area around the data, or any EPSG code; background maps in Web Mercator only)
- [x] Level-of-detail rendering: simplified arcs per zoom level, GPU meshes, viewport culling
- [x] Hover highlight with a tooltip card (name, key, data values)
- [x] Selection: click, Ctrl+click, rectangle, lasso, radius; by list (ranges like `01-09, 15`) (Select tool: click, Shift/Ctrl+click toggles, drag a rectangle, Alt+drag a lasso; list dialog with ranges and wildcards; no radius yet)
- [x] Transfer a selection to another layer (contained / touching / centre inside) ("Select in" on the selection bar: areas wholly inside, with their centre inside or touching; points inside; from points the areas they lie in)
- [x] Labels with collision avoidance, placed at a pole of inaccessibility
- [x] Layer list with visibility, order (drag), opacity, zoom range (order by context menu; no zoom range)
- [x] Background maps: XYZ raster tiles (OSM-style, configurable URL and attribution), cached
- [ ] Vector background (MapLibre styles, PMTiles/MBTiles offline) (optional)
- [x] Scale bar, north arrow, coordinates in the status bar (decimal or DMS, any EPSG) (scale bar and decimal degrees)
- [x] Measure distance and area (distance along clicked points; no area yet)
- [x] Search box: areas by key or name, table rows, addresses (when a geocoder is set up) (areas, points and territories by key or name, Ctrl+F; no addresses yet)
- [x] Named views (bookmarks), "focus on" a set of areas (island view) with dimmed surroundings (views kept per map; Focus in the selection bar fades the rest)
- [x] Fullscreen presentation mode (F11: full screen, map only; Esc leaves)

### 5.4 Data (M2)

- [x] Import CSV/TSV/TXT: delimiter, quote, encoding and decimal separator detected, previewed, overridable
- [x] Import Excel (xlsx, xls, ods): sheet or named range, header row detection (sheets; no named ranges)
- [x] Paste a table from the clipboard (Ctrl+V outside text fields opens the import dialog with the cells)
- [x] Column types inferred with preview: text, number, integer, boolean, date, key/code (text that keeps leading zeros), coordinate
- [x] Change a column's type after import
- [x] Linked tables: re-read from the source file on demand or on open; schema changes reported (on demand: Reload keeps links, column kinds and calculated columns; added/removed columns and broken links reported; not on open)
- [x] Data grid: virtualized, sort, filter, column statistics, sum row, search, copy (sort and search)
- [x] Edit cells of imported tables; add/delete rows (double-click a cell; rows added and deleted in data tables, cells only in layer tables)
- [x] Calculated columns with one expression language (see 5.5) (on tables and layer attributes; the formula is kept and dependent columns recalculate)
- [x] Lookup columns (value from another table by key, with aggregation for several matches) (Columns from… → Another table by key; the best key matching is chosen)
- [x] Spatial columns: "lies in" (area key/territory of a point), "count of points in", distance between coordinates or to nearest site (Columns from…: area a point lies in, points in an area, nearest point of a layer with distance; distance between coordinates by formula)
- [x] Score column: weighted combination of columns (absolute or as share of total) (⚖ Score: weights made to add up to 1, each as share, rank or as is; written as a formula to adjust)
- [x] Queries: join, union, group by with aggregations (sum, mean, min, max, median, mode, first, last, list, count rows, count values), filter before and after grouping (Summarise: group by one or more columns, sum/mean/median/min/max/count/first, filter before; joins via Columns from…; no union or filter after grouping)
- [x] Spatial join: points get attributes of the area they lie in (areas get counts and sums of the points inside)
- [x] Saved filters (segments) usable by analyses, reports and exports; one clear meaning (a segment is a yes/no column, usually calculated: "Only where" picks one or keeps the typed filter as one; tables show and export a segment's rows; not yet in reports)
- [x] Table ↔ map coupling: hover/select rows highlights areas and the other way round (clicking rows selects their areas, Shift toggles; rows of the map's selection are highlighted and can be filtered; not on hover)
- [x] Data export: CSV, XLSX (one sheet per table or files), filtered or all (plans, tables and layer attributes with each row's territories; not filtered yet)
- [x] Coordinates in degrees-minutes-seconds and "lat, lon" in one column parsed on import (decimal pairs, DMS with N/S/E/W before or after, GPS degrees and minutes; suggested when a column looks like that)

### 5.5 Expression language (M2)

- [x] Locale-independent syntax: `[Column Name]`, `"text"`, `1.5`, `+ - * / ^ %`, `& ` (concat), `= <> < <= > >=`, `and or not`, `null`
- [x] Functions: `if`, `ifnull`, `coalesce`, `case`/`switch`; math (`abs round floor ceil sqrt pow log exp min max clamp`); text (`left right mid len lower upper trim replace find contains startswith endswith like regex pad split`); dates (`date year month day weekday dateadd datediff format`); conversion (`num text bool`); `rank`, `percentile`, `share` (of column total), aggregates over a column (`sum([x])`) (all but `regex`; column functions take an optional group: `sum([Revenue], [Region])`)
- [x] Spatial functions: `distance(lat1, lon1, lat2, lon2)`, `dms(text)`
- [x] Typed checking with clear error messages pointing at the position (underlined in the editor, with "did you mean")
- [x] Autocomplete for columns and functions; live preview on the first rows (suggestions under the editor, Tab takes the first; syntax colours)
- [x] Text templates for labels, tooltips, legend titles: `{Name}: {sum([Sales])}` (labels and hover cards, with the layer's columns, linked tables (summed) and points inside; not legend titles yet)

### 5.6 Linking data to geometry (M2)

- [x] Link a table to a layer by key column; the key column is suggested from value overlap
- [x] Match modes: exact, ignore case/spaces, numeric (leading zeros), prefix roll-up (5-digit codes onto 2-digit regions)
- [x] Match report: share of rows matched, share of areas with data, duplicates; lists of unmatched rows and areas with likely fixes (padding, prefix) (unmatched samples and a tip; no list of areas without data)
- [x] Several links per table, several tables per layer
- [x] Aggregation when several rows hit one area (sum, mean, count, …), chosen per use
- [x] Place rows as points: coordinate columns, geocoded address, or area centroid by key (coordinate columns, geocoded addresses)
- [x] Geocoding (M7): Nominatim/Photon/Pelias/custom endpoint, batch with progress and resume, cache, quality column (Nominatim and Photon, public or own; structured or one-column addresses; no Pelias)

### 5.7 Analyses (M3)

- [x] Drag a column onto the map → suggested analysis with defaults and legend; analysis gallery to switch type (table column headers drag onto the map or a drop strip in Contents: numbers as colours (circles on points), texts as categories, linked tables through their link; "Show as" switches colours/circles/heat map keeping the value)
- [x] Choropleth (area colouring): classified, continuous, colour from a column, unique values (no colour from a column)
- [x] Classification: quantiles, equal interval, natural breaks (Jenks), standard deviation, mean split, equal sum, unique values, manual; rounded breaks; class count; range limits (manual limits by dragging or typing; no range limits yet)
- [x] Class editor with a live histogram and draggable breaks; class labels; "no data" and "not classified" styles (limits dragged in the histogram or typed; own labels per class; a colour for areas without data)
- [x] Colour ramps: sequential, diverging, qualitative (ColorBrewer-like, colour-blind safe marked), custom 2–5 colour ramps, reverse, opacity (no custom ramp editor)
- [x] Normalization: per area size, per another column (rates), share of total ("Relative to" on area colourings: per km², per another value, % of the total)
- [x] Symbols (points): colour, size (by area or radius, sqrt/linear/log), shape — each from a column, or one column for all (size from a column by area, width or logarithm; one colour or a colour per category of a text column; circle, square, diamond or triangle for all; no shape per category yet)
- [x] Symbols per area (aggregated) or per row (circles at area centres from attributes, linked tables or points inside; or on each point; relative to area/another value/total; size legend; in exports)
- [x] Charts per area or row: pie (and donut), bars, stacked bars, paired bars (population pyramid), with size scaling (pie, donut, bars and stacked bars from any numbers of the layer or linked tables; pies sized by their total, stacks as tall as it; values in the hover card, legend and exports; no paired bars yet)
- [x] Heatmap / density: kernel (Gaussian, linear, uniform), radius in km or pixels, weight column (quartic kernel, spread in km, optional weight column, sequential ramps; computed for the view; embedded in exports)
- [x] Hex or square grid aggregation of points (equal-area hexagons over points or areas, points counted and summed per cell; usable as territory units; no square grid)
- [x] Bivariate choropleth (two variables, 2×2/3×3 colour matrix) with quadrant labels (quantile classes, three colour grids, swap; legend grid tells ranges and counts on hover; in exports and web pages; no quadrant labels yet)
- [ ] Hatching for several values per area / overlapping territories
- [x] Lines: style a line layer by data (colour by value or category like areas; width from any number between two widths, on screen and in exports)
- [x] Flows: origin–destination lines with width/colour by data, curved, arrows (rows of a table or layer; each end the row's own point or a column naming a feature by key or name, guessed; equal pairs add up; width by a number or row count; one colour; in exports)
- [x] Radius/catchment circles around points, several rings; count and sum of what lies inside (around points or territory sites; points counted and summed, areas' values summed; a results table; labelled rings in exports)
- [x] Data-driven labels (text templates, font size/colour by data) (templates with several lines; font size by any number between two sizes, bigger labels placed first, on screen and in exports; no colour by data yet)
- [x] Tooltips: rich cards from a template; several analyses at one spot listed together (no templates)
- [x] Legends generated from the analysis; a few templates (list, bar, histogram, size); edited labels stay in sync (list and gradient in Contents, histogram in Properties)
- [x] Analysis-wide filter (segment) and "ignore filter" (per analysis: "Only where" with a formula over the linked table's or points' rows; no shared named segments yet)
- [x] Zoom-dependent visibility and symbol scaling (each map layer shown only closer or farther than a zoom taken from the view, shown as 1:n; greyed in Contents when hidden; exports follow; symbols keep their screen size)
- [x] Save/load class sets and ramps for reuse across projects ("Class sets" in the class editor keeps breaks (fixed as they are), colours and labels by name in the settings, and applies them to any coloured layer)

### 5.8 Territory planning (M4)

- [x] Plan (territory structure) on any area or point layer; several plans per project (scenarios)
- [x] Create a plan: empty, from an assignment table (unit key → territory code/name), from a customer list (customer → rep, rolled up to units), from site locations (empty or from an assignment table)
- [x] Assignment lists with ranges, enumerations, prefixes and wildcards (`10-19; 2*; 3301`)
- [x] Match report for assignment tables like 5.6
- [x] Territories: stable internal id, code, name, colour, weight/capacity, manager/site, notes (details of the active territory in the panel: name, code, size, lock, site, notes)
- [ ] Hierarchy: any number of levels (e.g. territory → region → area); levels built from the level below or from other boundaries ("borders like layer X") (one level of regions above territories: grouped automatically (balanced, contiguous) or by hand, with wider borders, figures and exports; no deeper levels yet)
- [x] Paint tool: click or drag over units to assign them to the active territory; Shift to pick a territory from the map; Alt to unassign
- [x] Assign by rectangle, lasso, radius, list, table selection (a bar over the map assigns or unassigns the selection, shows its measure total, zooms to it; no radius or table selection yet)
- [x] Move a whole territory into another parent; drag in the territory tree (Regions list each region's territories as chips; drag one onto another region or "no region"; also the Region column of the plan table)
- [x] KPI panel: per territory sums of chosen measures, target (mean or set), deviation in % with bars, unit count, area, parts (contiguity), compactness; totals; updates live while painting (no compactness figure)
- [x] Measures: any numeric column of linked tables (sum/mean/count per territory), unit count, area, customer count; several at once (one in the panel; the model and optimizer take several)
- [x] Tolerance band per measure (e.g. ±10 %); out-of-band territories flagged on map and in the panel (in the panel)
- [x] Territory table: editable code/name/colour/weight/locks; per-unit assignment table (plan ⋯ → Open as table: code, name, region, size, lock editable, figures sortable; the layer's table shows each area's territory; colour in the panel)
- [ ] Multiple assignment (a unit shared by territories, with shares) shown hatched (optional per plan)
- [x] Locks: pin a unit's assignment, freeze a territory, freeze a territory's outer border (children may move); shown on the map; respected by every tool and the optimizer (territory locks and unit pins; no border freeze, no map display)
- [x] Sites (rep homes, branches): place by coordinates, address or unit centroid; link to territories (1:1 or many); capacity (one site per territory, placed on the map or at the centre; no capacity yet)
- [x] Generate sites at territory centroids; move sites; "site outside its territory" warning
- [x] Territory borders drawn from the topology (outer and inner borders, per level)
- [x] Unassigned units: highlight, auto-assign to nearest/adjacent territory
- [x] Rename territories after their biggest place or site; renumber by position (largest area by the measure or by size; codes north to south; not after sites yet)
- [x] Scenarios: copy a plan, compare two plans (moved units, KPI deltas), adopt changes (⋯ menu → Duplicate as scenario; the panel compares, marks moved areas on the map, adopts into the original)
- [x] Change log: what moved since a snapshot ("143 customers change rep") (a scenario copy is the snapshot: moved areas, points in them per point layer, Excel export of the moves and per-territory figures; no automatic snapshots)
- [x] Rename, delete and export a plan from the Territories panel; one plan shown on the map at a time
- [ ] Analyses on plans: distance zones (unit to own site), catchment (potential within distance), key units (top units per territory), coverage (sites grab units until capacity) (distance zones: km to the own site as a value for colourings, farthest area per territory; key units in the report; no catchment or coverage yet)
- [x] Point-in-territory column for any table ("which rep owns this customer") (live column per plan in the table view, for linked tables and points in the plan's areas; in table exports and as extra sheets of the plan workbook)

### 5.9 Optimization (M5)

- [x] Balance: improve an existing plan with as few moves as possible
- [x] Reorganize: new territories from scratch for a given count
- [x] Districts for sites: territories around given sites (max distance, capacity) (sites stay in their territory and distance to them counts; no max distance or capacity)
- [x] Top-down: split each parent into n children (count given or from a target value) (a territory split into 2–5 contiguous, balanced territories; merge two territories; no target-driven count yet)
- [x] Several measures at once with weights and tolerance bands (no hand-built score needed) (optimizer; the panel sets one)
- [x] Compactness and contiguity: contiguity as a hard constraint (default) or soft; compactness weight
- [x] Respect locks, weights/capacities, unassignable units
- [ ] Distance model: straight line (default), drive time/distance from a matrix (M7), barriers (units not adjacent across)
- [x] Runs in the background with progress, live preview, cancel, deterministic seed (no live preview)
- [x] Result as a proposal: KPI before/after, moved units listed, accept or discard (counts, not a list)
- [x] Diagnostics instead of silent failure (a unit alone exceeds the target, too few units, negative values) (also areas without data and islands; each with a button that selects the areas)
- [x] Location optimization: place sites at (weighted) centroids or p-median over candidate points; match sites to territories (assignment problem) (sites at the centre of the measure, in the nearest area; sites from a point layer matched to territories with least total distance (Hungarian method); the best N of many candidate locations (p-median, greedy + swaps))
- [x] Colour optimization: neighbours get different colours; palettes; hierarchical colours (children shades of the parent) (neighbours differ; one palette)

### 5.10 Output (M6)

- [x] Export the map view as PNG (any DPI), SVG, PDF (with title, legends, scale bar, credits; no background tiles yet)
- [ ] Print layouts: page size/orientation, map frame(s) with scale, legend, title, text, image, scale bar, north arrow, inset map, table (A4/A3 landscape or portrait: title, logo, one map frame showing what the screen shows, legend beside or below, scale bar, north arrow, footer and credits; no free placement, inset maps or tables yet)
- [x] Text templates in layouts (`{project.title}`, `{date}`, `{territory.name}`) (page footers with {project}, {map} and {date}; territory names on report pages)
- [ ] Atlas/reports: one page per territory, area, filter value or column, or combinations; merged PDF or files; data tables per page (XLSX) (per territory: one PDF with an overview and a page per territory with its map, figures and largest areas; map atlas: the current map with a page per value of a column or per area, zoomed, others veiled, with legend and scale bar; no data tables per page yet)
- [x] Geodata export: GeoJSON, Shapefile, GeoPackage of layers, territories (dissolved) and points; KML (GeoJSON)
- [x] Plan export: assignment table (unit → territory per level), territory table with KPIs, assignment intervals (`1000-1099`), XLSX/CSV (no intervals)
- [x] Interactive HTML map (single file, offline): layers, tooltips, legends, search, filter (zoom, pan, hover cards with names, values and territories, legend and credits; no search or filter in the page yet)
- [x] Copy map as image to the clipboard (Ctrl+Shift+C, rendered like the export)

### 5.11 Geo services (M7)

- [ ] Provider settings (URL, key, rate limit) for geocoding, routing, tiles; keys in the OS keychain (geocoding and routing: server, profile and pause in Settings; tiles from presets; no keys or keychain yet)
- [x] Drive-time/distance matrices (OSRM `table`, Valhalla, GraphHopper), cached in the project (OSRM: minutes and km by road to the nearest point of another layer as columns of areas or points, asked in blocks with a pause; no Valhalla/GraphHopper, no full matrix for balancing yet)
- [ ] Isochrones around sites (several rings), as analysis and optimizer input
- [x] Routes between points; visit order (TSP) for a rep's customers ("Round" on the selection bar for 2–100 selected points: OSRM's trip gives the order, the road as a line layer with km and time, and a visit order column; starts at the first selected; for all territories of a plan on points from the plan menu, from each site)
- [ ] Workload: visit frequency × duration + travel time per territory as a KPI (travel: a round by road per territory from its site, with stops, km and hours per territory; visits × duration as a calculated column and measure; not yet combined into one figure)

### 5.12 Automation & polish (M8)

- [x] CLI: open a project, refresh linked data, export maps/reports/data, run an optimization (exports of maps (PNG, SVG, PDF, web page), reports and plan workbooks; no refresh or optimization from the command line yet)
- [x] Project templates (styles, layouts, analyses) and "new from template" (File → Templates: keep the project as a named template, start a new untitled project from one, delete; kept in the data folder)
- [x] Theme/corporate profile: default colours, fonts, logos (company name and logo (PNG, JPEG, SVG) on exported maps, web pages and reports; territory palettes per plan with a default for new plans; no fonts yet)
- [ ] German UI translation
- [ ] Packages for Linux (`.deb`, AppImage), Windows (MSI), macOS (`.dmg`)
- [x] User guide with a tutorial project built from open data (docs/guide.md: a first project from geoBoundaries cantons and a sample customer list geocoded by postcode and town, through colours, territories and exports; the window, data, selecting and settings; F1 help in the app)

---

## 6. Document model

```
Project (.territoria: zip with project.json + binary layer/table parts)
├── layers[]       geometry sources: id, name, kind (area/line/point), key column,
│                  attributes (table), CRS of origin, topology, vintage
├── tables[]       datasets: typed columns, calculated columns, origin (file, linked?)
├── links[]        table ↔ layer joins: key columns, match mode, statistics
├── plans[]        territory structures: base layer, levels, territories, assignment
│                  (unit → territory, pins), locks, sites, measures with targets/tolerances
├── maps[]         views: display CRS, extent, ordered map layers (boundary, analysis,
│                  plan, background tiles), legends
├── layouts[]      print pages and report definitions
└── settings       variables, metadata (title, author)
```

Differences to easymap on purpose: maps and print layouts are separate (no
"sheet → map" nesting for interactive work); territories have stable ids
apart from their user-visible codes (no hierarchical-number coupling);
assignment, base data and work tables are one table per plan; filters are
explicit segments with one meaning.
