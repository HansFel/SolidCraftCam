# 2.5D CAM for LinuxCNC

Open the command palette and choose **2.5D CAM — LinuxCNC**, or use the **Machining**
tab. The guided form creates
outside rectangular contours, rectangular pockets and hole patterns. All dimensions in
this form are millimetres, feed rates are mm/min and spindle speeds are rpm.

1. Choose an operation and name its setup and operation. Reusing both names replaces
   that operation; a different operation name appends to the setup. A new form chooses
   an unused tool number. Reusing a tool number updates that tool for all its operations.
2. Enter the cutter diameter, flute length, cutting depths, feeds and spindle speed.
   Top and bottom are absolute Z in the setup coordinate system. The bottom must be
   below the top; clearance must be at least retract, which must be at least the top.
3. Contours and pockets use the rectangle's lower-left X/Y, width and height. The
   contour is offset outside by the cutter radius. Pockets use inward offsets and the
   specified stepover. Sharp internal corners retain the cutter radius.
4. For drilling, enter one `x y` hole centre per line. Stepdown is the maximum peck
   depth. Every peck retracts to the retract height for chip clearance; optional dwell
   occurs at the final bottom only. The drill's flute length must cover the depth.
5. Click **Preview toolpaths**. The preview shows the current operation in work XY:
   accent colour is cutting, grey is rapid travel. Z motion is projected to XY. Changing
   an input invalidates the old preview. **Copy LinuxCNC G-code** copies the complete
   setup's program, including existing operations in that setup.
6. To write G-code, enter an output path ending in `.ngc` and click **Export G-code**.
   This computes the current form and exports the complete setup on an isolated copy;
   existing output files are refused. Press **OK** to store the CAM job in the open
   design as one undo step. Save the design to retain its job. Export and saving the
   CAM job are separate actions, so Cancel does not undo an explicitly requested export.

The LinuxCNC post explicitly sets millimetres, XY plane, absolute coordinates,
incremental arc centres, feed per minute, exact path mode and rpm spindle control.
Work offset 1–6 selects G54–G59. Tool changes use `Tn M6` and `G43 Hn`, so LinuxCNC's
tool table must contain the corresponding tool length offsets. The setup's work
offset and fixture clearance must be established on the machine before running.
The program retracts between operations, stops the spindle and coolant, and ends
with M30. Drilling is expanded to G0/G1 moves and G4 dwell, without modal canned cycles.

Every generated program is read back and its cutting geometry compared with the
planned toolpaths before it can be exported. This checks numerical postprocessing;
it is **not** a stock-removal, fixture, machine-limit or collision simulation. The
preview is a tool-centre path, not a simulation. Inspect the program in LinuxCNC's
preview and verify tool table, work zero and clearance for the actual machine.

## Setups

A setup is one clamping of the part. **New setup** (Machining tab, or the job list) and a
setup's **Edit** button in the job list open the setup form:

- **Work offset** G54–G59.
- **Part zero**: a **Picked point** (a corner, or an edge: a round edge gives its centre, so
  part zero can sit on a bore's axis; a straight edge its middle), **Picked per axis** (a
  corner, edge or face for **X zero**, **Y zero** and **Z zero** each; every pick moves zero
  along its own axis only; **Axes without a pick** sit on the model's or the stock's box,
  low, middle or high, or, when editing, stay **As before**), the **Model origin**, or a
  corner, face middle or centre **On the model's box** or **On the stock's box**, chosen
  low, middle or high per work axis.
- Axes: pick a flat face and +Z points out of it; pick a straight edge and +X runs along it.
  **Turn Z round** and **Turn X round** reverse an axis once. Nothing picked keeps the axes.
- **Stock**: a box around the model with allowances at the sides, on top and underneath, a
  box of fixed size, or a round bar of fixed size, centred on the model with its top at the
  model's top.
- **Machine**: one of your machine list (or one only the design has); it is copied into the
  design with the setup.
- **Material** and **Highest spindle speed (rpm)** for the cutting data.

**Delete** in the job list removes a setup with its steps (undo brings it back).

While the CAM window or the setup form is open, each setup shows its part zero with its
axes (red X, green Y, blue Z) and the stock's outline in the viewport, and every step its
toolpath: cutting moves in the preview colour, rapid moves thin, steps left out of the
G-code dimmed. OK in the milling form opens the job list on the new step. The form's
**Setup** list chooses the setup a step goes into; **New setup…** names a new one. Part zero and axes are
worked out when the setup is saved; after a model change, open the setup and press OK again.

## Contours and start points

