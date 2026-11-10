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
planned toolpaths before it can be exported. This checks numerical postprocessing.
Posting also checks the tool against the model and the fixtures (see **Collisions** below);
it does not check the stock or the machine itself. The preview is a tool-centre path; the
material-removal simulation (below) works on heights over the work plane only. Inspect the program in LinuxCNC's
preview and verify tool table, work zero and clearance for the actual machine.

## Setups

A setup is one clamping of the part. **New setup** (Machining tab, or the job list) and a
setup's **Edit** button in the job list open the setup form:

- **Setup**: its name. Changing it on an existing setup renames it; its steps stay.
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
toolpath (a setup whose eye is closed in the job list hides its part zero and stock with its
toolpaths, so what is shown belongs together): cutting moves in the preview colour, rapid moves thin, steps left out of the
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

To mill along edges you pick one after another, tick **Edges as one chain**: the picked
edges themselves, seen from above, are joined end to end into closed contours. Pick them all
the way round. Edges that bound no flat face square to the tool axis (the foot of a fillet or
chamfer, a sloped face) are always taken this way. **Find edge chain** picks one edge's whole
chain for you: from the picked edges it follows the edges joined to them in their plane (square
to the tool axis for a single straight edge), taking the one that turns least at each corner,
until the chain comes round or ends. The suggestion shows as picked; take out what you do not want.

A chain that does not close is an **open contour**. It runs from the first picked edge towards
the second (**Reverse direction** turns it round). **Side (open)** is the side the tool rides,
seen along the cut: left, right or on the edge. **Run-on start** and **Run-on end** extend it
along its tangent before the start and after the end. Over several depths an open contour always
cuts the same way: lead-in, cut, lead-out, up to the retract height, back to the start.

**Lead-in** and **Lead-out** say how the tool gets onto a contour and off it again: **Direct /
ramp** (down on the contour, ramping along a closed one at the ramp angle, as before), a
**Tangent arc** (radius), a **Tangent line** in line with the contour, or a **Square line** at a
right angle to it (length). Leads come from and go to the side the tool rides on; the plunge is at
the lead-in's start, clear of the wall. A lead on a closed contour that would come nearer the part
than the tool radius is refused. **Overlap** runs a closed contour's cut on past its start before
the lead-out, so no mark is left where it began.

**Holding tabs** keep a part cut right round in the stock: **Tabs per contour** are spread evenly
round each closed contour (none on its start), each **Tab width** long and **Tab height** high
from the cut's bottom. On the passes below a tab's top the tool lifts over it at the cutting feed
and comes back down; cut the tabs off by hand afterwards. A tab as high as the cut, or tabs wider
than the gap between them, are refused; open contours take no tabs.

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

## Radius compensation on the control

**Radius compensation** says who moves the tool off a closed contour by its radius:

- **In the CAM** (the default): the program is the tool centre's path for the tool's nominal
  diameter, as before.
- **On the control (G41/G42)**: the program is the contour itself (moved out by the wall stock),
  and the control adds the radius from its tool table. A reground or undersized cutter then still
  cuts the part to size: enter its measured diameter in the tool table under the tool's number
  (`D` is the tool number).
- **On the control, wear only**: the program is the nominal tool's centre path, as with the CAM,
  and the tool table holds only the difference between the measured and the nominal diameter (a
  value near zero). Every control that has `G41`/`G42` runs this without complaint.

Controls are strict about compensation, and most trouble on the machine comes from it, so every
compensated depth pass is written the same way: compensation off, a rapid to the entry point
square off the contour on the tool's side, down to the pass depth there; `G41`/`G42` on a
straight entry move onto the contour, at least 1.25 tool diameters long (a **Square line** lead
makes it longer; other leads are refused); the contour at that depth with nothing else in
between (no Z moves, no dwells, no tabs); `G40` on a straight exit move as long as the entry;
then up. The entry lands in the middle of the contour's longest straight side, or nearest the
start point you picked, at least a tool diameter from a corner. Arcs are split below half a turn.
Ramps and holding tabs cannot be compensated on the control and are refused, as are open edge
chains.

