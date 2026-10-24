# Territoria user guide

Territoria puts your data on maps of areas (postcodes, municipalities,
districts) and plans sales territories on them. This guide walks through
a first project with open data, then explains the parts of the window.
Press **F1** in the app for help on whatever is selected.

To look around first, click **Open the sample** on the welcome screen
(or **File → Open the Sample Project**): the Swiss cantons with about 400
made-up customers, coloured by their sales and divided into five sales
regions. It works offline; nothing is saved until you choose **Save As**.

## A first project in twenty minutes

You need an internet connection for the boundaries and for finding the
customers' places. The customer list is
[`docs/tutorial/customers.csv`](tutorial/customers.csv): thirty
made-up customers in Swiss towns, with a segment and their sales.

### 1. Boundaries

**File → Download Open Boundaries…** lists administrative areas of most
countries from [geoBoundaries](https://www.geoboundaries.org) (open data,
CC BY 4.0). Choose *Switzerland* and the level of the cantons (ADM1), then
**Download**. The cantons appear on the map and in **Contents** on the
left, under *Data*.

Your own boundaries work the same way with **Add boundaries**: Shapefiles
(also zipped), GeoJSON, GeoPackage, KML. Postcode areas are the usual
choice for sales territories.

### 2. Customers

**Add data** and pick `customers.csv`. The import dialog shows the
columns it recognised. Tick **Rows have addresses: find their
coordinates** and check the columns it suggests: postcode `Postcode`,
town `City`, country `Country`. Adding the table looks the places up
(Nominatim, one per second; the status bar shows the progress and a Stop
button). The customers appear as points, with a column saying how
precisely each was found.

Coordinates in one or two columns (`47.37, 8.54`) are placed without
looking anything up, and a table with a column of area keys (postcodes)
is *linked* to the boundaries instead, by **Link Table to Map**.

### 3. Show values

**Colour by value**, areas *cantons*, value **Number of customers**:
each canton is coloured by its class, the legend shows under the layer in
Contents, and the class editor in **Properties** (right) changes the
method, number of classes, colours and labels.

Try these on the same dialog or later from Properties → **Show as**:

- **Sales (customers)** as *Circles sized by value*.
- On the customers themselves: **Heat map**, weighted by sales.
- **Pie charts** of several numbers per area, or **Two values at once**.

Faster still: open a table (double-click it under Data), and drag a
column heading onto the map, or onto the strip that appears in Contents.

### 4. Territories

**New territories**, built from the cantons, with empty territories:
set **Territories** to 3 and **Create**. In the **Territories** panel:

- *Balance by* **Sales (customers)**, tolerance ±15 %.
- Under *Balance automatically*, choose **Start over** and **Balance**.
  The proposal shows on the map; **Apply** it.
- **Paint** (B) moves cantons by dragging over them into the active
  territory; the figures update as you paint. Lock a territory to keep
  it as it is when balancing again.
- **Sites** places each territory's office; **Names** names territories
  after their largest place.

Duplicate the plan (⋯ → **Duplicate as scenario**) to try changes
without losing the original; the comparison shows what moved.

### 5. Share

- **Map → Export Map**: PNG, SVG or PDF, as on screen or on an A4/A3
  page with title, legend, scale bar, north arrow and a footer. The
  background map comes along, as sharp as the export.
- **Map → Export as Web Page**: one HTML file that zooms and shows a
  card for each area on hover, to send to anyone.
- **Map → Export Atlas**: a PDF page per canton (or region, rep).
- The plan's **⋯** menu: a territory report (PDF), the assignment as
  Excel or CSV, the changes between scenarios.

**Save** keeps everything in one `.territoria` file.

## The window

| Part | What it is for |
|---|---|
| Toolbar | Open and save, add boundaries and data, colour by value, new territories, and the map tools: Navigate, Measure, Select, Paint |
| Contents (left) | The map's layers bottom to top with their legends; the data (layers and tables); the territory plans |
| Map (middle) | Drag to move, scroll to zoom; hover an area for its card. Tables open as tabs beside the map |
| Properties (right) | Settings of what is selected: a map layer's value, classes, colours, labels; a layer's key and name columns |
| Territories (right) | The active plan: territories with their figures, painting, balancing, sites, regions |
| Command palette | Ctrl+Shift+P: every command by name |
| Search | Ctrl+F: find an area or territory and go there |

## Data and formulas

- **Link** a table to boundaries by its key column; several rows per
  area are added up, counted or averaged.
- **ƒ Calculated column** computes a column from others:
  `[Sales] / [Customers]`, `share([Sales], [Region])`,
  `if([Sales] > 10000, "A", "B")`. Help → *Calculated columns and
  formulas* lists every function.
- **Segments** are yes/no columns such as `[Sales] > 20000`: tables show
  and export a segment's rows, and analyses count or add up *only where*
  a segment holds.
- **⊕ Columns from…** brings in the area each point lies in, the points
  in each area, another table's columns by key, or the nearest point of
  another layer, also by road (minutes and km from a routing server set
  in Settings).

## Selecting

**Select** (S): click, drag a rectangle, Alt+drag a lasso, or Ctrl+drag
a circle around a place (the radius shows in km). Shift adds. The
selection bar counts the selection, sums the plan's measure, assigns it
to the active territory, zooms to it, fades the rest (*Focus*), carries
it to another layer (*Select in*: the postcodes inside selected
districts) and, for points, plans a visiting **Round** by road.

## Settings

**File → Settings**: theme and size, autosave, the background map of new
maps, the geocoding and routing services, the company name and logo for
exports, territory colours for new plans, class sets kept for reuse, and
the keyboard shortcuts (click one, press the new keys).

Unsaved work is autosaved every few minutes. If Territoria closes before
you save, the welcome screen offers the kept copy next time: **Restore**
opens it as an unsaved project, **Discard** deletes it.

Services that need an **API key** (a commercial geocoder or routing
service, MapTiler or Thunderforest tiles) take it in the field under their
address. Keys are kept in the system keychain (macOS Keychain, Windows
Credential Manager, GNOME Keyring or KWallet), never in the settings file.
A key goes where the address says `{key}`, e.g.
`https://api.maptiler.com/maps/streets/{z}/{x}/{y}.png?key={key}`;
geocoding and routing addresses without `{key}` get it as the `key`
parameter.

## Working with an AI agent

Territoria can be driven by an AI agent such as Claude Code or Claude
Desktop through the Model Context Protocol. Register it once:

```sh
claude mcp add territoria -- territoria mcp
```

(in Claude Desktop, add a server with the command `territoria` and the
argument `mcp`). Then ask in plain words: *“Open sales.territoria, colour
the postcodes by revenue, make eight territories with the same revenue and
show me the map.”* The agent adds data, colours, plans, assigns and
exports with the same functions as the menus, looks at the map as a
picture, and saves the project; open it in Territoria to carry on.

## Open data credits

- Boundaries in the download dialog: geoBoundaries, CC BY 4.0.
- Geocoding: © OpenStreetMap contributors, via Nominatim (or Photon).
- Background maps: as credited on the map and in exports.
