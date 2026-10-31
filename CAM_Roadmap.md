# SolidCraft CAM roadmap

Where SolidCraft's CAM (computer-aided manufacturing) side stands and where it goes: from
today's 2.5D milling for LinuxCNC to a complete CAD/CAM workflow that hobby users (GRBL,
LinuxCNC) and professional CNC users (Fanuc, Siemens, Heidenhain) can both follow. User
documentation of what works today: [docs/cam-linuxcnc.md](docs/cam-linuxcnc.md).

Status words: **Done** works in the `cam-linuxcnc` branch; **Partial** exists in the data
model or the engine but not yet everywhere (UI, posts, checks); **Planned** is not started.

## Principles

- **Clean room.** Behaviour comes from general CAM practice, public controller manuals and
  user feedback. No commercial CAM product is driven, screenshotted or copied for this
  work: not its texts, dialogs, icons, templates or tool and post libraries
  (`skills/solidcraft-ground-rules/SKILL.md`).
- **Generated code moves a machine.** Every program is read back and compared with the
  planned toolpaths before export. A check is only called a simulation once it removes
  material; until then the UI and docs say what is and is not checked.
- **Everything is a command.** Setups, tools and operations are `cam.*` commands with
  schemas, usable from the UI, scripts, the CLI and MCP. Programmatic calls never open
  dialogs.
- **Pure Rust, also in the browser.** CAM code lives in `crates/cam` (geometry and
  G-code), `crates/doc` (saved data) and `crates/engine` (commands), and builds for wasm.
- **Small PRs.** Each item below is one topic; a design-format change is one file in
  `crates/doc/src/format/pending/`.

## Today (branch `cam-linuxcnc`)

- 2.5D operations: contour (inside/outside, start point, picked edges and faces with
  holes), pocket (with islands, ramp), drill (peck, dwell).
- Setups with work offset G54–G59, work coordinate system (origin, X axis, tool axis),
  material, highest spindle speed, machining order and skipped steps.
- Tools: flat end mill, ball end mill, drill; cutting data per material group (vc, fz,
  ap, ae) with starting values; a global tool library with a copy in each design.
- Posts: LinuxCNC and generic RS-274; read-back check of every program; cutting time
  without acceleration.
- UI: Machining ribbon tab, guided milling form, 3D toolpath preview, job list and tool
  library panel.

## Phase 0: ready for upstream

| Item | Status |
|---|---|
| Remove the setup labels in `crates/doc/src/cam.rs` that followed one commercial product's dialog text too closely (they were unused; UI text lives in the locale files) | Done |
| Fix the dead-code warning in `crates/engine/src/slicers.rs` that blocks `cargo xtask ci` (pre-existing, separate PR) | Done |
| Split the branch into claimable PRs: CAM core, document format, commands, UI panel, tool library | Planned |
| Real screenshots of the panel and the form for each UI PR | Planned |

## Phase 1: complete 2.5D for hobby machines

### 1. Setup list

A setup is one clamping of the part. Several setups per part (machine the top, turn over,
machine the bottom).

| Element | Status |
|---|---|
| Setup name | Done |
| Several setups per part, each deleted again from the job list | Done |
| Part zero picked on the model (vertex, edge midpoint, circle centre, stock or model box point) | Done |
| Part zero picked per axis (X, Y and Z zero each from a corner, edge or face) | Done |
| Orientation picked on a face or edge (Z+ out of a flat face, X along a straight edge, either turned round) | Done |
| Part zero and axes follow later model edits (today they are worked out when the setup is saved) | Planned |
| Work coordinate system shown in the viewport (axes at part zero) | Done |
| Machine coordinate system and work offset G54–G59 | Partial: work offset done; the setup names its machine; machine zero is set at the machine |
| Stock assigned to the setup, rotated and shifted | Partial: box around the model, fixed box and round bar in the setup form; no rotation or shift |
| Fixtures assigned (bodies the tool must not touch) | Planned |
| Setup preview: model, stock, fixtures and part zero together | Partial: stock box and part zero; no fixtures |
| Stock continues from the preceding setup | Partial: `from_preceding` in the data model; needs simulation for the real rest |