Before anything is written the contour is offset here the way a control does it (lines moved
square to themselves, arcs to a smaller or larger radius, a round corner added at outside
corners, inside corners cut back to where the offsets meet). An arc or inside corner tighter than
the tool, and contours or walls closer together than the tool, are refused with the place named,
because the control would stop there with a gouging or interference alarm. The result must match
the path the CAM works out for the tool; the preview, the material removal, the collision check
and the run time all use that tool centre path, and the read-back compares the program with the
contour it says.

The built-in posts write `G41 D<tool>`, `G42 D<tool>` and `G40` (LinuxCNC, RS-274, Mach3,
Centroid, Fanuc), `G41`/`G42`/`G40` with the edge `D1` of the tool change (Sinumerik), or `RL`,
`RR` and `R0` on the `L` and `C` blocks (Heidenhain). GRBL has no cutter compensation: a
compensated step is refused with its post. Your own posts set the words (`comp_left`,
`comp_right`, `comp_off`, `{register}` for the register; all empty: none). The setup sheet says
for each compensated step what goes into the tool table.

Checked against LinuxCNC 2.9.10's interpreter (`rs274`, run as a program only): rectangles, an
L shape, a circle and a plate with a bore, outside and inside, climb and conventional, with the
tool table at the nominal diameter, 0.4 mm over and 0.4 mm under (wear: 0, +0.4 and -0.4). Every
program was taken without an error, and at the nominal diameter the moves it makes lie on the
path the CAM shows to within 0.005 mm. One difference: on the entry line LinuxCNC heads straight
for the corner of the offset entry line and the first offset contour piece, so the tool reaches
the contour's offset a little before the square entry point the preview shows; that corner is
on the waste side, a radius off the contour. Note for your own tests: `rs274` without an INI
file reads the tool table in inches.

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

**Facing** (Machining tab, Milling) faces an area flat in three choices:

- **Where:** pick a flat face, and the box round its outline is faced; the base (bottom face)
  faces the whole footprint. Without a pick, the setup's whole stock.
- **Finished height Z:** where the faced surface ends up (part zero by default). Type it, or
  pick a face, edge or corner under **Final depth**.
- **Allowance Z:** the material above the finished height. It starts as what the stock has
  above it (1 mm when that is nothing); a pick under **Start depth** sets it to that height.

Facing starts at finished height + allowance; the retract and clearance heights follow, 2 and
10 mm above that. It steps down in passes no deeper than the stepdown, each pass running lanes
along X, back and forth, no further apart than the stepover; every lane starts and ends with
the tool clear of the area by **Overrun** (default 2 mm), and the first and last lanes put the
tool's edge that far past its sides, so no rim is left. The tool goes down beside the area,
never into it.

## Slots

**Slot** mills along a path with the tool centre on it. Picked edges are the path
themselves, joined end to end into chains seen along the tool axis: an open chain (one
edge, an L) is milled from end to end, a closed one all round. A picked face or sketch gives
its contours instead, each milled all round. Without picks, the rectangle in the fields is
the path. **Slot width** left empty is the tool's diameter, one pass on the path; a wider
slot adds side passes, at most **Stepover** apart, until the walls are reached, with
**Wall stock to leave** on both walls. Depth passes follow **Stepdown**; a ramp angle above
0 (programmatic) ramps back and forth along an open path instead of plunging.

## Chamfers and engraving

**Chamfer** runs a chamfer or engraving cutter along picked edges (a closed chain all round,
an open one along its side) or round the contours of a face or sketch. Give the **Chamfer
width** on the face and how far the **Tip** runs below the chamfer (0.5 mm by default, so the
flat tip never rubs the wall); the depth follows from the cutter's tip angle, one pass at the
start depth, which is the edge's height. A pointed cutter needs a tip offset above 0, and a
chamfer wider than the cone is refused.

**Engraving** runs the cutter's tip on picked edges, sketch curves or sketch text, as deep as
the final depth, in depth passes of **Stepdown**.

## Holes milled round their axis

