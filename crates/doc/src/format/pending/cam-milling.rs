//! A design can hold CAM milling data (`cam`: tools, setups, 2D contour/pocket operations,
//! and drill operations with work-XY hole centres, peck depths and bottom dwell).
//! A setup is an ISO 841 work frame in the model (`work_origin`, `tool_axis`, optional `x_axis`)
//! plus an RS-274 work offset (`G54`…`G59`) and an optional stock box. Sketch and face geometry
//! is mapped into that frame when posting; a face can name one of its contours (`contour_at`, a
//! point on it) instead of its whole outline; a face the model gives no name has an empty `name`
//! and is found by its points. A contour operation can name a `start` point (model
//! coordinates); explicit polylines are already work XY. Depths, feeds and stepdowns are
//! expressions evaluated when posting. A tool can hold cutting data by material group
//! (`cutting`); a setup can name its material, the machine's highest spindle speed (`max_rpm`),
//! its machine (`machine`, one of the design's `machines`: travel per axis, optional A axis,
//! spindle speed range and fastest feed) and the operations left out when posting (`skipped`);
//! the operations' order is the machining order. A contour's `side` is `auto` (around an outline,
//! inside a hole picked as its own contour), `outside`, `inside` or `on`; contours and pockets
//! can leave stock on the walls (`stock_to_leave`) and on the floor (`floor_stock`). A facing
//! operation (`face`) faces the setup's stock (`geometry: {"type": "stock"}`) or a rectangle
//! with a `stepover` and an `overrun` past the stock's edges. Toolpaths are never saved. Older designs need no transformation: an absent `cam` field
//! means no setups. The last release refuses a design saved with this (the newer format number).
