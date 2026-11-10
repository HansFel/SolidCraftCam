//! A design can hold CAM milling data (`cam`: tools, setups, 2D contour/pocket operations,
//! and drill operations with work-XY hole centres, peck depths and bottom dwell).
//! A setup is an ISO 841 work frame in the model (`work_origin`, `tool_axis`, optional `x_axis`)
//! plus an RS-274 work offset (`G54`…`G59`) and an optional stock box. Sketch and face geometry
//! is mapped into that frame when posting; a face can name one of its contours (`contour_at`, a
//! point on it) instead of its whole outline; a face the model gives no name has an empty `name`
//! and is found by its points. Picked edges can be one chain instead (`geometry: {"type": "edges",
//! "edges": [{body, at}]}`): the edges nearest the points, joined end to end into closed contours
//! seen along the tool axis; a chain that does not close is an open contour, cut on its
//! `open_side` (`left`, `right`, `on`, along the cutting direction), from its end when `reverse`,
//! run on by `extend_start` and `extend_end`. A contour's `lead_in` and `lead_out` are
//! `{"kind": "direct"}` (the default), `arc` (`radius`, `sweep`), `tangent` or `perpendicular`
//! (`length`); a closed contour's cut runs `overlap` past its start. Closed contours can keep
//! holding `tabs` (`{count | spacing, width, height}`, mm; absent: none). A contour operation can name a `start` point (model
//! coordinates); explicit polylines are already work XY. Depths, feeds and stepdowns are
//! expressions evaluated when posting. A tool can hold cutting data by material group
//! (`cutting`); a setup can name its material, the machine's highest spindle speed (`max_rpm`),
//! its machine (`machine`, one of the design's `machines`: travel per axis, optional A axis,
//! spindle speed range, fastest feed, an optional `spindle_power` (kW) cuts are checked against and an optional `post`, the post-processor its programs are
//! written with) and the operations left out when posting (`skipped`);
//! the operations' order is the machining order. A contour's `side` is `auto` (around an outline,
//! inside a hole picked as its own contour), `outside`, `inside` or `on`; contours and pockets
//! can leave stock on the walls (`stock_to_leave`) and on the floor (`floor_stock`). A facing
//! operation (`face`) faces the setup's stock (`geometry: {"type": "stock"}`) or a rectangle
//! with a `stepover` and an `overrun` past the stock's edges. A slot operation (`slot`) runs the tool centre along each picked edge chain
//! (open or closed) or contour, `width` wide (absent: the tool's diameter) with side passes `stepover`
//! apart, `stock_to_leave` on both walls, `tolerance` and `ramp_angle` as for pockets. A tool's `kind` can also be
//! `bull_nose` (with `corner_radius`), `chamfer` and `v_bit` (`tip_angle`, `tip_diameter`), `t_slot`,
//! `centre_drill`, `reamer`, `tap` and `thread_mill` (both with `pitch`); any tool can give `shank_diameter`,
//! `overall_length`, `stick_out` and a `holder` (`{description, diameter, length}`). Further operations:
//! `chamfer` (edges or contours, `params: {width, tip_offset, side, tolerance}`), `engrave` (`params:
//! {tolerance}`), `helical_bore` (hole centres, `params: {diameter, climb, pitch, outside}`: `pitch` the rise per turn, none for the
//! cut's stepdown; `outside` mills round a boss) and `thread_mill` (hole
//! centres, `params: {diameter, pitch, internal, left_hand}`); a drill operation whose tool is a tap taps.
//! A setup can name `fixtures` (bodies the tools keep clear of, never part of the model's box) and
//! `zero_follows` (a box-point part zero worked out again from the model as it is). A post can say how it
//! writes a tapped hole (`tapping`: `none`, `g84`, `g84_rigid`, `g33_1`, `g331`). A setup can name the
//! `part` it makes (its setups are that part's steps, on any machines) and its `operation_type`
//! (`milling`, `plasma`, …); it runs on a machine of that kind only (a machine's `kind`: `mill`,
//! `lathe`, `plasma`; a plasma table can hold `torch` cutting data `{kerf, feed, pierce_height,
//! cut_height, pierce_delay}`), and a post writes for one kind of machine (`machine`). A plasma
//! setup holds `plasma` operations: no tool, `params` as a contour's, `torch: {kerf, pierce_height,
//! cut_height, pierce_delay, slow_holes?: {diameter, share}}`, `cut` with the sheet from `top` to
//! `bottom`. A contour's `params` can say who offsets it by the tool's radius (`compensation`: `cam`, the default and
//! left out, `control` or `wear`), and a post the words it writes for that (`comp_left`, `comp_right`, `comp_off`).
//! Toolpaths are never saved. Older designs need no transformation: an absent `cam` field
//! means no setups. The last release refuses a design saved with this (the newer format number).