Instead of a rectangle, a contour or pocket can follow picked geometry. Under
**Contours**, pick edges or planar faces in the viewport. A picked edge adds the one
contour it lies on (**Edge contour**); a picked face adds its outline and holes
(**Face with its holes**). Each entry can be removed again; a contour picked twice is
used once. Under **Start point**, pick a corner or an edge (its middle): each contour then
starts at its point nearest to it, otherwise at a corner.

**Side** and **Direction** sit right below the picked contours. **Side** says where the tool rides:

- **Automatic**: the part stays whole: around an outline and around the foot of a boss, inside
  a hole picked on its own.
- **Outside the contour** and **Inside the contour**: outside or inside each picked contour,
  a hole's too. Inside a picked outline cuts an opening; inside a hole follows its wall.
- **On the contour**: the tool's centre on the line.

**Direction** is **Climb** (with a clockwise spindle the wall is on the tool's right) or
**Conventional**, for contours and pockets.

**Wall stock to leave** keeps that much material on the walls and **Floor stock to leave**
stops the last pass that far above the bottom, so a later finishing step can take the rest:
rough with stock left, then finish with a second contour at 0.

## Start and final depth from the model

**Start depth Z** is where cutting starts. It begins at the top of the setup's stock; when an
earlier step has already taken material off, pick a face, edge or corner under **Start depth**
and the field takes its height. Retract and clearance heights below the start depth move up
above it. **Final depth** works the same way for **Bottom Z**. Both fields stay free to change;
a new pick sets them again. Heights are in the setup's work frame (the model's frame for a
setup not made yet).

**Stepdown** and **Stepover** take millimetres or a share of the tool diameter: `100 %`
(the stepdown's default) is one diameter, `50 %` (the stepover's) half of it.

## Facing