Example:

```
Setup 1: top side
  part zero: top left corner
  orientation: Z+ normal to face A
Setup 2: turned over
  part zero: centre of bore B
  orientation: Z+ normal to face C
```

### 2. Machining tree

```
Project
├ Model
├ Stock
├ Fixtures
├ Tools
├ Machine
├ Setup 1
│  ├ Part zero
│  ├ Orientation
│  ├ Facing
│  ├ Pocket 1
│  ├ Drill
│  └ Contour
├ Setup 2
│  ├ Part zero
│  ├ Orientation
│  └ Operations…
├ Simulation
├ Tool list
├ Setup sheet
└ G-code
```

| Item | Status |
|---|---|
| Setups and their steps as a tree in the CAM panel | Partial: flat job list with order and skip |
| Drag and drop to reorder steps and move them between setups (`cam.op.move` gains a target setup) | Partial: Up/Down only |
| Edit a step in its form (values, tool and geometry; OK replaces it in place) | Done |
| Duplicate a step, rename it | Planned |
| Tree state saved with the design | Done (order and skipped steps) |

### 3. Tool management

| Item | Status |
|---|---|
| Flat end mill, ball end mill, drill | Done |
| Corner-radius (bull nose), chamfer, engraving (V-bit), T-slot, dovetail cutters | Planned |
| Spot (centre) drill, step drill, reamer | Planned |
| Tap and thread mill | Planned |
| Number, name, diameter, flute length, number of flutes | Done |
| Overall length, shank diameter, stick-out | Planned |
| Holder (geometry for collision checks, gauge length) | Planned |
| Tool material (HSS, carbide, coated) changing the starting values | Planned |
| Recommended cutting data per material group | Done |
| Import and export of the library as a file (own JSON format) | Planned |

### 4. Stock management

| Item | Status |
|---|---|
| Box, fixed or relative to the model (with offsets) | Done |
| Round stock and tube | Partial: round bar in the form; tube in the data model only |
| Stock from a body in the design or an imported STEP/STL | Partial: `from_solid` in the data model |
| Stock material group (aluminium, steel, stainless steel, brass/bronze, plastic, wood/MDF) | Done (on the setup) |
| Stock shown in the viewport | Partial: as a box outline; not yet a transparent solid |

### 5. 2.5D strategies

| Strategy | Status |
|---|---|
| Contour | Done |
| Pocket | Done |
| Drill (peck, dwell), at picked round edges or every round hole of a picked face | Done |
| Facing of the stock in passes, with overrun past its edges | Done |
| Facing a picked face's area to a finished height with a Z allowance, safety heights worked out | Done |
| Slot (along a line or arc) | Planned |
| Chamfer and deburr along edges (chamfer cutter) | Planned |
| Bore and helical bore | Planned |
| Thread milling and tapping (rigid or floating) | Planned |
| Engraving along sketch text and curves | Planned |
| Tabs (bridges) on through contours | Done |
| Lead-in and lead-out: tangent arc, tangent line, square line or direct/ramp, with overlap | Done |
| Open contours along an edge chain: side, direction, run-on at both ends | Done |
| Edge chains: picked edges joined into contours, and a chain found in its plane from one edge | Done |
| Final depth picked on the model (a face, edge or corner), then free to change | Done |
| Start depth picked the same way, the stock's top by default | Done |
| Stepdown and stepover in mm or % of the tool diameter (stepdown default one diameter) | Done |
| Contour side: automatic, outside, inside (openings and hole walls) or on the contour | Done |
| Climb or conventional milling for contours and pockets | Done |
| Stock to leave on walls and floor for a later finishing step | Done |

### 6. Posts

| Controller | Status |
|---|---|
| LinuxCNC | Done |
| Generic RS-274 | Done |
| GRBL (no tool changer, `M6` optional, limited canned cycles) | Done |
| Mach3 / Mach4 | Partial: Mach3 |
| Centroid | Done |
| Fanuc | Planned |
| Siemens Sinumerik | Planned |
| Heidenhain (conversational Klartext, not G-code) | Planned |
| Marlin: only for pen, laser and light routers driven by 3D-printer firmware | Planned, low priority |
| G-code preview in the panel, with the line that belongs to the selected move | Planned |
| G-code editor (view, search, edit before export; edits are not read back) | Planned |
| Post settings in the UI: comments, line numbers, decimals, arcs, canned cycles, tool change, safe start | Partial: `Post` struct has them, no UI |

