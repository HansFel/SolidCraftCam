# CAM: making parts on machines

CAM turns a design into programs for your machines: milling programs in G-code for a CNC
mill, and cutting programs for a plasma table. This chapter explains the CAM area as a user
sees it. The full reference, with every command and parameter, is
[2.5D CAM for LinuxCNC](https://github.com/HansFel/SolidCraftCam/blob/cam-linuxcnc/docs/cam-linuxcnc.md).

Inside SolidCraft the same explanations are at hand: the CAM window has a **Manual** tab, and
every field in the CAM forms explains itself when you hold the pointer over it (over the label
or over the field).

## How CAM is organised

The order is **part → setups → steps**.

- A **part** is what you make.
- A **setup** is one clamping of the part on one machine. It has its own part zero, its own
  machine and post, and its own program.
- A **step** is one operation in a setup: a contour, a pocket, a drilled hole, a plasma cut.

The **Machining** tab of the ribbon starts every setup and step. The **CAM window** has five
tabs: the **job list**, the **tool library**, your **machines**, the **posts** and the
**manual**.

## Parts

The job list groups the setups under their part ("Part: Bracket"). One part's setups may run
on different machines, for example cut on the plasma table first and then drilled on the mill.
Name the part in the setup form; setups that name none belong to "This design's part".
**Sheets for the part** prints the setup sheets of all its setups at once.

## Machines and the kind rule

Every machine has a **kind**: CNC mill, lathe or plasma table. Every post writes for one kind.
The setup's **type** (milling or plasma cutting) decides the rest:

| Setup type | Machines offered | Posts offered | Steps it takes |
|---|---|---|---|
| Milling | CNC mills | milling posts | contour, pocket, facing, slot, chamfer, engraving, drilling, helix, thread milling, tapping |
| Plasma cutting | plasma tables | the plasma post | plasma cuts |

Wrong combinations are refused, in the forms and through commands alike. Further kinds of
machine (laser, waterjet, punching) will follow the same rule.

The **Machines** tab holds your machines, shared by every design: travels of X, Y and Z (each
program is checked against them), spindle range, fastest feed, an A axis, the post and the
folder programs are written to. **New plasma table** adds a table without a spindle; it has
torch data instead (kerf, feed, pierce and cut height, pierce delay).

## Setups

**New setup** (ribbon or job list) and **Edit** in the job list open the setup form:

- **Setup** name and **Part**.
- **Type**: milling or plasma cutting.
- **Machine** and **Post**: only those of the kind the type needs; the post list starts with
  the machine's own post.
- **Work offset** G54–G59.
- **Part zero**: picked on the model (a corner; a round edge gives its centre; or one pick per
  axis), the model origin, or a point on the model's or the stock's box. Pick a flat face to
  point +Z out of it and a straight edge to run +X along it; **Turn Z round** and **Turn X
  round** reverse an axis.
- **Stock**: a box round the model with allowances, a box of fixed size, or a round bar.
- **Material** and highest spindle speed (milling only).

In the job list the **eye** in front of a setup hides or shows its toolpaths and its part zero
in the view, so you can see which belong together. **Fold** makes the window small; the eyes
stay.

## Tools

The **Tools** tab is your tool library, shared by every design; each design keeps a copy of the
tools it uses. A tool has a number (T), a kind (flat, ball or corner-radius end mill, drill,
centre drill, reamer, chamfer cutter, engraving cutter, T-slot cutter, thread mill, tap), a
diameter, a flute length, flutes, an optional holder (for the collision check) and cutting data
per material (vc, fz, ap, ae), from which feed and spindle speed are worked out.

The milling form also works out what a step asks of the spindle (**Load**): the thickest
chip, the material removed per minute, the power and the torque. Lines in orange say what
the setup's machine cannot give: a spindle speed outside its range, a feed above its
fastest, or more power than its **Spindle power (kW)**. A stepover below half the diameter
cuts chips thinner than fz; with **Make up for thin chips** ticked, **Use cutting data**
raises the feed until they are fz thick again. The power uses rounded values per material
group (Kienzle): a guide, not the tool maker's data.

A plasma table needs no tool: its torch data belong to the machine and to each cut.

## Making a step

Choose the step in the ribbon. The form shows only what that step needs:

1. Choose its **setup** (only setups whose type takes the step are offered), or **New setup…**.
2. Choose the **tool** (milling) or check the torch data (plasma).
3. Pick the geometry on the model: edges, faces, holes; or type a rectangle or hole centres.
   **Start depth** and **Final depth** can be picked on the model too.
4. **Preview toolpaths** draws the path. **OK** stores the step in the design (one undo step)
   and opens the job list on it.

In the job list every step can be edited, renamed, duplicated, moved up or down, copied to
another setup that takes it, skipped or removed.

## Milling steps

- **Contour**: along a picked outline, inside, outside or on it; with lead-in and lead-out
  (direct, tangent arc, tangent line, square line), overlap and holding tabs. An open edge
  chain runs from the first picked edge towards the second.
- **Pocket**: clears the inside of an outline and leaves its islands standing, with a ramp.
- **Facing**: takes the allowance off the top down to the finished height.
- **Slot**: a groove along edges or curves.
- **Chamfer**: a chamfer cutter along edges.
- **Engraving**: the tip of a V-bit follows edges, sketch curves or text.

Wall and floor stock leave material for a finishing step: rough with stock left, then finish
with a second step at 0.

## Holes

- **Drill**: peck drilling at picked bores or typed centres.
- **Tap**: rigid tapping, written as the post's tapping cycle.
- **Helical bore**: the tool goes down a helix and makes one full turn at the bottom. In a hole
  it mills a hole wider than the tool (at most twice its diameter). Untick **In a hole** to mill
  round a boss from outside; then any diameter is possible. **Rise per turn** sets how steeply
  the helix goes down; empty uses the stepdown.
- **Thread mill**: a thread in a hole or on a stud, right or left hand, one pitch a turn.

## Plasma cutting

Plasma cuts go into a plasma setup on a plasma table and need no tool. The torch data (kerf,
pierce height, cut height, pierce delay) come from the table and can be changed per cut. Each
contour is pierced, cut, and the torch switched off. Small holes can be cut slower; they are cut
before the outline, so the part stays where it is.

## Checking

Before a program can be exported it is read back and compared with the planned toolpaths.
Posting refuses a step whose shank or holder meets the part, or which comes too close to a
fixture (bodies ticked as fixtures in the setup), and checks the travels against the machine.

**Simulate material removal** in the G-code view cuts the stock and reports rapid moves through
material and cuts above the flutes. The cutting time counts accelerations and tool changes.

## G-code

**Show G-code** in the job list shows a setup's program: click a line to see its move on the
model; search, copy, export. **Edit** allows changes by hand; they are written as they are and
not checked again. **Export G-code** writes the program into the machine's program folder.

Built-in posts: LinuxCNC, RS-274, Fanuc, Sinumerik, Heidenhain (conversational), Mach3,
Centroid, Grbl and Plasma. The **Posts** tab makes your own from one of them.

## Setup sheets and tool list

**Setup sheet** (one per setup) and **Sheets for the part** make a printable page for the
machine: part, type, machine and its kind, post, work offset, part zero and axes, stock,
fixtures, the steps with their tools or torch data, and the tool list.