**Facing** (Machining tab, Milling) faces the setup's whole stock flat: from the start depth
(the stock's top) down to the final depth (part zero by default) in passes no deeper than the
stepdown. Each pass runs lanes along X, back and forth, no further apart than the stepover;
every lane starts and ends with the tool clear of the stock by **Overrun**
(default 2 mm), and the first and last lanes put the tool's edge that far past the stock's
sides, so no rim is left. The tool goes down beside the stock, never into it. Give the stock
an allowance on top in the setup, or there is nothing to face.

## Drilling at picked bores

For **Drill**, pick round edges (each gives its centre) or a flat face (every round hole in
it) instead of typing hole centres; the typed list is used only while nothing is picked.
The centres are taken in the setup's work XY when the step is saved.

## Job list, tool library and machines

On the **Machining** tab, the **Setup** panel opens **Job list**, **Tool library** and
**Machine list**, three tabs of one window.

The job list shows each setup with its **Material** and **Highest spindle speed
(rpm)**. Its steps run from top to bottom: **Up** and **Down** change the machining
order, and a step whose box is cleared is left out of the G-code. **Edit** opens a step in
the milling form with its values, tool and geometry (shown as kept until new contours are
picked); OK replaces it in place. **Remove** deletes a step; **Export G-code…** writes the whole setup.

The tool library is kept in the preferences, for every design. **Use in design**
copies a library tool into the open design; **Add to library** goes the other way and
lists the design's tools that are missing from the library or differ from it. A tool
holds cutting data per material group (aluminium, steel, stainless steel,
brass/bronze, plastic, wood/MDF): vc in m/min, fz in mm per tooth (a drill: per
revolution), and ap and ae as shares of the diameter. Empty fields use grey starting
values for solid carbide tools; check them against the tool maker's data. From them:

- spindle speed n = vc · 1000 / (π · D), limited to the setup's highest spindle speed;
- feed vf = n · z · fz (a drill: n · fz); plunge feed vf / 2 (a drill: vf);
- stepdown ap · D, stepover ae · D.

In the guided form, **Tool from library** and **Material** choose the tool and the
material group, and **Use cutting data** fills spindle speed, feeds, stepdown and
stepover. Running the job copies the library tool into the design.

The machine list is kept in the preferences, like the tool library, so a machine is made
once and every design can use it; a name already on the list is refused for a new machine.
A machine has a name,
the travel of each axis as its lowest and highest position (X−, X+, Y−, Y+, Z−, Z+), an
optional A axis (limited to A− … A+ degrees, or endless), the slowest and fastest spindle
speed and the fastest feed (mm/min). With a machine on the setup:

- **Use cutting data** and `cam.tool.speeds` hold the spindle speed to the machine's range
  (the feeds follow the speed actually reached) and every feed to the fastest;
- exporting refuses a program whose spindle speed or feeds leave the machine's range, or
  that reaches further along X, Y or Z than the machine travels. Where part zero sits on the
  table is set at the machine, so the check is by reach, not by position. The A axis is
  recorded for later 4-axis work; 2.5D programs do not use it.

## Programmatic commands

`cam.job.preview` and `cam.job.create` share an input schema. The former leaves the
design and undo history unchanged; the latter stores tools, setup and operation as
one undoable command. Both return LinuxCNC G-code, preview points and cutting time
(without rapid, acceleration or tool-change time).

```json
{
  "setup": "Top",
  "name": "Pocket",
  "kind": "pocket",
  "tool": 1,
  "diameter": 6,
  "flute_length": 20,
  "x": 0,
  "y": 0,
  "width": 40,
  "height": 30,
  "top": 0,
  "bottom": -3,
  "clearance": 10,
  "retract": 2,
  "stepdown": 1,
  "stepover": 2,
  "feed": 600,
  "plunge_feed": 150,
  "spindle_rpm": 6000,
  "work_offset": 1
}
```

For drill jobs set `kind` to `drill` and add `points: [[10, 10], [30, 10]]` and
optionally `dwell` in seconds. The rectangle values remain required but are ignored.

Export an existing setup with `cam.export`:

```json
{"setup": "Top", "path": "part.ngc", "overwrite": false}
```

Only explicit `overwrite: true` replaces a file, keeping its previous contents in
`.bak`. Writes are atomic and obey the engine's file-access scope. In the browser
they use the virtual filesystem and the host's download handling.

The lower-level commands `cam.tool.define`, `cam.setup`, `cam.op.contour`,
`cam.op.pocket`, `cam.op.drill`, `cam.op.remove`, `cam.tool.remove`, `cam.machine.define`,
`cam.machine.remove`, `cam.list` and `cam.post` are also available through scripts, CLI and the control channel. Contour
and pocket commands additionally accept sketch profiles, planar faces and polygon
regions with islands, and expression-based depths and feeds. Use `cam.post` with
`post: "linuxcnc"` for LinuxCNC output; its existing default remains `rs274`.

Picked geometry: `faces` entries may name one contour with `contour_at` (a model point on
it), `edges` (`[{body, at}]`) add the contours through those edges, and a contour's
`start` is a model point. `cam.tool.define` takes `cutting`
(`{"aluminium": {"vc": 300, "fz": 0.05, "ap": 0.5, "ae": 0.4}}`); `cam.setup` takes
`material` and `max_rpm` (null clears either). `cam.setup` also takes picks on the model:
`z_face` and `x_edge` (`{body, at}`), `zero_edge` (a round edge's centre, else its middle),
`flip_z` and `flip_x`, `zero_axes` (`{x?, y?, z?: {body, at, edge?}}`), `wcs_origin_mode` (`selected_point`, `model_origin`,
`model_box_point`, `stock_box_point`) with `box_point` (`{x, y, z}`, each `negative`,
`center` or `positive` in the work frame) and `stock_spec`
(`{"kind": "relative_box", "mode": "sides_and_top_bottom", "side_offset": 2, …}`,
`{"kind": "box", "width", "depth", "height"}` or `{"kind": "cylinder", "diameter", "height"}`).
`cam.op.face` faces the stock (or an `outer` rectangle) with `stepover` (default 70 % of the
diameter), `overrun` (default 2 mm), `top` (default the stock's top) and `bottom`; the guided
job takes `kind: "face"` with `overrun`. Without `stepdown`, milling steps down one tool
diameter. Contours take `side` (`auto`, `outside`, `inside`, `on`); contours and pockets take `climb`
(false: conventional), `stock_to_leave` (walls) and `floor_stock` (floor), in the guided job
too. Machines:

```json
{"name": "Portal", "x": [0, 600], "y": [0, 400], "z": [-120, 0], "a": {"kind": "endless"},
 "spindle_min": 3000, "spindle_max": 12000, "max_feed": 2000}
```

`cam.machine.define` adds or replaces the design's machine of that name, `cam.machine.remove`
removes one no setup names, and `cam.setup` takes `machine` as a name, a whole machine
(copied into the design) or null. Machining order and skipped steps:

```json
{"setup": "Top", "name": "Holes", "to": 0}
{"setup": "Top", "name": "Pocket", "skip": true}
```

`cam.op.move` gives an operation a new place in the order (from 0) and `cam.op.skip`
leaves it out of posting or puts it back; both return the order and the skipped steps.
`cam.tool.speeds` (`{"tool": 1, "material": "aluminium"}`, or `setup` for that setup's
material and highest spindle speed) returns the spindle speed, feeds, stepdown and
stepover the cutting data give, without changing the design.

The local integration builds on the SolidCraft CAM core and document command work
from [#5751](https://github.com/tangency-labs/solidcraft/pull/5751) and
[#2881](https://github.com/tangency-labs/solidcraft/pull/2881). These are review-stage
project changes; this local branch does not imply their upstream acceptance.

LinuxCNC syntax reference: [official G-code reference](https://linuxcnc.org/docs/html/gcode.html).