**Helical bore** mills a hole wider than the tool (and at most twice as wide, so no core is
left standing) by going down a helix, **Rise per turn** a turn (empty: the stepdown), then one
full turn round the bottom, and out through the middle. Untick **In a hole** to mill round a
round boss from outside instead: the tool's centre runs half a tool diameter outside the boss's
diameter, any diameter is possible, climb milling turns clockwise, and after the turn at the
bottom the tool leaves straight up its own groove (`internal: false` and `pitch` in
`cam.op.helix` and `cam.job.create`). Without a diameter, a helix at picked circles (a round edge, a face's
round holes, a sketch circle) takes their diameter, so picking a boss's rim mills round that
rim; picked circles of different sizes need a diameter given. **Thread** mills a thread with a thread mill: down the
middle of the drilled core hole (or beside a stud for an outside thread), out onto the
thread, and up one **pitch** a turn to above the top, counter-clockwise for a right-hand
thread. **Tapping** with a tap from the library feeds one pitch a turn and is always written
as the post's rigid tapping cycle (`G84`, Fanuc's `M29` and `G84`, LinuxCNC's `G33.1`,
Sinumerik's `G331`/`G332`); a post without one (GRBL, Heidenhain for now) refuses the program
rather than writing a tap as plain moves.

## Fixtures and part zero

A setup's **Fixtures** are the bodies that hold the part (vice jaws, clamps). They never count
as part of the model's box, so the stock and a box-point part zero ignore them, and posting
refuses a step whose tool, shank or holder comes within 0.5 mm of a fixture. **Part zero follows
later model changes** works a box-point part zero out again from the model as it is each time,
instead of keeping where it was when the setup was saved.

## Collisions

Posting refuses a step whose tool runs into the part or a fixture, and says which part of the
tool, which body and where (in model coordinates):

- the **shank** above the flutes (its **Shank diameter**, else the cutting diameter, up to the
  **Stick-out**) and the **holder** (its diameter and length above that), at every move;
- the **whole tool** at a rapid move across the tool axis (straight in and out along the axis
  it moves through what it cut itself);
- **every part** of the tool, flutes too, within 0.5 mm of a fixture, at every move.

The flutes cut the part on purpose, so they are not checked against it at feed. A shank or
holder may skim the part by 0.02 mm. The check sweeps each part of the tool along the path into
a solid and intersects it with the bodies on their exact faces; it never uses the display mesh.
The model must hold the finished part: holes, pockets and chamfers the program cuts but the
model lacks are material in the way. A tool without a stick-out is taken to stick out its flute
length, so give tools their stick-out and holder for the check to see them. A T-slot cutter's
undercut and the stock are not checked yet.

## Parts, setups and machines

The job list is grouped by **part**: each part, then the setups that make it, in order. One
part's setups can run on different machines, a mill for the drilling and a plasma table for the
outline, say. A setup's **Part** is chosen in the setup form; setups that name none belong to
the design's one part.

A setup says what it does (**Machining**: milling or plasma cutting) and runs on a machine of
that kind only; a machine says its **Kind of machine** (CNC mill, lathe, plasma table) in the
machine list. The rule holds for everything that follows from it:

- the setup form offers only machines of the setup's kind, and `cam.setup` refuses another;
- a post writes for one kind of machine: the setup form offers only those under **Post** (the
  machine's post first), the machine list only those for the machine's kind, and posting
  refuses a post for another kind;
- milling and drilling steps go in milling setups, plasma cuts in plasma setups; the step form
  offers only setups that take the step, and a copy goes only into such a setup.

Each setup has a **Setup sheet** button for its machine; the part's heading has one for all its
setups. A lathe is a kind already; its steps come later (see the roadmap), and further kinds
(laser, waterjet, punching) fit the same rule.

## Plasma cutting

A **plasma table** is a machine of the kind plasma: its bed, fastest feed and its torch's
cutting data (**Kerf width**, **Feed**, **Pierce height**, **Cut height**, **Pierce delay**),
which every new cut on it starts from. It has no spindle and no tool library: a **Plasma cut**
needs no tool. Make a setup with **Machining: Plasma cutting** on the table, then add plasma
cuts to it; each cut can change the torch's data for itself. A plasma cut cuts the sheet once,
from above it:

- at each cut the torch comes down at rapid to the pierce height, is lit, waits the pierce
  delay, feeds down to the cut height (at the plunge feed) and cuts the lead-in, the loop, the
  overlap and the lead-out there; it is put out before it rapids up to the retract height;
- holes are cut before the outline round them, so a part is never cut free before its holes;
- **Small holes up to** a diameter can be cut at **Their feed** (a share of the feed); leave it
  empty to cut them all at the feed;
- the step's top is the sheet's top and its bottom the sheet's underside; there are no depth
  passes, ramps, allowances or holding tabs.

The posts light the torch with their spindle-on block and put it out with their spindle-off
block, at every cut; the torch gets `S1`. The built-in **plasma** post writes no tool change and
no tool length offset (`M3 S1`, `M5`, pierce delays as `G4 P` in seconds); height control is
left to the table's controller; it is the only built-in post for plasma tables. The simulation
cuts the kerf through the whole stock. The setup sheet gives each plasma cut its torch data
instead of a tool.

## Tool list and setup sheet

**Tool list and setup sheet…** under the job list saves the manufacturing documents of every
setup: the tool list (each tool the steps use, with its kind, sizes, stick-out, holder and the
steps that cut with it) and a sheet per setup (work offset, part zero and how it is found, axes,
material, stock size and box in work coordinates, fixtures, model bodies, machine, post, the
steps in order with their tools and run times, and the setup's run time), then the total run
time. Save it as **HTML**, a printable page (print it to PDF from the browser; each setup starts
a new page), as a **CSV** tool list, or as **JSON**. A setup whose program cannot be posted has
no run time; the sheet says why.

## Drilling at picked bores

For **Drill**, pick round edges (each gives its centre), a flat face (every round hole in
it), a sketch circle (click inside it: the circle's centre; a profile with round holes gives
each of them) or a sketch point, instead of typing hole centres. The typed list is used only
while nothing is picked: one hole per line, `10 20`, `10.5; 20`, `10,5 20` (a decimal comma)
or `10,20`. The centres are taken in the setup's work XY when the step is saved, so moving the
sketch afterwards needs the step saved again.

## Job list, tool library and machines

On the **Machining** tab, the **Setup** panel opens **Job list**, **Tool library** and
**Machine list**, three tabs of one window.

The job list shows each setup with its **Material** and **Highest spindle speed
(rpm)**. Its steps run from top to bottom: **Up** and **Down** change the machining
order, and a step whose box is cleared is left out of the G-code. **Edit** opens a step in
the milling form with its values, tool and geometry (shown as kept until new contours are
picked); OK replaces it in place. **Remove** deletes a step; **Export G-code…** writes the whole setup.

The window can be sized by dragging its edges; its tabs scroll inside it.

**Show G-code** swaps the window for a small panel at the bottom right, so the model stays
free. It lists the setup's program as it would be exported, with the post its machine names
(LinuxCNC when none), one numbered row per line, and only this setup's toolpaths stay drawn.
Clicking a row draws that line's move on the model, thick for a cut and thinner for a rapid,
with a ring where it ends; **◀ Previous move** and **Next move ▶** step through the motion
lines. The program is posted again whenever the design changes, so it always shows what an
export would write. **Copy** puts the whole program on the clipboard. The panel can be sized
too; closing it brings the CAM window back. **Search** finds the next line holding a text.
**Edit** turns the list into the program as text; **Export edited program…** writes that text
as it is. Edited text is neither posted again nor read back and checked, which the panel says
in red: check such a program in the control's own simulation before running it.

**Play** runs the tool along the program as it is written: the way it has come is drawn over
the toolpaths, the tool is shown as a line up its axis with a ring at its tip, and the row it
is on is selected as it goes. **Pause** holds it, **To start** puts it back at the beginning,
**Speed** sets how fast it travels (mm of tool travel per second, rapids alike), and the slider
moves it to any point of the program. Clicking a row or stepping puts the tool at the end of
that line's move. It shows the moves; it does not remove material.

**Simulate material removal** cuts the setup's steps (not the skipped ones) from its stock and
draws what is left over the model, translucent. The panel says how much material came off and,
per step, how many rapid moves go through the stock (a crash on the machine) and how many cuts
have material above the tool's flute length (the shank or holder would rub). The stock is a
height field over the work plane, a grid of about a million cells: it sees floors, walls and
drill points, not undercuts, the tool's side, the holder or fixtures. While shown it is made
again whenever the design changes; **Hide simulation** takes it away. The stock is drawn in the
colour of a body still to be made, apart from the model. When it is no larger than the model
in X and Y (the default **Model box** without allowance) the panel says so, since cuts round the
part then remove nothing: give the setup's stock an allowance under **Stock › Sides (mm)**.

Under the summary the panel shows the estimated **run time**, split into cutting, rapids and tool
changes. Each move accelerates and brakes (500 mm/s² per axis); moves that join almost straight
(under 10°) at the same feed run on, while corners, feed changes and rapids stop the tool; tight
arcs are slowed so the bend stays within the acceleration. Rapids and every axis are held to the
machine's fastest feed (5000 mm/min without a machine), and a tool change counts 10 s. It is a
guide for planning, not the controller's own figure. `cam.post` returns the same estimate as
`time` (seconds: `cutting`, `rapid`, `dwell`, `tool_changes`, `total`, `per_operation` with the
step names in `operations`).

The tool library is kept in the preferences, for every design. **Export library…** writes it
to a file (desktop app; `{"format": "solidcraft-tools", "version": 1, "tools": [...]}`), for
another computer or a colleague. **Import library…** reads such a file and says how many of
its tools are new and which differ from the library's tools of the same number; **Take only
new ones** keeps the library's own, **Take all (replace)** takes the file's. A file that is
not a tool library, or holds a broken tool, is refused and changes nothing. **Use in design**
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

The **Load** line under the fields works out what the step as typed asks of the spindle:
the thickest chip (fz thinned by a stepover below half the diameter), the removal rate
Q = ap · ae · vf, the power Pc = Q · kc / (60 · 10⁶ · η) with kc = kc1.1 · hm^−mc after
Kienzle from the mean chip hm (rounded kc1.1 and mc per material group, η = 0.8) and the
torque. A slot and a helix take the whole diameter as ae, a drill its whole cross-section.
In orange it says what the setup's machine cannot give: a spindle speed outside its range,
a feed above its fastest, more power than its **Spindle power (kW)** (empty: not checked).
**Make up for thin chips** makes **Use cutting data** raise the feed until the thickest
chip is fz again (at most five times, never above the machine's fastest feed).

The machine list is kept in the preferences, like the tool library, so a machine is made
once and every design can use it; a name already on the list is refused for a new machine.
A machine has a name,
the travel of each axis as its lowest and highest position (X−, X+, Y−, Y+, Z−, Z+), an
optional A axis (limited to A− … A+ degrees, or endless), the slowest and fastest spindle
speed, the fastest feed (mm/min), the spindle power (kW, optional) and its **Post-processor**: the controller its programs
are written for. **Default (LinuxCNC)** keeps today's output; every built-in post is listed,
then your own posts (below). With a machine on the setup:

- **Use cutting data** and `cam.tool.speeds` hold the spindle speed to the machine's range
  (the feeds follow the speed actually reached) and every feed to the fastest;
- exporting refuses a program whose spindle speed or feeds leave the machine's range, or
  that reaches further along X, Y or Z than the machine travels. Where part zero sits on the
  table is set at the machine, so the check is by reach, not by position. The A axis is
  recorded for later 4-axis work; 2.5D programs do not use it;
- exporting and the preview write the program with the machine's post, unless the setup
  names its own (`post`) or `cam.post` is given one.

**Program folder** (desktop app only) is where the machine's programs go: a local folder
(`C:\CNC\Mill`) or the machine's network share (`\\linuxcnc\nc_files`, the `nc_files` folder
a LinuxCNC machine shares over Samba). With a folder set, **Export G-code…** writes the
program straight into it as `<setup>.<extension of the post>` and replaces a program of that
name; empty, the save dialog asks each time. When the folder cannot be written (the machine is
switched off, the share is gone) the job list says why and the save dialog opens, so the
program is not lost. The folder is kept with the personal machine list on this computer, never
in the design, since a path holds on one computer only. In the browser an export is always a
download.

### Your own posts

The **Posts** tab of the CAM window keeps your own posts. Each one starts from a built-in post
(**Start from**, then **New post**) and changes its settings: the file extension, millimetres or
inches, the decimals of coordinates and of feeds, line numbers (`N`, first number and step),
comments in parentheses, after a semicolon or none, arcs as `G2`/`G3` or as short lines (with
their chord error), arcs that also move in Z, a decimal point on whole numbers (`X10.`),
two-digit codes (`G01`), the tool change also for the first tool, the dwell, and the code
blocks: program start (the safe start), tool change, spindle on and off, coolant on and off,
and program end, one code line per row. Words in braces are filled in when the program is
written: `{program}`, `{units}`, `{work_offset}`, `{tool}`, `{diameter}`, `{rpm}`,
`{seconds}`, `{milliseconds}`; a post with an unknown word, or a built-in post's name, is
refused when you save it.

**Drilling as canned cycles** writes each drilled hole as one block instead of move by move:
`G81` for a hole drilled in one go, `G82` with a dwell (`P` in the unit of the post's dwell),
`G83` with pecks (`Q` is the step-down), each with `G98` so the tool comes back up to the
clearance height, and `G80` after it. The controller's `G83` measures its pecks from the
retract height `R`, so the first peck may end a little higher than in the preview; the hole's
depth is the same. A pecked hole with a dwell stays move by move, as `G83` has no dwell. The
built-in posts leave this off; the check that reads the program back understands the cycles.
Turn it on only for a controller that has these cycles (GRBL has none).

Like machines, your posts are kept in the preferences and shared by every design. A machine
can name one as its post-processor; a setup on that machine copies the post into the design
with the machine, so a design sent to someone else still writes the same program. A post a
listed machine names cannot be removed until that machine has another post.

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
`cam.machine.remove`, `cam.post.define`, `cam.post.remove`, `cam.list`, `cam.post`, `cam.simulate` and `cam.sheet` are also available through scripts, CLI and the control channel. Contour
and pocket commands additionally accept sketch profiles, planar faces and polygon
regions with islands, and expression-based depths and feeds. Use `cam.post` with
`post: "linuxcnc"` for LinuxCNC output; its existing default remains `rs274`.

Picked geometry: `faces` entries may name one contour with `contour_at` (a model point on
it), `edges` (`[{body, at}]`) add the contours through those edges (with `chain: true`, or
when an edge bounds no flat face square to the tool axis, the edges themselves joined into
closed contours, stored as `{"type": "edges"}`; a chain that does not close is an open contour
with `open_side` `left`/`right`/`on`, `reverse` and `extend_start`/`extend_end` in mm), a
contour's `lead_in`/`lead_out` (`{"kind": "direct"}`, `{"kind": "arc", "radius", "sweep"?}`,
`{"kind": "tangent"|"perpendicular", "length"}`) and `overlap`, holding `tabs`
(`{"count": 4, "width": 5, "height": 1}`, or `spacing` instead of `count`; null removes them), and a contour's
`start` is a model point. `cam.tool.define` takes `cutting`
(`{"aluminium": {"vc": 300, "fz": 0.05, "ap": 0.5, "ae": 0.4}}`); `cam.setup` takes
`material` and `max_rpm` (null clears either), and `rename` (a new name for the setup `name`;
a name another setup has is refused). `cam.setup` also takes picks on the model:
`z_face` and `x_edge` (`{body, at}`), `zero_edge` (a round edge's centre, else its middle),
`flip_z` and `flip_x`, `zero_axes` (`{x?, y?, z?: {body, at, edge?}}`), `wcs_origin_mode` (`selected_point`, `model_origin`,
`model_box_point`, `stock_box_point`) with `box_point` (`{x, y, z}`, each `negative`,
`center` or `positive` in the work frame) and `stock_spec`
(`{"kind": "relative_box", "mode": "sides_and_top_bottom", "side_offset": 2, …}`,
`{"kind": "box", "width", "depth", "height"}` or `{"kind": "cylinder", "diameter", "height"}`).
`cam.edges.chain` (`setup`, `edges: [{body, at}]`) suggests the chain through picked edges:
`edges: [{body, at, index}]` in order and `closed`; it changes nothing.
`cam.faces.level` (`setup`, `faces: [{body, at}]`) finds every flat face of the picked faces'
bodies in the same plane and facing the same way, such as all letters of embossed or engraved
text from one of them: `faces: [{body, name, at, index}]`, the picked ones first, each with a
point inside it, ready to pass as `faces` to a step; it changes nothing. In the milling form it
is **All faces in this plane**.
`cam.op.face` faces a picked face's outline (`faces` or `edges`), an `outer` rectangle or the
stock with `stepover` (default 70 % of the diameter), `overrun` (default 2 mm), `bottom` (the
finished height, default part zero) and `allowance` (the material above it; without it, `top`
or the stock's top); retract and clearance heights below the start are raised above it. The
guided job takes `kind: "face"` with `overrun` and the picks; its `top - bottom` is the
allowance. `cam.op.slot` runs the tool centre along picked `edges` (always the edges
themselves, open or closed chains) or around the contours of `faces`, a `sketch` or an
`outer` polygon, with `width` (default the tool's diameter), `stepover` (side passes,
default half the diameter), `stock_to_leave`, `tolerance` and `ramp_angle`; the guided job
takes `kind: "slot"` with `slot_width`. Without `stepdown`, milling steps down one tool
diameter. Contours take `side` (`auto`, `outside`, `inside`, `on`); contours and pockets take `climb`
(false: conventional), `stock_to_leave` (walls) and `floor_stock` (floor), in the guided job
too. Machines:

```json
{"name": "Portal", "x": [0, 600], "y": [0, 400], "z": [-120, 0], "a": {"kind": "endless"},
 "spindle_min": 3000, "spindle_max": 12000, "max_feed": 2000, "post": "linuxcnc"}
```

`post` is optional: a built-in post (`cam.list` returns them as `builtin_posts`) or one saved on
the design. `cam.post` without `post` uses the setup's post, else its machine's, else `rs274`;
the guided job and export fall back to `linuxcnc`.
`cam.simulate {"setup": "Top", "cell": 0.2}` cuts the setup's steps from its stock (`cell`, the
grid size in mm, is optional) and returns `removed` (mm³), the `cell` used, per step `removed`,
`rapid_hits` and `too_deep` (move indices, the first 100) and `warnings` as text. A stock box
given to `cam.setup` as `stock` is used as it is; otherwise the setup's stock around the model.
`cam.sheet {"setup": "Top", "path": "sheet.html"}` returns the tool list and setup sheets as
data (`tools`, `setups`, `seconds`) and, given a `path`, writes them as `html`, `csv` (the tool
list) or `json` (`format`, else from the extension; `overwrite`, `title` and `labels` for the
page's headings are optional).
`cam.machine.define` adds or replaces the design's machine of that name, `cam.machine.remove`
removes one no setup names, and `cam.setup` takes `machine` as a name, a whole machine
(copied into the design) or null; with a whole machine, `machine_post` copies the post it
names in full as well. A post saved on the design:

```json
{"name": "Werkstatt", "base": "grbl", "line_numbers": true, "line_start": 100, "line_step": 5,
 "comments": "semicolon", "decimals": 2, "header": ["G17 G21 G90 G94 {work_offset}"]}
```

`cam.post.define` starts from `base` (a built-in or saved post; default the saved post of that
name, else `linuxcnc`), changes only the settings given and adds or replaces the design's post
of that name; a built-in post's name is refused. `cam.post.remove` removes a saved post no
machine or setup names. Machining order and skipped steps:

```json
{"setup": "Top", "name": "Holes", "to": 0}
{"setup": "Top", "name": "Pocket", "skip": true}
```

`cam.op.move` gives an operation a new place in the order (from 0) and `cam.op.skip`
leaves it out of posting or puts it back; both return the order and the skipped steps.
`cam.op.duplicate` (`{"setup", "name", "new_name"?, "to_setup"?}`) copies a step right after
itself, or to the end of another setup, the name numbered on when taken; `cam.op.rename` gives
a step a new name, also in the skipped list. `cam.op.chamfer`, `cam.op.engrave`,
`cam.op.helix`, `cam.op.thread` and `cam.op.tap` make the steps above; `cam.export` takes
`text` to write a hand-edited program as it is (`checked: false` in its answer).
`cam.tool.speeds` (`{"tool": 1, "material": "aluminium"}`, or `setup` for that setup's
material and highest spindle speed) returns the spindle speed, feeds, stepdown and
stepover the cutting data give, without changing the design, with the cut's `load`
(`chip`, `thinned_feed`, `removal`, `power`, `torque`) and what the setup's machine cannot
give (`overload`: `spindle_fast`, `spindle_slow`, `feed`, `power`).

The local integration builds on the SolidCraft CAM core and document command work
from [#5751](https://github.com/tangency-labs/solidcraft/pull/5751) and
[#2881](https://github.com/tangency-labs/solidcraft/pull/2881). These are review-stage
project changes; this local branch does not imply their upstream acceptance.

LinuxCNC syntax reference: [official G-code reference](https://linuxcnc.org/docs/html/gcode.html).