All posts are written from the controllers' public programming manuals.

## Phase 2: checks, simulation and machines

### 7. Simulation

| Item | Status |
|---|---|
| Every step's toolpath shown while the CAM window is open | Done |
| Tool motion along the path (play, step, speed) | Planned |
| Rapid moves shown apart from cutting moves | Done (preview) |
| Material removal (dexel or voxel stock, shown in the viewport) | Planned |
| Collision of tool, holder and fixtures with stock and model | Planned |
| Machine kinematics (axes, travels) | Partial: travels checked by reach on export |
| Machining time with accelerations, rapids and tool changes | Partial: cutting time only |

### 8. Collision and rest-material analysis

Before each export:

| Item | Status |
|---|---|
| Read-back of the program against the planned toolpaths | Done |
| Rest material shown on the model | Planned |
| Areas no tool can reach marked | Planned |
| Collisions marked | Planned |
| A warning list per setup (shown in the panel, returned by `cam.post`) | Planned |

### 9. Machine library

| Item | Status |
|---|---|
| Personal machine list in the preferences, copied into the design with the setup | Done |
| Machine types: 3-axis mill, 4-axis mill, 5-axis mill, lathe, mill-turn | Planned |
| Travels (X, Y, Z, A limited or endless) with a check of every program | Partial: X, Y, Z reach checked on export; A recorded only |
| Tool changer: none (manual), carousel, number of pockets | Planned |
| Spindle speed range and fastest feed, held in cutting data and checked on export | Done |
| Default post per machine | Done |

### 10. Cutting-data calculator

| Item | Status |
|---|---|
| Spindle speed n = vc · 1000 / (π · D), held to the setup's highest and the machine's range | Done |
| Feed vf = n · z · fz, plunge feed, stepdown ap · D, stepover ae · D | Done |
| Chip thinning for small stepover | Planned |
| Spindle power Pc = ap · ae · vf · kc / (60 · 10⁶ · η), with kc per material group | Planned |
| Warnings when power or speed exceed the machine | Planned |

## Phase 3: feature recognition

| Feature | Status |
|---|---|
| Holes and threads from SolidCraft's own hole and thread features (exact sizes, no guessing) | Planned |
| Holes, counterbores and countersinks recognised on imported bodies | Planned |
| Pockets and slots | Planned |
| Outer contours | Partial: picking edges and faces |
| Suggested operations per feature, with tools from the library | Planned |

## Phase 4: 3D strategies

| Strategy | Status |
|---|---|
| Roughing (constant engagement) | Planned |
| Rest roughing | Planned |
| Parallel finishing | Planned |
| Z-level (waterline) | Planned |
| Scallop (constant cusp) | Planned |
| Pencil (corners) | Planned |
| Morphed spiral between two curves | Planned |
| Projection of 2D curves onto the surface | Planned |

## Phase 5: 4th and 5th axis

| Item | Status |
|---|---|
| Wrapped 2D paths on a rotary axis | Planned |
| Rotary machining (A axis continuous) | Planned |
| 3+2 positioning (tilted work plane per setup) | Planned |
| Simultaneous 5-axis | Planned, last |

## Phase 6: manufacturing documents

Generated from the design, also as PDF:

| Document | Status |
|---|---|
| Tool list (number, type, diameter, lengths, holder, used in which steps) | Planned |
| Setup sheet (part zero, orientation, stock, fixtures, picture per setup) | Planned |
| Setup list and fixture overview | Planned |
| Run time per setup and in total | Partial: cutting time per step |
| Material list (stock size and material group) | Planned |

## Turning and mill-turn

Lathe setups (`SetupOperationType::Turning`) are in the data model only. Facing,
turning, grooving, threading and parting on a lathe come after Phase 2, with their own
roadmap.
