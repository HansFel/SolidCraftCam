//! CAM commands: tools, setups, 2D contour/pocket operations, and G-code posting through
//! `solidcraft-cam`. Toolpaths are computed at post time and never stored on the design.
//! Geometry is a sketch profile or planar face (or explicit polylines); depths and feeds are
//! expressions evaluated when posting.

use serde_json::{Value, json};
use solidcraft_cam::{
    ContourParams, Cut, FaceParams, Feeds, Heights, Lead, Leads, Machine, Move, OpenContourParams, OpenPath, OpenSide, Operation, PocketParams, Post,
    Program, Side, SlotParams, Tabs, Tool, ToolKind, contour_open, contour_with_tabs, drill, emit, face, pocket, readback, slot,
};
use solidcraft_doc::cam::{
    BoxAxisRef, CamContourParams, CamCut, CamEdgeRef, CamFaceRef, CamGeometry, CamLead, CamOpenSide, CamOperation, CamPocketParams, CamSettings,
    CamSetup, CamSide, CamStock, CamTabs, MAX_MACHINES, MAX_OPS, MAX_SETUPS, MAX_TOOLS, PolygonRegion, StockBox, StockOffsetMode, WcsOriginMode,
    WorkCoordinateSystem, default_contour_params, default_cut, default_face_params, default_pocket_params, default_slot_params,
};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::project::resolve_face_indices;
use solidcraft_doc::{Expr, FeatureKind, select_profiles};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_sketch::{LinkGeom, LinkKind};

use super::CommandSpec;
use crate::params::{bad, bool_, expr, num, req_expr, str_, string_list, vec2, vec2_list, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.tool.define", "Define Tool", crate::checked!("cam.tool.define", tool_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::ToolParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::ToolResult>).icon("hole").params(
        "number (1…9999); kind?: flat_endmill|ball_endmill|drill (default flat_endmill); diameter (mm); flute_length (mm); \
         description?; flutes?; spindle_rpm?; feed?; plunge_feed?; stepdown?; stepover?; \
         cutting?: {material: {vc m/min, fz mm/tooth (drill: per rev), ap × D, ae × D}}",
    ),
    CommandSpec::new("cam.tool.remove", "Remove Tool", crate::checked!("cam.tool.remove", tool_remove)).contract(&crate::schema::TypedContract::<crate::schema::cam::ToolRemoveParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::DeleteResult>).icon("delete").params("number"),
    CommandSpec::new("cam.machine.define", "Define Machine", crate::checked!("cam.machine.define", machine_cmd))
        .contract(&crate::schema::TypedContract::<crate::schema::cam::MachineParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::cam::MachineResult>)
        .icon("settings")
        .params(
            "name; x, y, z: [lowest, highest] travel (mm, X− X+ …); a?: {kind: limited, min, max} (degrees) | {kind: endless}; \
             spindle_min, spindle_max (rev/min); max_feed (mm/min); post?: the post its programs are written with              (built-in or saved on the design). Adds or replaces the design's machine of that name.",
        ),
    CommandSpec::new("cam.machine.remove", "Remove Machine", crate::checked!("cam.machine.remove", machine_remove))
        .contract(&crate::schema::TypedContract::<crate::schema::cam::MachineRemoveParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::cam::DeleteResult>)
        .icon("delete")
        .params("name (a machine no setup names)"),
    CommandSpec::new("cam.setup", "CAM Setup", crate::checked!("cam.setup", setup_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::SetupParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::SetupResult>).icon("settings").params(
        "name?: Setup 1, Setup 2, … when omitted; rename?: a new name for the existing setup `name` (steps and settings stay); work_offset?: 1…6 (G54…G59, default 1); \
         tool_axis?: [x,y,z] (programmed +Z, default [0,0,1]); \
         x_axis?: [x,y,z] (programmed +X, square to the tool axis; omitted, derived); \
         work_origin?: [x,y,z] mm part zero in the model (default [0,0,0]); \
         z_face?: {body, at} (+Z out of that flat face); x_edge?: {body, at} (+X along that straight edge); \
         flip_z?, flip_x?: bool (turn that axis round once); zero_edge?: {body, at} (part zero at a round edge's centre, else its middle); \
         zero_axes?: {x?, y?, z?: {body, at, edge?}} (each moves zero along that work axis only,          after `wcs_origin_mode` has placed it, so unpicked axes take the box point); \
         wcs_origin_mode?: model_origin|selected_point|model_box_point|stock_box_point, with box_point?: \
         {x,y,z: negative|center|positive} in the work frame; stock_spec?: {kind: relative_box|box|cylinder|…}; \
         model_bodies?: [names]; stock?: {min:[x,y,z], max:[x,y,z]}; \
         machine?: a machine's name, a machine as cam.machine.define takes it (copied into the design), or null; delete?: bool",
    ),
    CommandSpec::new("cam.op.contour", "2D Contour", crate::checked!("cam.op.contour", contour_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::MillingParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::OperationResult>).icon("cut").params(
        "setup; name; tool (number); sketch?: id or name, with profiles?: as solid.extrude; faces?: [{body, name, at?, contour_at?}]; \
         edges?: [{body, at}] (each the contour of a face square to the tool axis through that edge point); \
         chain?: bool (the picked edges themselves, joined end to end into closed contours seen along the tool axis; \
         also taken when an edge bounds no such face); \
         geometry?: a step's stored geometry as cam.list shows it (an edit hands it back); \
         start?: [x,y,z] model point each contour starts nearest; or outer: [[x,y],…] (or regions: [{outer, holes?}]); holes?: [[[x,y],…]]; \
         side?: auto|outside|inside|on (default auto: around an outline, inside a picked hole; outside and inside are of each \
         picked contour); climb?: bool (default true, false: conventional); stock_to_leave? (walls); floor_stock? (floor); tolerance?; \
         ramp_angle?: deg (default 0); lead_in?, lead_out?: {kind: direct|arc|tangent|perpendicular, radius?, sweep?, length?}; \
         overlap?; open contours: open_side?: left|right|on, reverse?, extend_start?, extend_end?; \
         tabs?: {count? | spacing?, width, height} holding tabs on closed contours, or null; \
         clearance?; retract?; top?; bottom (required unless cut given); stepdown?; \
         feed?; plunge_feed?; spindle_rpm?; coolant?",
    ),
    CommandSpec::new("cam.op.pocket", "2D Pocket", crate::checked!("cam.op.pocket", pocket_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::MillingParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::OperationResult>).icon("hole").params(
        "setup; name; tool (number); sketch?: id or name, with profiles?; faces?; edges?; or outer/regions; holes?; \
         stepover? (default half the tool diameter); climb?; stock_to_leave? (walls); floor_stock? (floor); tolerance?; ramp_angle?: deg; \
         clearance?; retract?; top?; bottom; stepdown?; feed?; plunge_feed?; spindle_rpm?; coolant?",
    ),
    CommandSpec::new("cam.op.slot", "Slot", crate::checked!("cam.op.slot", slot_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::MillingParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::OperationResult>).icon("cut").params(
        "setup; name; tool (number); edges?: [{body, at}] the picked edges, joined end to end into chains seen along the tool          axis (open or closed); or sketch?/profiles?, faces? or outer/regions: their contours, each a closed path;          the tool centre runs on each path. width? (default the tool diameter: one pass; wider adds side passes);          stepover? (side passes, default half the tool diameter); stock_to_leave? (both walls); tolerance?; ramp_angle?: deg          (default 0: plunge; an open path ramps back and forth); clearance?; retract?; top?; bottom; stepdown?; feed?;          plunge_feed?; spindle_rpm?; coolant?",
    ),
    CommandSpec::new("cam.op.face", "Facing", crate::checked!("cam.op.face", face_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::MillingParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::OperationResult>).icon("cut").params(
        "setup; name; tool (number); faces?/edges?: a picked face, whose outline's box is faced (the base faces the \
         whole footprint); outer?: [[x,y],…] a rectangle in work XY (default: the setup's stock outline); \
         stepover? (default 70 % of the tool diameter); overrun?: how far the tool's edge runs past the stock (default 2 mm); \
         bottom? (the finished height; default part zero); allowance?: the material \
         above it, where facing starts (default: up from the stock's top, or top?); stepdown? (default the tool \
         diameter); clearance?/retract? (raised above the start when lower); feed?; plunge_feed?; spindle_rpm?; coolant?",
    ),
    CommandSpec::new("cam.op.remove", "Remove CAM Operation", crate::checked!("cam.op.remove", delete_op)).contract(&crate::schema::TypedContract::<crate::schema::cam::RemoveParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::DeleteResult>).icon("delete").params("setup; name"),
    CommandSpec::new("cam.op.drill", "Drill", crate::checked!("cam.op.drill", drill_cmd)).icon("hole")
        .contract(&DRILL_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::cam::OperationResult>)
        .params("setup; name; tool; points?: [[x,y], ...] in work XY; holes?: [{body, at, edge?}] (a round edge's centre, else every round hole of the flat face at that point); bottom; stepdown?: maximum peck depth; dwell?: seconds; clearance?; retract?; top?; feed?; plunge_feed?; spindle_rpm?; coolant?"),
    CommandSpec::new("cam.post", "Post Process", crate::checked!("cam.post", post_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::PostParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::PostResult>).icon("export").noundo().params(
        "setup; post?: a built-in post or the name of a saved post (default: the setup's post, else its machine's, else rs274); program?: name (default the setup's) → \
         {gcode, extension, post, operations, readback}. G-code is always read back and checked against the toolpaths, \
         and against the setup's machine (spindle range, fastest feed, travel) when it names one.",
    ),
    CommandSpec::new("cam.op.move", "Move CAM Operation", crate::checked!("cam.op.move", move_op))
        .contract(&crate::schema::TypedContract::<crate::schema::cam::MoveParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::cam::OrderResult>)
        .icon("list")
        .params("setup; name; to: new place in the machining order, from 0 → {setup, order, skipped}"),
    CommandSpec::new("cam.op.skip", "Skip CAM Operation", crate::checked!("cam.op.skip", skip_op))
        .contract(&crate::schema::TypedContract::<crate::schema::cam::SkipParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::cam::OrderResult>)
        .icon("list")
        .params("setup; name; skip: true leaves it out when posting, false puts it back → {setup, order, skipped}"),
    CommandSpec::new("cam.tool.speeds", "Tool Cutting Speeds", crate::checked!("cam.tool.speeds", speeds_cmd))
        .contract(&crate::schema::TypedContract::<crate::schema::cam::SpeedsParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::cam::SpeedsResult>)
        .icon("hole")
        .noundo()
        .params(
            "tool; material?: group (aluminium, steel, stainless, brass, plastic, wood or the tool's own); setup?: takes its material and \
             highest spindle speed (held to its machine's range too) → spindle_rpm, feed, plunge_feed, stepdown, stepover from n = vc·1000/(π·D), vf = n·z·fz",
        ),
    CommandSpec::new("cam.edges.chain", "Find Edge Chain", crate::checked!("cam.edges.chain", chain_cmd))
        .contract(&crate::schema::TypedContract::<crate::schema::cam::ChainParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::cam::ChainResult>)
        .icon("cut")
        .noundo()
        .params(
            "setup; edges: [{body, at}] (picked edges of one body) → edges: [{body, at, index}], the edges joined to them end to end \
             in their plane (square to the tool axis for one straight edge square to it), in order; closed: whether the chain comes round",
        ),
    CommandSpec::new("cam.list", "List CAM", crate::checked!("cam.list", list_cmd)).contract(&crate::schema::TypedContract::<crate::schema::cam::ListParams>::new()).output(crate::schema::output_schema_for::<crate::schema::cam::ListResult>).icon("list").noundo().params("→ tools, machines, setups (with operations) and saved posts"),
];

const DRILL_CONTRACT: crate::schema::TypedContract<crate::schema::cam::DrillParams> = crate::schema::TypedContract::new();

pub(super) fn drill_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.op.drill";
    let args: crate::schema::cam::DrillParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let tool = s.doc.cam.tool(args.tool).cloned().ok_or_else(|| bad(cmd, "define the drill tool first"))?;
    let mut points = if p.get("points").is_some() { vec2_list(cmd, p, "points", solidcraft_doc::cam::MAX_LOOP_POINTS)? } else { Vec::new() };
    // Picked bores: a round edge's centre, or every round hole of a picked flat face, in work XY.
    if let Some(holes) = p.get("holes") {
        let holes = holes
            .as_array()
            .filter(|h| h.len() <= solidcraft_doc::cam::MAX_LOOP_POINTS)
            .ok_or_else(|| bad(cmd, "`holes` must be a list of {body, at}"))?;
        let setup = s.doc.cam.setup(&args.setup).ok_or_else(|| bad(cmd, "create the setup first"))?;
        let frame = work_frame(setup).map_err(|e| bad(cmd, e))?;
        for pick in holes {
            let (body, at) = pick_param(cmd, pick, "holes")?;
            let centres = if bool_(pick, "edge") == Some(true) {
                vec![edge_centre(s, &body, at).map_err(|e| bad(cmd, e))?]
            } else {
                face_hole_centres(s, &body, at).map_err(|e| bad(cmd, e))?
            };
            for c in centres {
                let w = frame.local(c);
                let w = Vec2::new(w.x, w.y);
                if !points.iter().any(|q| q.dist(w) < 1e-6) {
                    points.push(w);
                }
            }
        }
    }
    let op = CamOperation::Drill {
        name: args.name.clone(),
        tool: args.tool,
        geometry: CamGeometry::Points { points: points.iter().map(|p| [p.x, p.y]).collect() },
        cut: cut_param(cmd, p, &tool)?,
        dwell: args.dwell,
        spindle_rpm: args.spindle_rpm.or(tool.spindle_rpm),
        coolant: args.coolant,
    };
    op.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let setup = s.doc.cam.setup(&args.setup).cloned().ok_or_else(|| bad(cmd, "create the setup first"))?;
    build_path(s, &setup, &op).map_err(|e| bad(cmd, e))?;
    let setup = s.doc_mut().cam.setup_mut(&args.setup).ok_or_else(|| bad(cmd, "setup disappeared"))?;
    if let Some(slot) = setup.operations.iter_mut().find(|o| o.name() == args.name) {
        *slot = op;
    } else {
        if setup.operations.len() >= MAX_OPS {
            return Err(bad(cmd, "too many CAM operations"));
        }
        setup.operations.push(op);
    }
    Ok(json!({"setup": args.setup, "operation": args.name}))
}

pub(super) fn tool_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool.define";
    let number = num(p, "number")
        .filter(|n| *n >= 1.0 && *n <= 9999.0 && n.fract() == 0.0)
        .map(|n| n as u32)
        .ok_or_else(|| bad(cmd, "`number` must be an integer 1…9999"))?;
    let kind = match str_(p, "kind").unwrap_or("flat_endmill") {
        "flat_endmill" => ToolKind::FlatEndmill,
        "ball_endmill" => ToolKind::BallEndmill,
        "drill" => ToolKind::Drill,
        other => return Err(bad(cmd, format!("unknown tool kind `{other}` (flat_endmill, ball_endmill, drill)"))),
    };
    let diameter = num(p, "diameter").ok_or_else(|| bad(cmd, "`diameter` is required (mm)"))?;
    let flute_length = num(p, "flute_length").ok_or_else(|| bad(cmd, "`flute_length` is required (mm)"))?;
    let mut tool = Tool::new(number, kind, diameter, flute_length);
    // Omitted, a redefined tool keeps its description and flute count.
    if let Some(old) = s.doc.cam.tool(number) {
        (tool.description, tool.flutes) = (old.description.clone(), old.flutes);
    }
    if let Some(d) = str_(p, "description") {
        tool.description = d.chars().take(256).collect();
    }
    if let Some(f) = num(p, "flutes").filter(|n| *n >= 1.0 && *n <= 100.0 && n.fract() == 0.0) {
        tool.flutes = f as u32;
    }
    tool.spindle_rpm = num(p, "spindle_rpm").or(tool.spindle_rpm);
    tool.feed = num(p, "feed").or(tool.feed);
    tool.plunge_feed = num(p, "plunge_feed").or(tool.plunge_feed);
    tool.stepdown = num(p, "stepdown").or(tool.stepdown);
    tool.stepover = num(p, "stepover").or(tool.stepover);
    // Omitted, a redefined tool keeps the cutting data it had.
    tool.cutting = match p.get("cutting") {
        Some(cutting) => serde_json::from_value(cutting.clone()).map_err(|e| bad(cmd, format!("`cutting`: {e}")))?,
        None => s.doc.cam.tool(number).map(|t| t.cutting.clone()).unwrap_or_default(),
    };
    tool.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let doc = s.doc_mut();
    if doc.cam.tools.len() >= MAX_TOOLS && !doc.cam.tools.iter().any(|t| t.number == number) {
        return Err(bad(cmd, format!("a design has at most {MAX_TOOLS} tools")));
    }
    match doc.cam.tools.iter_mut().find(|t| t.number == number) {
        Some(slot) => *slot = tool.clone(),
        None => doc.cam.tools.push(tool.clone()),
    }
    doc.cam.tools.sort_by_key(|t| t.number);
    Ok(json!({"tool": tool}))
}

/// The setup and operation a command names, the setup to change.
fn named_op<'a>(s: &'a mut Session, cmd: &str, p: &Value) -> Result<(&'a mut CamSetup, String)> {
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    if !s.doc.cam.setup(&setup_name).is_some_and(|setup| setup.operations.iter().any(|o| o.name() == name)) {
        return Err(bad(cmd, format!("no operation `{name}` in setup `{setup_name}`")));
    }
    let setup = s.doc_mut().cam.setup_mut(&setup_name).ok_or_else(|| bad(cmd, format!("no setup `{setup_name}`")))?;
    Ok((setup, name))
}

fn order_json(setup: &CamSetup) -> Value {
    json!({"setup": setup.name, "order": setup.operations.iter().map(CamOperation::name).collect::<Vec<_>>(), "skipped": setup.skipped})
}

fn move_op(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.op.move";
    let to = num(p, "to").filter(|n| *n >= 0.0 && n.fract() == 0.0).ok_or_else(|| bad(cmd, "`to` must be a place from 0"))? as usize;
    let (setup, name) = named_op(s, cmd, p)?;
    let from = setup.operations.iter().position(|o| o.name() == name).ok_or_else(|| bad(cmd, "the operation is gone"))?;
    let op = setup.operations.remove(from);
    setup.operations.insert(to.min(setup.operations.len()), op);
    Ok(order_json(setup))
}

fn skip_op(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.op.skip";
    let skip = bool_(p, "skip").ok_or_else(|| bad(cmd, "`skip` must be true or false"))?;
    let (setup, name) = named_op(s, cmd, p)?;
    setup.skipped.retain(|n| *n != name);
    if skip {
        setup.skipped.push(name);
    }
    Ok(order_json(setup))
}

fn speeds_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool.speeds";
    let number =
        num(p, "tool").filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u32).ok_or_else(|| bad(cmd, "`tool` must be a tool number"))?;
    let tool = s.doc.cam.tool(number).ok_or_else(|| bad(cmd, format!("no tool {number} (cam.tool.define first)")))?;
    let setup = match str_(p, "setup") {
        Some(name) => Some(s.doc.cam.setup(name.trim()).ok_or_else(|| bad(cmd, format!("no setup `{name}`")))?),
        None => None,
    };
    let material = str_(p, "material")
        .map(str::to_string)
        .or_else(|| setup.and_then(|x| x.material.clone()))
        .ok_or_else(|| bad(cmd, "`material` is required unless the setup names one"))?;
    let data = tool.cutting_for(&material).ok_or_else(|| bad(cmd, format!("tool {number} has no cutting data for `{material}`")))?;
    let mut speeds = data.speeds(tool, setup.and_then(|x| x.max_rpm)).map_err(|e| bad(cmd, e.to_string()))?;
    if let Some(machine) = setup.and_then(|x| s.doc.cam.setup_machine(x)) {
        speeds = machine.hold(speeds);
    }
    Ok(json!({
        "tool": number, "material": material, "cutting": data, "own": tool.cutting.contains_key(&material),
        "spindle_rpm": speeds.spindle_rpm, "feed": speeds.feed, "plunge_feed": speeds.plunge_feed,
        "stepdown": speeds.stepdown, "stepover": speeds.stepover,
    }))
}

fn tool_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool.remove";
    let number = num(p, "number")
        .filter(|n| *n >= 1.0 && *n <= 9999.0 && n.fract() == 0.0)
        .map(|n| n as u32)
        .ok_or_else(|| bad(cmd, "`number` must be an integer 1…9999"))?;
    let doc = s.doc_mut();
    if doc.cam.setups.iter().any(|setup| setup.operations.iter().any(|op| op.tool() == number)) {
        return Err(bad(cmd, format!("tool {number} is used by an operation")));
    }
    let before = doc.cam.tools.len();
    doc.cam.tools.retain(|t| t.number != number);
    if doc.cam.tools.len() == before {
        return Err(bad(cmd, format!("no tool {number}")));
    }
    Ok(json!({"deleted": number}))
}

/// Adds `machine` to the design, or replaces its machine of that name.
fn put_machine(s: &mut Session, cmd: &str, machine: Machine) -> Result<()> {
    machine.validate().map_err(|e| bad(cmd, e.to_string()))?;
    if let Some(post) = &machine.post {
        resolve_post(&s.doc.cam, post).map_err(|e| bad(cmd, format!("machine `{}`: {e}", machine.name)))?;
    }
    let doc = s.doc_mut();
    let full = doc.cam.machines.len() >= MAX_MACHINES;
    match doc.cam.machines.iter_mut().find(|m| m.name == machine.name) {
        Some(slot) => *slot = machine,
        None if full => return Err(bad(cmd, format!("a design has at most {MAX_MACHINES} machines"))),
        None => doc.cam.machines.push(machine),
    }
    doc.cam.machines.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(())
}

fn machine_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.machine.define";
    let machine: Machine = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    put_machine(s, cmd, machine.clone())?;
    Ok(json!({"machine": machine}))
}

fn machine_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.machine.remove";
    let name = str_(p, "name").ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    if let Some(setup) = s.doc.cam.setups.iter().find(|x| x.machine.as_deref() == Some(name.as_str())) {
        return Err(bad(cmd, format!("setup `{}` uses machine `{name}`", setup.name)));
    }
    let doc = s.doc_mut();
    let before = doc.cam.machines.len();
    doc.cam.machines.retain(|m| m.name != name);
    if doc.cam.machines.len() == before {
        return Err(bad(cmd, format!("no machine `{name}`")));
    }
    Ok(json!({"deleted": name}))
}

pub(super) fn setup_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.setup";
    let named = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).map(str::to_string);
    if bool_(p, "delete").unwrap_or(false) {
        let name = named.ok_or_else(|| bad(cmd, "`name` is required to delete a setup"))?;
        let doc = s.doc_mut();
        let before = doc.cam.setups.len();
        doc.cam.setups.retain(|x| x.name != name);
        if doc.cam.setups.len() == before {
            return Err(bad(cmd, format!("no setup `{name}`")));
        }
        return Ok(json!({"deleted": name}));
    }
    let name = named.unwrap_or_else(|| s.doc.cam.unused_setup_name());
    let existing = s.doc.cam.setup(&name).cloned();
    // A new name for an existing setup: its steps and settings stay, and so does its place in the list.
    let renamed = match str_(p, "rename").map(str::trim) {
        None => None,
        Some(n) if n == name => None,
        Some(n) if n.is_empty() || n.len() > 128 => return Err(bad(cmd, "`rename` is 1 to 128 characters")),
        Some(_) if existing.is_none() => return Err(bad(cmd, format!("no setup `{name}` to rename"))),
        Some(n) if s.doc.cam.setup(n).is_some() => return Err(bad(cmd, format!("a setup `{n}` already exists"))),
        Some(n) => Some(n.to_string()),
    };
    let work_offset = if p.get("work_offset").is_some() {
        num(p, "work_offset")
            .filter(|v| (1.0..=6.0).contains(v) && v.fract() == 0.0)
            .map(|v| v as u8)
            .ok_or_else(|| bad(cmd, "work_offset must be an integer 1 to 6"))?
    } else {
        existing.as_ref().map(|e| e.work_offset).unwrap_or(1)
    };
    let tool_axis = match p.get("tool_axis") {
        Some(v) => {
            let a = vec3(v).ok_or_else(|| bad(cmd, "`tool_axis` must be [x, y, z]"))?;
            [a.x, a.y, a.z]
        }
        None => existing.as_ref().map(|e| e.tool_axis).unwrap_or([0.0, 0.0, 1.0]),
    };
    let work_origin = match p.get("work_origin") {
        Some(v) => {
            let a = vec3(v).ok_or_else(|| bad(cmd, "`work_origin` must be [x, y, z]"))?;
            [a.x, a.y, a.z]
        }
        None => existing.as_ref().map(|e| e.work_origin).unwrap_or([0.0, 0.0, 0.0]),
    };
    let x_axis = match p.get("x_axis") {
        Some(v) => {
            let a = vec3(v).ok_or_else(|| bad(cmd, "`x_axis` must be [x, y, z]"))?;
            Some([a.x, a.y, a.z])
        }
        None => existing.as_ref().and_then(|e| e.x_axis),
    };
    let stock = match p.get("stock") {
        None => existing.as_ref().and_then(|e| e.stock),
        Some(v) if v.get("min").is_some() => Some(stock_box(cmd, v)?),
        Some(_) => existing.as_ref().and_then(|e| e.stock),
    };
    let mut setup = CamSetup::new(renamed.as_deref().unwrap_or(&name));
    setup.work_offset = work_offset;
    setup.tool_axis = tool_axis;
    setup.x_axis = x_axis;
    setup.work_origin = work_origin;
    setup.stock = stock;
    setup.stock_spec = parse_value(cmd, p, "stock_spec", existing.as_ref().map(|e| e.stock_spec.clone()).unwrap_or_default())?;
    setup.wcs = parse_value(cmd, p, "wcs", existing.as_ref().map(|e| e.wcs.clone()).unwrap_or_default())?;
    setup.wcs_orientation = parse_value(cmd, p, "wcs_orientation", existing.as_ref().map(|e| e.wcs_orientation).unwrap_or_default())?;
    setup.wcs_origin_mode =
        parse_value(cmd, p, "wcs_origin_mode", existing.as_ref().map(|e| e.wcs_origin_mode).unwrap_or(WcsOriginMode::SelectedPoint))?;
    setup.box_point = parse_value(cmd, p, "box_point", existing.as_ref().map(|e| e.box_point).unwrap_or_default())?;
    setup.flip_z = bool_(p, "flip_z").unwrap_or(existing.as_ref().map(|e| e.flip_z).unwrap_or(false));
    setup.flip_x = bool_(p, "flip_x").unwrap_or(existing.as_ref().map(|e| e.flip_x).unwrap_or(false));
    setup.model_bodies = if p.get("model_bodies").is_some() {
        string_list(cmd, p, "model_bodies")?
    } else {
        existing.as_ref().map(|e| e.model_bodies.clone()).unwrap_or_default()
    };
    setup.operation_type = parse_value(cmd, p, "operation_type", existing.as_ref().map(|e| e.operation_type).unwrap_or_default())?;
    setup.post_opts = parse_value(cmd, p, "post_opts", existing.as_ref().map(|e| e.post_opts.clone()).unwrap_or_default())?;
    // A machine given in full is copied into the design with the setup.
    let new_machine = match p.get("machine") {
        Some(v @ Value::Object(_)) => Some(serde_json::from_value::<Machine>(v.clone()).map_err(|e| bad(cmd, format!("`machine`: {e}")))?),
        _ => None,
    };
    setup.machine = match p.get("machine") {
        Some(Value::String(n)) if s.doc.cam.machine(n).is_some() => Some(n.clone()),
        Some(Value::String(n)) => return Err(bad(cmd, format!("no machine `{n}` (cam.machine.define first)"))),
        Some(Value::Object(_)) => new_machine.as_ref().map(|m| m.name.clone()),
        Some(Value::Null) => None,
        Some(_) => return Err(bad(cmd, "`machine` must be a name, a machine or null")),
        None => existing.as_ref().and_then(|e| e.machine.clone()),
    };
    setup.post = match p.get("post") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Null) => None,
        Some(_) => return Err(bad(cmd, "`post` must be a name or null")),
        None => existing.as_ref().and_then(|e| e.post.clone()),
    };
    if p.get("wcs").is_some() {
        setup.work_origin = setup.wcs.origin;
        setup.tool_axis = setup.wcs.z_axis();
        setup.x_axis = Some(setup.wcs.x_axis);
    } else if existing.is_none() {
        setup.wcs.origin = setup.work_origin;
    }
    if (1..=6).contains(&setup.post_opts.wcs_offset) && p.get("post_opts").is_some() && p.get("work_offset").is_none() {
        setup.work_offset = setup.post_opts.wcs_offset as u8;
    }
    setup.material = match p.get("material") {
        Some(Value::String(m)) => Some(m.trim().to_string()),
        Some(Value::Null) => None,
        Some(_) => return Err(bad(cmd, "`material` must be a material group name or null")),
        None => existing.as_ref().and_then(|e| e.material.clone()),
    };
    setup.max_rpm = match p.get("max_rpm") {
        Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().ok_or_else(|| bad(cmd, "`max_rpm` must be a number or null"))?),
        None => existing.as_ref().and_then(|e| e.max_rpm),
    };
    if let Some(existing) = &existing {
        setup.operations = existing.operations.clone();
        setup.skipped = existing.skipped.clone();
    }
    // Picked geometry sets the frame: Z+ out of a flat face, X+ along a straight edge.
    if let Some(pick) = p.get("z_face") {
        let (body, at) = pick_param(cmd, pick, "z_face")?;
        let z = face_normal(s, &body, at).map_err(|e| bad(cmd, e))?;
        setup.tool_axis = [z.x, z.y, z.z];
        if p.get("x_axis").is_none() {
            setup.x_axis = None;
        }
    }
    if let Some(pick) = p.get("x_edge") {
        let (body, at) = pick_param(cmd, pick, "x_edge")?;
        let x = edge_direction(s, &body, at, setup.tool_axis_vec()).map_err(|e| bad(cmd, e))?;
        setup.x_axis = Some([x.x, x.y, x.z]);
    }
    // Turning an axis round is done once, here; the setup keeps the turned axes.
    if bool_(p, "flip_z") == Some(true) {
        let z = setup.tool_axis;
        setup.tool_axis = [-z[0], -z[1], -z[2]];
    }
    if bool_(p, "flip_x") == Some(true) {
        let x = match setup.x_axis {
            Some(x) => x,
            None => work_frame(&setup).map(|f| [f.x.x, f.x.y, f.x.z]).map_err(|e| bad(cmd, e))?,
        };
        setup.x_axis = Some([-x[0], -x[1], -x[2]]);
    }
    (setup.flip_z, setup.flip_x) = (false, false);
    if let Some(pick) = p.get("zero_edge") {
        let (body, at) = pick_param(cmd, pick, "zero_edge")?;
        let o = edge_centre(s, &body, at).map_err(|e| bad(cmd, e))?;
        setup.work_origin = [o.x, o.y, o.z];
    }
    // A typed or picked part zero is a selected point; the box modes work it out in the new frame.
    if (p.get("work_origin").is_some() || p.get("zero_edge").is_some() || p.get("zero_axes").is_some()) && p.get("wcs_origin_mode").is_none() {
        setup.wcs_origin_mode = WcsOriginMode::SelectedPoint;
    }
    match setup.wcs_origin_mode {
        WcsOriginMode::ModelOrigin => setup.work_origin = [0.0; 3],
        WcsOriginMode::ModelBoxPoint | WcsOriginMode::StockBoxPoint => {
            let o = box_origin(s, &setup).map_err(|e| bad(cmd, e))?;
            setup.work_origin = [o.x, o.y, o.z];
        }
        WcsOriginMode::SelectedPoint => {}
    }
    // Part zero axis by axis: each pick moves zero along its own work axis only; an axis
    // without a pick keeps what the mode above gave it. The result is a fixed point.
    if let Some(picks) = p.get("zero_axes") {
        let picks = picks.as_object().ok_or_else(|| bad(cmd, "`zero_axes` must be {x?, y?, z?}"))?;
        if let Some(k) = picks.keys().find(|k| !matches!(k.as_str(), "x" | "y" | "z")) {
            return Err(bad(cmd, format!("`zero_axes` has no axis `{k}`")));
        }
        let frame = work_frame(&setup).map_err(|e| bad(cmd, e))?;
        let mut o = setup.work_origin_vec();
        for (key, axis) in [("x", frame.x), ("y", frame.y), ("z", frame.z)] {
            let Some(pick) = picks.get(key) else { continue };
            let (body, at) = pick_param(cmd, pick, "zero_axes")?;
            let point = if bool_(pick, "edge") == Some(true) { edge_centre(s, &body, at).map_err(|e| bad(cmd, e))? } else { at };
            o = o + axis * (point - o).dot(axis);
        }
        setup.work_origin = [o.x, o.y, o.z];
        setup.wcs_origin_mode = WcsOriginMode::SelectedPoint;
    }
    if let Ok(frame) = work_frame(&setup) {
        setup.wcs =
            WorkCoordinateSystem { origin: setup.work_origin, x_axis: [frame.x.x, frame.x.y, frame.x.z], y_axis: [frame.y.x, frame.y.y, frame.y.z] };
    }
    setup.validate().map_err(|e| bad(cmd, e.to_string()))?;
    if let Some(machine) = new_machine {
        put_machine(s, cmd, machine)?;
    }
    let doc = s.doc_mut();
    if doc.cam.setups.len() >= MAX_SETUPS && !doc.cam.setups.iter().any(|x| x.name == name) {
        return Err(bad(cmd, format!("a design has at most {MAX_SETUPS} setups")));
    }
    match doc.cam.setups.iter_mut().find(|x| x.name == name) {
        Some(slot) => {
            let (operations, skipped) = (std::mem::take(&mut slot.operations), std::mem::take(&mut slot.skipped));
            *slot = setup.clone();
            (slot.operations, slot.skipped) = (operations, skipped);
        }
        None => doc.cam.setups.push(setup.clone()),
    }
    Ok(json!({"setup": setup_json(s.doc.cam.setup(&setup.name).unwrap_or(&setup))}))
}

fn parse_value<T: serde::de::DeserializeOwned>(cmd: &str, p: &Value, k: &str, fallback: T) -> Result<T> {
    match p.get(k) {
        None => Ok(fallback),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("`{k}`: {e}"))),
    }
}

fn stock_box(cmd: &str, v: &Value) -> Result<StockBox> {
    let min = v.get("min").and_then(Value::as_array).filter(|a| a.len() == 3);
    let max = v.get("max").and_then(Value::as_array).filter(|a| a.len() == 3);
    let (Some(min), Some(max)) = (min, max) else {
        return Err(bad(cmd, "`stock` must be {min:[x,y,z], max:[x,y,z]}"));
    };
    let parse = |a: &[Value]| -> Result<[f64; 3]> {
        Ok([
            a[0].as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "stock coordinates must be finite"))?,
            a[1].as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "stock coordinates must be finite"))?,
            a[2].as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "stock coordinates must be finite"))?,
        ])
    };
    let box_ = StockBox { min: parse(min)?, max: parse(max)? };
    box_.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(box_)
}

pub(super) fn contour_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    op_cmd(s, p, "cam.op.contour", true)
}

pub(super) fn pocket_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    op_cmd(s, p, "cam.op.pocket", false)
}

pub(super) fn slot_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.op.slot";
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let tool_n =
        num(p, "tool").filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u32).ok_or_else(|| bad(cmd, "`tool` must be a tool number"))?;
    let tool = s.doc.cam.tool(tool_n).cloned().ok_or_else(|| bad(cmd, format!("no tool {tool_n} (cam.tool.define first)")))?;
    // Picked edges are the slot's path themselves, never the faces they bound.
    let mut p = p.clone();
    if p.get("edges").is_some() {
        p["chain"] = json!(true);
    }
    let p = &p;
    let geometry = geometry_param(s, cmd, p)?;
    let cut = cut_param(cmd, p, &tool)?;
    let stepover = expr(p, "stepover").unwrap_or_else(|| Expr::new(format!("{} mm", tool.diameter * 0.5)));
    let mut params = default_slot_params(stepover);
    params.width = expr(p, "width");
    for (key, slot) in [("stock_to_leave", &mut params.stock_to_leave), ("tolerance", &mut params.tolerance), ("ramp_angle", &mut params.ramp_angle)]
    {
        if let Some(v) = expr(p, key) {
            *slot = v;
        }
    }
    let op = CamOperation::Slot {
        name,
        tool: tool_n,
        geometry,
        params,
        cut,
        spindle_rpm: num(p, "spindle_rpm").or(tool.spindle_rpm),
        coolant: bool_(p, "coolant").unwrap_or(false),
    };
    store_op(s, cmd, &setup_name, op)
}

fn op_cmd(s: &mut Session, p: &Value, cmd: &str, is_contour: bool) -> Result<Value> {
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let tool_n =
        num(p, "tool").filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u32).ok_or_else(|| bad(cmd, "`tool` must be a tool number"))?;
    let tool = s.doc.cam.tool(tool_n).cloned().ok_or_else(|| bad(cmd, format!("no tool {tool_n} (cam.tool.define first)")))?;
    let geometry = geometry_param(s, cmd, p)?;
    let cut = cut_param(cmd, p, &tool)?;
    let spindle_rpm = num(p, "spindle_rpm").or(tool.spindle_rpm);
    let coolant = bool_(p, "coolant").unwrap_or(false);
    let op = if is_contour {
        let mut params = default_contour_params();
        if let Some(side) = str_(p, "side") {
            params.side = parse_side(cmd, side)?;
        }
        if let Some(c) = bool_(p, "climb") {
            params.climb = c;
        }
        if let Some(v) = expr(p, "stock_to_leave") {
            params.stock_to_leave = v;
        }
        if let Some(v) = expr(p, "floor_stock") {
            params.floor_stock = v;
        }
        if let Some(v) = expr(p, "tolerance") {
            params.tolerance = v;
        }
        if let Some(v) = expr(p, "ramp_angle") {
            params.ramp_angle = v;
        }
        if let Some(v) = p.get("start") {
            let v = vec3(v).ok_or_else(|| bad(cmd, "`start` must be [x, y, z]"))?;
            params.start = Some([v.x, v.y, v.z]);
        }
        if let Some(v) = p.get("lead_in") {
            params.lead_in = lead_param(cmd, v, "lead_in")?;
        }
        if let Some(v) = p.get("lead_out") {
            params.lead_out = lead_param(cmd, v, "lead_out")?;
        }
        for (key, slot) in [("overlap", &mut params.overlap), ("extend_start", &mut params.extend_start), ("extend_end", &mut params.extend_end)] {
            if let Some(v) = expr(p, key) {
                *slot = v;
            }
        }
        if let Some(side) = str_(p, "open_side") {
            params.open_side = match side {
                "left" => CamOpenSide::Left,
                "right" => CamOpenSide::Right,
                "on" => CamOpenSide::On,
                _ => return Err(bad(cmd, "`open_side` must be left, right or on")),
            };
        }
        if let Some(r) = bool_(p, "reverse") {
            params.reverse = r;
        }
        match p.get("tabs") {
            None => {}
            Some(Value::Null) => params.tabs = None,
            Some(v) => params.tabs = Some(Box::new(tabs_param(cmd, v)?)),
        }
        CamOperation::Contour { name: name.clone(), tool: tool_n, geometry, params, cut, spindle_rpm, coolant }
    } else {
        let stepover = match expr(p, "stepover") {
            Some(e) => e,
            None => match tool.stepover {
                Some(v) => Expr::new(format!("{v} mm")),
                None => Expr::new(format!("{} mm", tool.diameter * 0.5)),
            },
        };
        let mut params = default_pocket_params(stepover);
        if let Some(c) = bool_(p, "climb") {
            params.climb = c;
        }
        if let Some(v) = expr(p, "stock_to_leave") {
            params.stock_to_leave = v;
        }
        if let Some(v) = expr(p, "floor_stock") {
            params.floor_stock = v;
        }
        if let Some(v) = expr(p, "tolerance") {
            params.tolerance = v;
        }
        if let Some(v) = expr(p, "ramp_angle") {
            params.ramp_angle = v;
        }
        CamOperation::Pocket { name: name.clone(), tool: tool_n, geometry, params, cut, spindle_rpm, coolant }
    };
    store_op(s, cmd, &setup_name, op)
}

/// Checks `op`, makes its toolpath once, and puts it in setup `setup_name` in place of the
/// step of its name, else at the end.
fn store_op(s: &mut Session, cmd: &str, setup_name: &str, op: CamOperation) -> Result<Value> {
    let name = op.name().to_string();
    op.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let setup = s.doc.cam.setup(setup_name).cloned().ok_or_else(|| bad(cmd, format!("no setup `{setup_name}` (cam.setup first)")))?;
    build_path(s, &setup, &op).map_err(|e| bad(cmd, e))?;
    let doc = s.doc_mut();
    let setup = doc.cam.setup_mut(setup_name).ok_or_else(|| bad(cmd, format!("no setup `{setup_name}` (cam.setup first)")))?;
    if let Some(slot) = setup.operations.iter_mut().find(|o| o.name() == name) {
        *slot = op;
    } else {
        if setup.operations.len() >= MAX_OPS {
            return Err(bad(cmd, format!("a setup has at most {MAX_OPS} operations")));
        }
        setup.operations.push(op);
    }
    Ok(json!({"setup": setup_name, "operation": name}))
}

pub(super) fn face_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.op.face";
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let tool_n =
        num(p, "tool").filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u32).ok_or_else(|| bad(cmd, "`tool` must be a tool number"))?;
    let tool = s.doc.cam.tool(tool_n).cloned().ok_or_else(|| bad(cmd, format!("no tool {tool_n} (cam.tool.define first)")))?;
    let setup = s.doc.cam.setup(&setup_name).cloned().ok_or_else(|| bad(cmd, format!("no setup `{setup_name}` (cam.setup first)")))?;
    let picked = ["faces", "edges", "sketch", "geometry"].iter().any(|k| p.get(*k).is_some());
    let geometry = if picked {
        geometry_param(s, cmd, p)?
    } else if p.get("outer").is_some() || p.get("regions").is_some() {
        CamGeometry::Polygons { regions: regions_param(cmd, p)? }
    } else {
        CamGeometry::Stock
    };
    let frame = work_frame(&setup).map_err(|e| bad(cmd, e))?;
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    // The finished height: `bottom`, else part zero. A picked face only says where to face
    // (the base gives the whole footprint), not how deep.
    let mut p = p.clone();
    if p.get("bottom").is_none() && p.get("cut").is_none() {
        p["bottom"] = json!(0.0);
    }
    let mut cut = cut_param(cmd, &p, &tool)?;
    // Where cutting starts: `allowance` above the finished height, else `top`, else the stock's top.
    let top = match (num(&p, "allowance"), p.get("top").is_some() || p.get("cut").is_some()) {
        (Some(allowance), _) => {
            if !allowance.is_finite() || allowance <= 0.0 || allowance > 1.0e4 {
                return Err(bad(cmd, "`allowance` must be more than 0 mm"));
            }
            let bottom = num(&p, "bottom").ok_or_else(|| bad(cmd, "with `allowance`, `bottom` must be a number"))?;
            Some(round(bottom + allowance))
        }
        (None, true) => None,
        (None, false) => {
            let (_, hi) = stock_extent(s, &setup, &frame).map_err(|e| bad(cmd, e))?;
            Some(round(hi.z))
        }
    };
    if let Some(top) = top {
        if num(&p, "bottom").is_some_and(|b| b >= top) {
            return Err(bad(cmd, "nothing to face: the finished height is at or above the start; give an allowance above it"));
        }
        cut.top = Expr::new(format!("{top} mm"));
        // The safety heights stay above the start: given ones below it are raised.
        let retract = num(&p, "retract").filter(|r| *r > top).unwrap_or(round(top + 2.0));
        let clearance = num(&p, "clearance").filter(|c| *c >= retract).unwrap_or(round(retract.max(top + 10.0)));
        cut.retract = Expr::new(format!("{retract} mm"));
        cut.clearance = Expr::new(format!("{clearance} mm"));
    }
    let p = &p;
    let stepover = expr(p, "stepover").unwrap_or_else(|| Expr::new(format!("{} mm", tool.diameter * 0.7)));
    let mut params = default_face_params(stepover);
    if let Some(v) = expr(p, "overrun") {
        params.overrun = v;
    }
    let op = CamOperation::Face {
        name: name.clone(),
        tool: tool_n,
        geometry,
        params,
        cut,
        spindle_rpm: num(p, "spindle_rpm").or(tool.spindle_rpm),
        coolant: bool_(p, "coolant").unwrap_or(false),
    };
    op.validate().map_err(|e| bad(cmd, e.to_string()))?;
    build_path(s, &setup, &op).map_err(|e| bad(cmd, e))?;
    let setup = s.doc_mut().cam.setup_mut(&setup_name).ok_or_else(|| bad(cmd, format!("no setup `{setup_name}`")))?;
    if let Some(slot) = setup.operations.iter_mut().find(|o| o.name() == name) {
        *slot = op;
    } else {
        if setup.operations.len() >= MAX_OPS {
            return Err(bad(cmd, format!("a setup has at most {MAX_OPS} operations")));
        }
        setup.operations.push(op);
    }
    Ok(json!({"setup": setup_name, "operation": name}))
}

/// Holding tabs: `{count?, spacing?, width, height}`, a count or a spacing.
fn tabs_param(cmd: &str, v: &Value) -> Result<CamTabs> {
    let need = |k: &str| expr(v, k).ok_or_else(|| bad(cmd, format!("`tabs` needs `{k}`")));
    let count = match v.get("count") {
        None => 0,
        Some(c) => c
            .as_u64()
            .and_then(|c| u32::try_from(c).ok())
            .filter(|c| *c <= 1000)
            .ok_or_else(|| bad(cmd, "`tabs.count` must be a whole number 0 to 1000"))?,
    };
    let spacing = expr(v, "spacing").unwrap_or_else(|| Expr::new("0 mm"));
    if count == 0 && v.get("spacing").is_none() {
        return Err(bad(cmd, "`tabs` needs a `count` or a `spacing`"));
    }
    Ok(CamTabs { count, spacing, width: need("width")?, height: need("height")? })
}

/// A lead: `{kind: direct|arc|tangent|perpendicular, radius?, sweep?, length?}`.
fn lead_param(cmd: &str, v: &Value, key: &str) -> Result<CamLead> {
    let need = |k: &str| expr(v, k).ok_or_else(|| bad(cmd, format!("`{key}` needs `{k}`")));
    match v.get("kind").and_then(Value::as_str) {
        Some("direct") => Ok(CamLead::Direct),
        Some("arc") => Ok(CamLead::Arc { radius: need("radius")?, sweep: expr(v, "sweep").unwrap_or_else(|| Expr::new("90 deg")) }),
        Some("tangent") => Ok(CamLead::Tangent { length: need("length")? }),
        Some("perpendicular") => Ok(CamLead::Perpendicular { length: need("length")? }),
        _ => Err(bad(cmd, format!("`{key}` must be {{kind: direct|arc|tangent|perpendicular, ...}}"))),
    }
}

fn parse_side(cmd: &str, side: &str) -> Result<CamSide> {
    match side {
        "auto" => Ok(CamSide::Auto),
        "outside" => Ok(CamSide::Outside),
        "inside" => Ok(CamSide::Inside),
        "on" => Ok(CamSide::On),
        other => Err(bad(cmd, format!("`side` must be auto, outside, inside or on (not `{other}`)"))),
    }
}

fn geometry_param(s: &Session, cmd: &str, p: &Value) -> Result<CamGeometry> {
    // A step's own stored geometry, as an edit hands it back unchanged.
    if let Some(g) = p.get("geometry") {
        let g: CamGeometry = serde_json::from_value(g.clone()).map_err(|e| bad(cmd, format!("`geometry`: {e}")))?;
        g.validate().map_err(|e| bad(cmd, e.to_string()))?;
        return Ok(g);
    }
    if p.get("sketch").is_some() {
        let key = super::sketch_key(p.get("sketch")).ok_or_else(|| bad(cmd, "`sketch` must be a sketch id or name"))?;
        let sketch = match s.doc.find_feature(&key) {
            Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => f.id,
            _ => return Err(bad(cmd, format!("no sketch `{key}`"))),
        };
        let profiles = super::feature_profiles(p, cmd)?;
        return Ok(CamGeometry::Sketch { sketch, profiles });
    }
    let face_list = p.get("faces").and_then(Value::as_array);
    let edge_list = p.get("edges").and_then(Value::as_array);
    if face_list.is_some() || edge_list.is_some() {
        let (arr, edges) = (face_list.map_or(&[][..], Vec::as_slice), edge_list.map_or(&[][..], Vec::as_slice));
        if arr.is_empty() && edges.is_empty() {
            return Err(bad(cmd, "`faces` and `edges` are empty"));
        }
        let mut chain = Vec::new();
        for v in edges {
            let body = v.get("body").and_then(Value::as_str).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "each edge needs `body`"))?;
            let at = v.get("at").and_then(vec3).ok_or_else(|| bad(cmd, "each edge needs `at`: [x, y, z]"))?;
            chain.push(CamEdgeRef { body: body.to_string(), at: [at.x, at.y, at.z] });
        }
        let setup =
            || str_(p, "setup").and_then(|n| s.doc.cam.setup(n.trim())).ok_or_else(|| bad(cmd, "edges need an existing setup (cam.setup first)"));
        // On request, or when an edge bounds no flat face square to the tool axis, the picked
        // edges themselves are the contour.
        let mut faces = Vec::new();
        let mut loose = bool_(p, "chain").unwrap_or(false);
        if !loose {
            for e in &chain {
                let at = Vec3::new(e.at[0], e.at[1], e.at[2]);
                match edge_face(s, setup()?, &e.body, at) {
                    Ok(face) => faces.push(face),
                    Err(_) if s.model.state().body(&e.body).is_some() => loose = true,
                    Err(err) => return Err(bad(cmd, err)),
                }
            }
        }
        if loose && !chain.is_empty() {
            if !arr.is_empty() {
                return Err(bad(cmd, "a chain of edges takes no faces: pick edges only, or faces only"));
            }
            let geometry = CamGeometry::Edges { edges: chain };
            geometry.validate().map_err(|e| bad(cmd, e.to_string()))?;
            let frame = work_frame(setup()?).map_err(|e| bad(cmd, e))?;
            edge_chain(s, &frame, &edge_refs(&geometry)).map_err(|e| bad(cmd, e))?;
            return Ok(geometry);
        }
        for v in arr {
            let body = v.get("body").and_then(Value::as_str).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "each face needs `body`"))?;
            // A face the model has no name for is found again by `at`.
            let name = v.get("name").and_then(Value::as_str).unwrap_or("");
            let at = match v.get("at") {
                None => [0.0, 0.0, 0.0],
                Some(a) => {
                    let p = vec3(a).ok_or_else(|| bad(cmd, "`at` must be [x, y, z]"))?;
                    [p.x, p.y, p.z]
                }
            };
            let contour_at = match v.get("contour_at") {
                None => None,
                Some(a) => {
                    let p = vec3(a).ok_or_else(|| bad(cmd, "`contour_at` must be [x, y, z]"))?;
                    Some([p.x, p.y, p.z])
                }
            };
            faces.push(CamFaceRef { body: body.to_string(), name: name.to_string(), at, contour_at });
        }
        return Ok(CamGeometry::Faces { faces });
    }
    Ok(CamGeometry::Polygons { regions: regions_param(cmd, p)? })
}

fn regions_param(cmd: &str, p: &Value) -> Result<Vec<PolygonRegion>> {
    if let Some(arr) = p.get("regions").and_then(Value::as_array) {
        return arr.iter().map(|v| polygon_region(cmd, v)).collect();
    }
    let outer = vec2_list(cmd, p, "outer", solidcraft_doc::cam::MAX_LOOP_POINTS)?;
    if outer.len() < 3 {
        return Err(bad(cmd, "`outer` needs at least 3 points (or pass `sketch`, `faces`, or `regions`)"));
    }
    let holes = match p.get("holes") {
        None => Vec::new(),
        Some(Value::Array(hs)) => hs
            .iter()
            .map(|h| {
                let pts = h.as_array().ok_or_else(|| bad(cmd, "`holes` must be a list of point lists"))?;
                if pts.len() < 3 || pts.len() > solidcraft_doc::cam::MAX_LOOP_POINTS {
                    return Err(bad(cmd, "each hole needs 3 to 10000 points"));
                }
                pts.iter().map(|v| vec2(v).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "hole points must be [x, y]"))).collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(bad(cmd, "`holes` must be a list of point lists")),
    };
    let region = PolygonRegion { outer: outer.into_iter().map(|p| [p.x, p.y]).collect(), holes };
    region.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(vec![region])
}

fn polygon_region(cmd: &str, v: &Value) -> Result<PolygonRegion> {
    let outer = v
        .get("outer")
        .and_then(Value::as_array)
        .ok_or_else(|| bad(cmd, "each region needs `outer`: [[x,y],…]"))?
        .iter()
        .map(|p| vec2(p).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "region points must be [x, y]")))
        .collect::<Result<Vec<_>>>()?;
    let holes = match v.get("holes") {
        None => Vec::new(),
        Some(Value::Array(hs)) => hs
            .iter()
            .map(|h| {
                h.as_array()
                    .ok_or_else(|| bad(cmd, "holes must be point lists"))?
                    .iter()
                    .map(|p| vec2(p).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "hole points must be [x, y]")))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(bad(cmd, "`holes` must be a list of point lists")),
    };
    let region = PolygonRegion { outer, holes };
    region.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(region)
}

fn cut_param(cmd: &str, p: &Value, tool: &Tool) -> Result<CamCut> {
    let src = p.get("cut").unwrap_or(p);
    let bottom = req_expr(cmd, src, "bottom")?;
    let top = expr(src, "top").unwrap_or_else(|| Expr::new("0 mm"));
    let retract = expr(src, "retract").unwrap_or_else(|| Expr::new("2 mm"));
    let clearance = expr(src, "clearance").unwrap_or_else(|| Expr::new("10 mm"));
    let stepdown = match expr(src, "stepdown") {
        Some(e) => e,
        None => match tool.stepdown {
            Some(v) => Expr::new(format!("{v} mm")),
            None => Expr::new(format!("{} mm", tool.diameter)),
        },
    };
    let feed = match expr(src, "feed") {
        Some(e) => e,
        None => match tool.feed {
            Some(v) => Expr::new(format!("{v} mm")),
            None => Expr::new("600 mm"),
        },
    };
    let plunge_feed = match expr(src, "plunge_feed") {
        Some(e) => e,
        None => match tool.plunge_feed {
            Some(v) => Expr::new(format!("{v} mm")),
            None => Expr::new("200 mm"),
        },
    };
    let mut cut = default_cut(bottom, stepdown);
    cut.top = top;
    cut.retract = retract;
    cut.clearance = clearance;
    cut.feed = feed;
    cut.plunge_feed = plunge_feed;
    Ok(cut)
}

fn delete_op(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.op.remove";
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let doc = s.doc_mut();
    let setup = doc.cam.setup_mut(&setup_name).ok_or_else(|| bad(cmd, format!("no setup `{setup_name}`")))?;
    let before = setup.operations.len();
    setup.operations.retain(|o| o.name() != name);
    if setup.operations.len() == before {
        return Err(bad(cmd, format!("no operation `{name}` in setup `{setup_name}`")));
    }
    setup.skipped.retain(|n| *n != name);
    Ok(json!({"deleted": name, "setup": setup_name}))
}

pub(super) fn post_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    post_program(s, p)
}

/// `cam.post` without changing anything, for [`Session::cam_program`] too.
fn post_program(s: &Session, p: &Value) -> Result<Value> {
    let cmd = "cam.post";
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let setup = s.doc.cam.setup(&setup_name).cloned().ok_or_else(|| bad(cmd, format!("no setup `{setup_name}`")))?;
    if setup.operations.iter().all(|op| setup.skipped.iter().any(|n| n == op.name())) {
        return Err(bad(cmd, format!("setup `{setup_name}` has no operations to post")));
    }
    let post_name = str_(p, "post").map(str::to_string).or_else(|| chosen_post(&s.doc.cam, &setup)).unwrap_or_else(|| "rs274".into());
    let post = resolve_post(&s.doc.cam, &post_name).map_err(|e| bad(cmd, e))?;
    post.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let program_name = str_(p, "program").unwrap_or(setup.name.as_str()).to_string();
    let mut program = Program { name: program_name, operations: Vec::new(), work_offset: Some(setup.work_offset) };
    let mut move_count = 0usize;
    let machine = match &setup.machine {
        Some(name) => Some(
            s.doc
                .cam
                .machine(name)
                .cloned()
                .ok_or_else(|| bad(cmd, format!("setup `{setup_name}` names machine `{name}`, which the design lacks")))?,
        ),
        None => None,
    };
    let mut reach: Option<([f64; 3], [f64; 3])> = None;
    for op in setup.operations.iter().filter(|op| !setup.skipped.iter().any(|n| n == op.name())) {
        let tool = s.doc.cam.tool(op.tool()).cloned().ok_or_else(|| bad(cmd, format!("operation `{}` needs tool {}", op.name(), op.tool())))?;
        let path = build_path(s, &setup, op).map_err(|e| bad(cmd, e))?;
        move_count = move_count.saturating_add(path.moves.len());
        if move_count > 2_000_000 {
            return Err(bad(cmd, "setup exceeds 2000000 toolpath moves; split the job"));
        }
        let (spindle_rpm, coolant) = (op.spindle_rpm().or(tool.spindle_rpm).unwrap_or(6000.0), op.coolant());
        if let Some(machine) = &machine {
            machine
                .check_operation(spindle_rpm, &path)
                .map_err(|e| bad(cmd, format!("operation `{}` on machine `{}`: {e}", op.name(), machine.name)))?;
            if let Some((lo, hi)) = solidcraft_cam::reach(&path) {
                reach = Some(match reach {
                    None => (lo, hi),
                    Some((a, b)) => (std::array::from_fn(|i| a[i].min(lo[i])), std::array::from_fn(|i| b[i].max(hi[i]))),
                });
            }
        }
        program.operations.push(Operation { name: op.name().into(), tool, spindle_rpm, coolant, path });
    }
    if let (Some(machine), Some((lo, hi))) = (&machine, reach) {
        machine.check_reach(lo, hi).map_err(|e| bad(cmd, format!("machine `{}`: {e}", machine.name)))?;
    }
    let gcode = emit(&program, &post).map_err(|e| bad(cmd, e.to_string()))?;
    let mut out = json!({
        "gcode": gcode,
        "extension": post.extension,
        "post": post.name,
        "operations": program.operations.len(),
        "work_offset": setup.work_offset,
    });
    let rb = readback(&gcode, &program, &post).map_err(|e| bad(cmd, e.to_string()))?;
    if !rb.within() {
        return Err(bad(cmd, format!("G-code read-back deviation {:.4} mm exceeds allowed {:.4} mm", rb.max_deviation, rb.allowed)));
    }
    out["readback"] = json!({
        "max_deviation": rb.max_deviation,
        "allowed": rb.allowed,
        "cut_length": rb.cut_length,
        "cut_minutes": rb.cut_minutes,
        "within": rb.within(),
    });
    Ok(out)
}

/// The post a setup's programs are written with when the caller names none: the setup's own,
/// else its machine's.
pub(super) fn chosen_post(cam: &CamSettings, setup: &CamSetup) -> Option<String> {
    setup.post.clone().or_else(|| setup.machine.as_deref().and_then(|m| cam.machine(m)).and_then(|m| m.post.clone()))
}

fn resolve_post(cam: &CamSettings, name: &str) -> std::result::Result<Post, String> {
    if let Some(p) = cam.post(name) {
        return Ok(p.clone());
    }
    Post::builtin(name).ok_or_else(|| format!("unknown post `{name}` (built-in: {}; or save one on the design)", Post::BUILTIN.join(", ")))
}

struct WorkFrame {
    o: Vec3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl WorkFrame {
    fn local(&self, p: Vec3) -> Vec3 {
        let d = p - self.o;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    fn model(&self, p: Vec3) -> Vec3 {
        self.o + self.x * p.x + self.y * p.y + self.z * p.z
    }
}

/// A point in a setup's work coordinates, in model coordinates.
pub(super) fn work_to_model(setup: &CamSetup, p: Vec3) -> std::result::Result<Vec3, String> {
    Ok(work_frame(setup)?.model(p))
}

/// Work X and Y for a tool axis. Along a world axis they are world axes too.
fn axes(axis: Vec3) -> Option<(Vec3, Vec3, Vec3)> {
    let z = axis.normalized().filter(|z| z.is_finite())?;
    let x = if z.z.abs() > 1.0 - 1e-9 || z.y.abs() > 1.0 - 1e-9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else if z.x.abs() > 1.0 - 1e-9 {
        Vec3::new(0.0, z.x.signum(), 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0).cross(z).normalized()?
    };
    Some((x, z.cross(x), z))
}

fn work_frame(setup: &CamSetup) -> std::result::Result<WorkFrame, String> {
    let z = setup.tool_axis_vec().normalized().filter(|z| z.is_finite()).ok_or("the setup's tool axis is not a direction")?;
    let x = match setup.x_axis {
        Some(a) => {
            let x = Vec3::new(a[0], a[1], a[2]).normalized().filter(|x| x.is_finite()).ok_or("the setup's X axis is not a direction")?;
            if x.dot(z).abs() > 1e-6 {
                return Err("the setup's X axis must be square to the tool axis".into());
            }
            x
        }
        None => axes(z).ok_or("the setup's tool axis is not a direction")?.0,
    };
    let y = z.cross(x);
    let y = y.normalized().filter(|y| y.is_finite()).ok_or("the setup's axes do not form a frame")?;
    Ok(WorkFrame { o: setup.work_origin_vec(), x, y, z })
}

/// Sketch-plane regions in a setup's XY. The sketch plane must face along the tool axis.
fn to_setup(plane: &Plane, frame: &WorkFrame, regions: &[Region2]) -> std::result::Result<Vec<Region2>, String> {
    let facing = plane.normal().dot(frame.z);
    if !(facing.abs() > 1.0 - 1e-9) {
        return Err("not supported yet: operations on a sketch or face that is not square to the setup's tool axis".into());
    }
    let mirrored = facing < 0.0;
    let x_axis = plane.dir_to_world(Vec2::new(1.0, 0.0));
    let turn = x_axis.dot(frame.y).atan2(x_axis.dot(frame.x));
    let point = |p: Vec2| {
        let w = frame.local(plane.to_world(p));
        Vec2::new(w.x, w.y)
    };
    let seg = |s: &Seg2| match *s {
        Seg2::Line { a, b } => Seg2::Line { a: point(a), b: point(b) },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc {
            center: point(center),
            radius,
            start: if mirrored { turn - start } else { turn + start },
            sweep: if mirrored { -sweep } else { sweep },
        },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: point(p0), p1: point(p1), p2: point(p2), p3: point(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: point(a), apex: point(apex), b: point(b), w },
        Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(point) },
    };
    let lp = |l: &Loop2| Loop2 { segs: l.segs.iter().map(seg).collect() };
    Ok(regions.iter().map(|r| Region2 { outer: lp(&r.outer), holes: r.holes.iter().map(lp).collect() }).collect())
}

fn outline(geom: &[LinkGeom]) -> std::result::Result<Region2, String> {
    let mut loops: Vec<Loop2> = Vec::new();
    let mut open: Vec<Seg2> = Vec::new();
    for g in geom {
        match *g {
            LinkGeom::Point(_) => {}
            LinkGeom::Line(a, b) => open.push(Seg2::Line { a, b }),
            LinkGeom::Circle(c, r) => loops.push(Loop2::circle(c, r)),
            LinkGeom::Arc { c, a, b } => {
                let (start, end) = ((a - c).angle(), (b - c).angle());
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                open.push(Seg2::Arc { center: c, radius: a.dist(c), start, sweep: if sweep > 1e-12 { sweep } else { std::f64::consts::TAU } });
            }
            LinkGeom::Conic { a, apex, b, rho } if rho > 1e-6 && rho < 1.0 - 1e-6 => open.push(Seg2::Conic { a, apex, b, w: rho / (1.0 - rho) }),
            _ => return Err("not supported yet: faces with elliptical or spline edges".into()),
        }
    }
    if open.len() > 20_000 {
        return Err("the face has too many edges".into());
    }
    loops.extend(join_loops(open).map_err(|_| "the face's edges do not form closed loops".to_string())?);
    let outer = loops
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.signed_area().abs().total_cmp(&b.1.signed_area().abs()))
        .map(|(i, _)| i)
        .ok_or("the face has no outline")?;
    let outer = loops.remove(outer);
    Ok(Region2 { outer, holes: loops })
}

/// Segments joined end to end into closed loops (either way round); else the open end where a
/// loop stops.
fn join_loops(open: Vec<Seg2>) -> std::result::Result<Vec<Loop2>, Vec2> {
    let (loops, chains) = join_chains(open);
    match chains.first().and_then(|c| c.last()) {
        Some(seg) => Err(seg.end()),
        None => Ok(loops),
    }
}

/// Segments joined end to end (either way round): the closed loops, and the open chains left
/// over, each in order from one free end to the other.
fn join_chains(open: Vec<Seg2>) -> (Vec<Loop2>, Vec<Vec<Seg2>>) {
    const JOIN: f64 = 1e-3;
    let (mut loops, mut chains) = (Vec::new(), Vec::new());
    let mut used = vec![false; open.len()];
    // The unused segment meeting `at`, turned to start there.
    let take = |used: &mut Vec<bool>, at: Vec2| -> Option<Seg2> {
        let (i, seg) = open.iter().zip(used.iter()).enumerate().filter(|(_, (_, taken))| !**taken).find_map(|(i, (seg, _))| {
            if seg.start().dist(at) <= JOIN {
                Some((i, *seg))
            } else if seg.end().dist(at) <= JOIN {
                Some((i, seg.reversed()))
            } else {
                None
            }
        })?;
        if let Some(flag) = used.get_mut(i) {
            *flag = true;
        }
        Some(seg)
    };
    for first in 0..open.len() {
        if used.get(first).copied().unwrap_or(true) {
            continue;
        }
        let Some(seed) = open.get(first).copied() else { continue };
        if let Some(flag) = used.get_mut(first) {
            *flag = true;
        }
        let mut chain = vec![seed];
        let origin = seed.start();
        let mut end = seed.end();
        let mut closed = end.dist(origin) <= JOIN;
        while !closed {
            let Some(seg) = take(&mut used, end) else { break };
            end = seg.end();
            chain.push(seg);
            closed = end.dist(origin) <= JOIN;
        }
        if closed {
            loops.push(Loop2 { segs: chain });
            continue;
        }
        // Open: on back from the seed's start too.
        let mut front = Vec::new();
        let mut at = origin;
        while let Some(seg) = take(&mut used, at) {
            at = seg.end();
            front.push(seg.reversed());
        }
        chains.push(front.into_iter().rev().chain(chain).collect());
    }
    (loops, chains)
}

/// How far from a contour a point picked on its edge may lie (mm): edges are picked on their
/// tessellation.
const ON_CONTOUR: f64 = 0.5;

fn loop_dist(lp: &Loop2, p: Vec2) -> f64 {
    lp.segs.iter().map(|seg| seg.point_at(seg.closest_param(p)).dist(p)).fold(f64::INFINITY, f64::min)
}

/// The face reference for the contour through a picked edge point: the planar face square to
/// the setup's tool axis that the edge bounds, one facing along the axis first.
fn edge_face(s: &Session, setup: &CamSetup, body_name: &str, at: Vec3) -> std::result::Result<CamFaceRef, String> {
    let frame = work_frame(setup)?;
    let index = edge_face_index(s, &frame, body_name, at)?;
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    // A face without a name is found again by the picked point.
    let name = solidcraft_doc::naming::face_names(body).get(index).cloned().unwrap_or_default();
    Ok(CamFaceRef { body: body_name.to_string(), name, at: [at.x, at.y, at.z], contour_at: Some([at.x, at.y, at.z]) })
}

/// The index of the face [`edge_face`] takes for a picked edge point.
fn edge_face_index(s: &Session, frame: &WorkFrame, body_name: &str, at: Vec3) -> std::result::Result<usize, String> {
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    let mut best: Option<((bool, f64), usize)> = None;
    for index in 0..body.body.face_count().min(20_000) {
        let Some(plane) = body.body.planar_face(index) else { continue };
        let facing = plane.normal().dot(frame.z);
        let local = plane.to_local(at);
        if facing.abs() < 1.0 - 1e-9 || plane.to_world(local).dist(at) > ON_CONTOUR {
            continue;
        }
        let Ok(resolved) = resolve_face_indices(&state, &plane, LinkKind::Project, body_name, &[index]) else { continue };
        let Ok(region) = outline(&resolved.geom) else { continue };
        let d = std::iter::once(&region.outer).chain(&region.holes).map(|lp| loop_dist(lp, local)).fold(f64::INFINITY, f64::min);
        let key = (facing < 0.0, d);
        if d <= ON_CONTOUR && best.as_ref().is_none_or(|(k, _)| key < *k) {
            best = Some((key, index));
        }
    }
    let (_, index) = best.ok_or("the picked edge bounds no flat face square to the setup's tool axis")?;
    Ok(index)
}

/// The flat face of `body` a picked point `at` lies on (inside its outline, not in a hole).
fn face_index_at(s: &Session, body_name: &str, at: Vec3) -> std::result::Result<usize, String> {
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    let tol = (body.body.size() * 1e-4).max(1e-4);
    for index in 0..body.body.face_count().min(20_000) {
        let Some(plane) = body.body.planar_face(index) else { continue };
        let local = plane.to_local(at);
        if plane.to_world(local).dist(at) > tol {
            continue;
        }
        let Ok(resolved) = resolve_face_indices(&state, &plane, LinkKind::Project, body_name, &[index]) else { continue };
        let Ok(region) = outline(&resolved.geom) else { continue };
        if region.outer.contains(local) && !region.holes.iter().any(|h| h.contains(local)) {
            return Ok(index);
        }
    }
    Err(format!("its face on `{body_name}` is gone"))
}

/// The edge picks of a [`CamGeometry::Edges`] as body names and model points.
fn edge_refs(geometry: &CamGeometry) -> Vec<(&str, Vec3)> {
    match geometry {
        CamGeometry::Edges { edges } => edges.iter().map(|e| (e.body.as_str(), Vec3::new(e.at[0], e.at[1], e.at[2]))).collect(),
        _ => Vec::new(),
    }
}

/// The closed contours picked edges make, seen along the tool axis in the setup's XY: each
/// pick takes the body edge nearest it, and the edges are joined end to end. Circles and arcs
/// square to the tool axis stay arcs.
fn edge_chain(s: &Session, frame: &WorkFrame, picks: &[(&str, Vec3)]) -> std::result::Result<(Vec<Region2>, Vec<Vec<Seg2>>), String> {
    let state = s.model.state();
    let flat = |p: Vec3| {
        let w = frame.local(p);
        Vec2::new(w.x, w.y)
    };
    let mut taken: Vec<(&str, usize)> = Vec::new();
    let mut loops: Vec<Loop2> = Vec::new();
    let mut open: Vec<Seg2> = Vec::new();
    for &(body_name, at) in picks {
        let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
        let mesh = body.mesh();
        let index = nearest_edge(&mesh.edges, at).ok_or_else(|| format!("no edge of `{body_name}` at the picked point"))?;
        // A round rim split into pieces is taken whole, as the viewport shows it picked; an
        // edge picked twice counts once.
        for index in mesh.circular_rim(index) {
            if taken.contains(&(body_name, index)) {
                continue;
            }
            taken.push((body_name, index));
            let pts = mesh.edges.get(index).ok_or("the picked edge is gone")?;
            if pts.len() > 20_000 || open.len() > 20_000 {
                return Err("the picked edges have too many points".into());
            }
            let tol = (body.body.size() * 1e-6).max(1e-6) * 10.0;
            let circle = solidcraft_kernel::edge_circle(&body.body, index).filter(|c| {
                let ends = [pts.first(), pts.last()].into_iter().flatten().all(|p| p.dist(c.start).min(p.dist(c.end)) <= tol);
                ends && pts.iter().all(|p| (p.dist(c.center) - c.radius).abs() <= tol) && c.axis.dot(frame.z).abs() > 1.0 - 1e-9
            });
            match circle {
                Some(c) if c.angle >= std::f64::consts::TAU - 1e-9 => loops.push(Loop2::circle(flat(c.center), c.radius)),
                Some(c) => {
                    let (center, a) = (flat(c.center), flat(c.start));
                    let sweep = if c.axis.dot(frame.z) > 0.0 { c.angle } else { -c.angle };
                    open.push(Seg2::Arc { center, radius: c.radius, start: (a - center).angle(), sweep });
                }
                None => {
                    let mut prev: Option<Vec2> = None;
                    for p in pts.iter().map(|p| flat(*p)) {
                        match prev {
                            Some(a) if a.dist(p) <= 1e-9 => continue,
                            Some(a) => open.push(Seg2::Line { a, b: p }),
                            None => {}
                        }
                        prev = Some(p);
                    }
                }
            }
        }
    }
    let (joined, mut chains) = join_chains(open);
    // An open chain runs from the first picked edge towards the second.
    if let [(_, first), (_, second), ..] = picks {
        let (first, second) = (flat(*first), flat(*second));
        for chain in &mut chains {
            let place =
                |p: Vec2| chain.iter().enumerate().map(|(i, seg)| (i, seg.point_at(seg.closest_param(p)).dist(p))).min_by(|a, b| a.1.total_cmp(&b.1));
            if let (Some((i, a)), Some((j, b))) = (place(first), place(second))
                && a <= ON_CONTOUR
                && b <= ON_CONTOUR
                && i > j
            {
                *chain = chain.iter().rev().map(Seg2::reversed).collect();
            }
        }
    }
    loops.extend(joined);
    if loops.iter().any(|lp| lp.signed_area().abs() < 1e-6) {
        return Err("the picked edges enclose nothing seen along the setup's tool axis".into());
    }
    if chains.iter().any(|c| c.iter().map(Seg2::length).sum::<f64>() < 1e-6) {
        return Err("the picked edges have no length seen along the setup's tool axis".into());
    }
    Ok((loops.into_iter().map(|lp| Region2 { outer: lp.ccw(), holes: Vec::new() }).collect(), chains))
}

/// Where an open chain stops, for a message: its end, in work XY and in the model.
fn open_end_note(frame: &WorkFrame, chains: &[Vec<Seg2>]) -> String {
    match chains.first().and_then(|c| c.last()) {
        Some(seg) => {
            let gap = seg.end();
            let at = frame.model(Vec3::new(gap.x, gap.y, 0.0));
            format!(": it is open near X {:.2} Y {:.2} (model {:.2}, {:.2}, {:.2})", gap.x, gap.y, at.x, at.y, at.z)
        }
        None => String::new(),
    }
}

/// The tessellated edge nearest `at`, if one passes within [`ON_CONTOUR`].
fn nearest_edge(edges: &[Vec<Vec3>], at: Vec3) -> Option<usize> {
    let near = |pts: &[Vec3]| {
        pts.windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(seg3_dist(*a, *b, at)),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min)
    };
    edges.iter().enumerate().map(|(i, pts)| (i, near(pts))).filter(|(_, d)| *d <= ON_CONTOUR).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
}

/// The normal of the plane through `pts`. Points on one straight line give the tool axis when
/// the line is square to it; otherwise there is no plane.
fn plane_normal(pts: &[Vec3], axis: Vec3, tol: f64) -> std::result::Result<Vec3, String> {
    let first = *pts.first().ok_or("the picked edges have no points")?;
    let far = pts.iter().copied().max_by(|a, b| a.dist(first).total_cmp(&b.dist(first))).unwrap_or(first);
    let along = (far - first).normalized().filter(|d| d.is_finite()).ok_or("the picked edges have no length")?;
    let off = |p: Vec3| {
        let d = p - first;
        (d - along * d.dot(along)).len()
    };
    let side = pts.iter().copied().max_by(|a, b| off(*a).total_cmp(&off(*b))).unwrap_or(first);
    let normal = if off(side) > tol {
        along.cross(side - first).normalized().filter(|n| n.is_finite()).ok_or("the picked edges give no plane")?
    } else if along.dot(axis).abs() < 1e-6 {
        axis
    } else {
        return Err("a single straight edge gives no plane: pick a second edge of the chain".into());
    };
    if pts.iter().any(|p| (*p - first).dot(normal).abs() > tol) {
        return Err("the picked edges are not in one plane".into());
    }
    Ok(normal)
}

/// The chain of edges of `body_name` in the plane of the picked ones, joined end to end: from
/// the first pick both ways, at each corner the edge that was picked, else the one that turns
/// least, until the chain comes round to its start or ends. Each edge with its index and the
/// point halfway along it; and whether the chain is closed.
fn find_chain(s: &Session, frame: &WorkFrame, body_name: &str, ats: &[Vec3]) -> std::result::Result<(Vec<(usize, Vec3)>, bool), String> {
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    let mesh = body.mesh();
    let edges = &mesh.edges;
    if edges.len() > 100_000 {
        return Err("the body has too many edges".into());
    }
    let tol = (body.body.size() * 1e-5).max(1e-5);
    let mut picked: Vec<usize> = Vec::new();
    for at in ats {
        let index = nearest_edge(edges, *at).ok_or_else(|| format!("no edge of `{body_name}` at the picked point"))?;
        for i in mesh.circular_rim(index) {
            if !picked.contains(&i) {
                picked.push(i);
            }
        }
    }
    let pts: Vec<Vec3> = picked.iter().filter_map(|i| edges.get(*i)).flatten().copied().collect();
    let normal = plane_normal(&pts, frame.z, tol)?;
    let origin = *pts.first().ok_or("the picked edges have no points")?;
    let ends = |i: usize| edges.get(i).and_then(|e| Some((*e.first()?, *e.last()?, e.get(1).copied()?, e.get(e.len().checked_sub(2)?).copied()?)));
    let in_plane = |i: usize| edges.get(i).is_some_and(|e| e.len() >= 2 && e.iter().all(|p| (*p - origin).dot(normal).abs() <= tol));
    let seed = *picked.first().ok_or("no edge picked")?;
    let (start, end, after_start, before_end) = ends(seed).ok_or("the picked edge is gone")?;
    let mut taken = vec![seed];
    // Walks on from `at` (heading `dir`) until `goal`; the edges in walking order.
    let grow = |taken: &mut Vec<usize>, mut at: Vec3, mut dir: Vec3, goal: Vec3| -> (Vec<usize>, bool) {
        let mut out = Vec::new();
        for _ in 0..edges.len() {
            if at.dist(goal) <= tol {
                return (out, true);
            }
            let next = (0..edges.len())
                .filter(|i| !taken.contains(i) && in_plane(*i))
                .filter_map(|i| {
                    let (a, b, a1, b1) = ends(i)?;
                    let (far, out, back) = if a.dist(at) <= tol {
                        (b, a1 - a, b - b1)
                    } else if b.dist(at) <= tol {
                        (a, b1 - b, a - a1)
                    } else {
                        return None;
                    };
                    let straight = out.normalized().zip(dir.normalized()).map_or(-1.0, |(o, d)| o.dot(d));
                    let score = if picked.contains(&i) { 10.0 } else { 0.0 } + straight;
                    Some((i, far, back, score))
                })
                .max_by(|a, b| a.3.total_cmp(&b.3));
            let Some((i, far, back, _)) = next else { return (out, false) };
            taken.push(i);
            out.push(i);
            (at, dir) = (far, back);
        }
        (out, false)
    };
    // Forward from the seed's end, then (if that does not come round) back from its start.
    let (forward, mut closed) = if start.dist(end) <= tol { (Vec::new(), true) } else { grow(&mut taken, end, end - before_end, start) };
    let mut chain = vec![seed];
    chain.extend(forward);
    if !closed {
        let (backward, _) = grow(&mut taken, start, start - after_start, end);
        chain = backward.into_iter().rev().chain(chain).collect();
        closed = false;
    }
    let mid = |i: usize| {
        let e = edges.get(i)?;
        let length: f64 = e.windows(2).map(|w| if let [a, b] = w { a.dist(*b) } else { 0.0 }).sum();
        let mut left = length / 2.0;
        for w in e.windows(2) {
            if let [a, b] = w {
                let l = a.dist(*b);
                if l >= left && l > 0.0 {
                    return Some(*a + (*b - *a) * (left / l));
                }
                left -= l;
            }
        }
        e.first().copied()
    };
    Ok((chain.into_iter().filter_map(|i| Some((i, mid(i)?))).collect(), closed))
}

/// `cam.edges.chain`: a suggested chain through picked edges, for the form to show as picked.
fn chain_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.edges.chain";
    let setup = str_(p, "setup").and_then(|n| s.doc.cam.setup(n.trim())).ok_or_else(|| bad(cmd, "`setup` must name a setup (cam.setup first)"))?;
    let frame = work_frame(setup).map_err(|e| bad(cmd, e))?;
    let picks = p
        .get("edges")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty() && a.len() <= MAX_REGIONS_PICKED)
        .ok_or_else(|| bad(cmd, "`edges` needs 1 to 256 picked edges"))?;
    let mut body = String::new();
    let mut ats = Vec::new();
    for v in picks {
        let (b, at) = pick_param(cmd, v, "edges")?;
        if !body.is_empty() && b != body {
            return Err(bad(cmd, "the picked edges must be on one body"));
        }
        body = b;
        ats.push(at);
    }
    let (chain, closed) = find_chain(s, &frame, &body, &ats).map_err(|e| bad(cmd, e))?;
    let edges: Vec<Value> = chain.iter().map(|(i, at)| json!({"body": body, "at": [at.x, at.y, at.z], "index": i})).collect();
    Ok(json!({"edges": edges, "closed": closed}))
}

/// Most edges a chain suggestion starts from.
const MAX_REGIONS_PICKED: usize = 256;

/// The distance from `p` to the segment `a`-`b`.
fn seg3_dist(a: Vec3, b: Vec3, p: Vec3) -> f64 {
    let d = b - a;
    let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    (a + d * t).dist(p)
}

/// A pick on the model: `{body, at}` with `at` a model point.
fn pick_param(cmd: &str, v: &Value, key: &str) -> Result<(String, Vec3)> {
    let body = v.get("body").and_then(Value::as_str).filter(|b| !b.is_empty() && b.len() <= 128);
    let at = v.get("at").and_then(vec3).filter(|a| a.x.abs().max(a.y.abs()).max(a.z.abs()) <= 1.0e6);
    match (body, at) {
        (Some(body), Some(at)) => Ok((body.to_string(), at)),
        _ => Err(bad(cmd, format!("`{key}` must be {{body, at: [x, y, z]}}"))),
    }
}

/// The outward normal of the flat face of `body_name` that `at` lies on.
fn face_normal(s: &Session, body_name: &str, at: Vec3) -> std::result::Result<Vec3, String> {
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    let mut best: Option<(f64, Vec3)> = None;
    for index in 0..body.body.face_count().min(20_000) {
        let Some(plane) = body.body.planar_face(index) else { continue };
        let d = plane.to_world(plane.to_local(at)).dist(at);
        if d <= ON_CONTOUR && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, plane.normal()));
        }
    }
    best.and_then(|(_, n)| n.normalized()).ok_or_else(|| "the picked point is on no flat face".to_string())
}

/// The direction of the straight edge of `body_name` nearest `at`, square to `z`: the setup's X.
fn edge_direction(s: &Session, body_name: &str, at: Vec3, z: Vec3) -> std::result::Result<Vec3, String> {
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    let tol = (body.body.size() * 1e-3).max(1e-3);
    let edges = body.body.edges(tol).map_err(|e| e.to_string())?;
    let (index, d) = body.body.nearest_edge(at, tol).map_err(|e| e.to_string())?.ok_or("the body has no edges")?;
    let edge = edges.iter().find(|e| e.index == index).filter(|_| d <= ON_CONTOUR).ok_or("the picked point is on no edge")?;
    let (Some(a), Some(b)) = (edge.points.first(), edge.points.last()) else { return Err("the picked edge has no length".into()) };
    let along = *b - *a;
    let straight = (along.len() - edge.length).abs() <= 1e-6 * edge.length.max(1.0);
    if !straight {
        return Err("the X direction needs a straight edge".into());
    }
    let z = z.normalized().ok_or("the setup's tool axis is not a direction")?;
    (along - z * along.dot(z)).normalized().filter(|x| x.is_finite()).ok_or_else(|| "the picked edge runs along the tool axis".to_string())
}

/// Part zero on the edge of `body_name` nearest `at`: a round edge's centre (a bore's axis), else
/// the edge's middle.
fn edge_centre(s: &Session, body_name: &str, at: Vec3) -> std::result::Result<Vec3, String> {
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    let tol = (body.body.size() * 1e-3).max(1e-3);
    let edges = body.body.edges(tol).map_err(|e| e.to_string())?;
    let (index, d) = body.body.nearest_edge(at, tol).map_err(|e| e.to_string())?.ok_or("the body has no edges")?;
    let edge = edges.iter().find(|e| e.index == index).filter(|_| d <= ON_CONTOUR).ok_or("the picked point is on no edge")?;
    let n = edge.points.len();
    let (Some(a), Some(b), Some(c)) = (edge.points.first(), edge.points.get(n / 3), edge.points.get(2 * n / 3)) else { return Ok(edge.mid) };
    // The circle through three of its points; every point on it within the meshing tolerance.
    let (ab, ac) = (*b - *a, *c - *a);
    let normal = ab.cross(ac);
    let n2 = normal.dot(normal);
    if !(n2 > 1e-18) {
        return Ok(edge.mid);
    }
    let centre = *a + (normal.cross(ab) * ac.dot(ac) + ac.cross(normal) * ab.dot(ab)) * (0.5 / n2);
    let r = centre.dist(*a);
    let round = r.is_finite() && edge.points.iter().all(|q| (q.dist(centre) - r).abs() <= 2.0 * tol + 1e-6 * r);
    Ok(if round { centre } else { edge.mid })
}

/// The centres of the round holes in the flat face of `body_name` that `at` lies on.
fn face_hole_centres(s: &Session, body_name: &str, at: Vec3) -> std::result::Result<Vec<Vec3>, String> {
    let state = s.model.state();
    let body = state.body(body_name).ok_or_else(|| format!("there is no body `{body_name}`"))?;
    let mut best: Option<(f64, usize, Plane)> = None;
    for index in 0..body.body.face_count().min(20_000) {
        let Some(plane) = body.body.planar_face(index) else { continue };
        let d = plane.to_world(plane.to_local(at)).dist(at);
        if d <= ON_CONTOUR && best.as_ref().is_none_or(|(b, _, _)| d < *b) {
            best = Some((d, index, plane));
        }
    }
    let (_, index, plane) = best.ok_or("the picked point is on no flat face")?;
    let resolved = resolve_face_indices(&state, &plane, LinkKind::Project, body_name, &[index]).map_err(|e| e.to_string())?;
    let region = outline(&resolved.geom)?;
    let centres: Vec<Vec3> = region
        .holes
        .iter()
        .filter_map(|lp| {
            let mut circle: Option<(Vec2, f64)> = None;
            for seg in &lp.segs {
                let Seg2::Arc { center, radius, .. } = seg else { return None };
                match circle {
                    None => circle = Some((*center, *radius)),
                    Some((c, _)) if c.dist(*center) <= 1e-6 => {}
                    Some(_) => return None,
                }
            }
            circle
        })
        // A boss standing on the face has a round rim too: just inside its rim and above the
        // face is its wall, where a hole has none (a bore in the boss leaves the wall there).
        .filter(|(c, r)| {
            let inset = (r * 0.1).min(0.2);
            let probe = plane.to_world(*c + Vec2::new(r - inset, 0.0)) + plane.normal() * 0.1;
            !body.body.contains(probe)
        })
        .map(|(c, _)| plane.to_world(c))
        .collect();
    if centres.is_empty() { Err("the picked face has no round holes".into()) } else { Ok(centres) }
}

/// The smallest and largest work coordinates of `bodies` (every body when empty) in `frame`:
/// the corners of each body's box.
fn model_extent(s: &Session, bodies: &[String], frame: &WorkFrame) -> std::result::Result<(Vec3, Vec3), String> {
    let state = s.model.state();
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for body in state.bodies.iter().filter(|b| bodies.is_empty() || bodies.contains(&b.name)) {
        let b = body.body.outline_bounds();
        if !(b.min.x <= b.max.x && b.min.y <= b.max.y && b.min.z <= b.max.z) {
            continue;
        }
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            );
            let w = frame.local(corner);
            (lo, hi) = (Vec3::new(lo.x.min(w.x), lo.y.min(w.y), lo.z.min(w.z)), Vec3::new(hi.x.max(w.x), hi.y.max(w.y), hi.z.max(w.z)));
        }
    }
    if lo.x <= hi.x && lo.is_finite() && hi.is_finite() { Ok((lo, hi)) } else { Err("there is no body to machine".into()) }
}

/// The stock's box in `frame`: around the model, or the stock's own size centred on the model
/// with its top at the model's top. Round stock is boxed square to the tool axis.
fn stock_extent(s: &Session, setup: &CamSetup, frame: &WorkFrame) -> std::result::Result<(Vec3, Vec3), String> {
    let (lo, hi) = model_extent(s, &setup.model_bodies, frame)?;
    let mid = (lo + hi) * 0.5;
    let up = |size: f64, step: f64| if step > 0.0 { (size / step).ceil() * step } else { size };
    let fixed = |w: f64, d: f64, h: f64| (Vec3::new(mid.x - w / 2.0, mid.y - d / 2.0, hi.z - h), Vec3::new(mid.x + w / 2.0, mid.y + d / 2.0, hi.z));
    Ok(match &setup.stock_spec {
        CamStock::RelativeBox { mode, side_offset, top_offset, bottom_offset, round_up, .. } => {
            let (side, top, bottom) = match mode {
                StockOffsetMode::NoAdditional => (0.0, 0.0, 0.0),
                StockOffsetMode::SidesAndTopBottom => (*side_offset, *top_offset, *bottom_offset),
                StockOffsetMode::AllSides => (*side_offset, *side_offset, *side_offset),
            };
            let (w, d) = (up(hi.x - lo.x + 2.0 * side, *round_up), up(hi.y - lo.y + 2.0 * side, *round_up));
            let h = up(hi.z - lo.z + top + bottom, *round_up);
            (Vec3::new(mid.x - w / 2.0, mid.y - d / 2.0, hi.z + top - h), Vec3::new(mid.x + w / 2.0, mid.y + d / 2.0, hi.z + top))
        }
        CamStock::Box { width, depth, height } => fixed(*width, *depth, *height),
        CamStock::Cylinder { diameter, height } => fixed(*diameter, *diameter, *height),
        CamStock::Tube { outer_diameter, height, .. } => fixed(*outer_diameter, *outer_diameter, *height),
        CamStock::RelativeCylinder { radial_offset, front_offset, back_offset, round_up } => {
            let r = 0.5 * (hi.x - lo.x).hypot(hi.y - lo.y) + radial_offset;
            let dia = up(2.0 * r, *round_up);
            (
                Vec3::new(mid.x - dia / 2.0, mid.y - dia / 2.0, lo.z - back_offset),
                Vec3::new(mid.x + dia / 2.0, mid.y + dia / 2.0, hi.z + front_offset),
            )
        }
        CamStock::FromSolid { body } => model_extent(s, std::slice::from_ref(body), frame)?,
        CamStock::FromPreceding { .. } => (lo, hi),
    })
}

/// Part zero at the setup's box point of the model's or the stock's box.
fn box_origin(s: &Session, setup: &CamSetup) -> std::result::Result<Vec3, String> {
    let frame = WorkFrame { o: Vec3::ZERO, ..work_frame(setup)? };
    let (lo, hi) = match setup.wcs_origin_mode {
        WcsOriginMode::StockBoxPoint => stock_extent(s, setup, &frame)?,
        _ => model_extent(s, &setup.model_bodies, &frame)?,
    };
    let at = |r: BoxAxisRef, a: f64, b: f64| match r {
        BoxAxisRef::Negative => a,
        BoxAxisRef::Center => (a + b) * 0.5,
        BoxAxisRef::Positive => b,
    };
    let p = &setup.box_point;
    Ok(frame.model(Vec3::new(at(p.x, lo.x, hi.x), at(p.y, lo.y, hi.y), at(p.z, lo.z, hi.z))))
}

/// What the viewport draws for a setup: part zero, its axes (unit vectors) and the stock's eight
/// corners, all in model coordinates.
pub struct SetupOutline {
    pub origin: Vec3,
    pub axes: [Vec3; 3],
    pub stock: Option<[Vec3; 8]>,
}

impl Session {
    /// The frame and stock of setup `name`; None when its axes do not form a frame.
    pub fn cam_setup_outline(&self, name: &str) -> Option<SetupOutline> {
        let setup = self.doc.cam.setup(name)?;
        let frame = work_frame(setup).ok()?;
        let stock = stock_extent(self, setup, &frame).ok().map(|(lo, hi)| {
            std::array::from_fn(|i| {
                frame.model(Vec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                ))
            })
        });
        Some(SetupOutline { origin: frame.o, axes: [frame.x, frame.y, frame.z], stock })
    }

    /// Every operation's toolpath as model points, each marked cutting or not, for the viewport.
    /// An operation whose path cannot be made is left out with its reason.
    pub fn cam_toolpaths(&self) -> Vec<CamToolpath> {
        let mut out = Vec::new();
        for setup in &self.doc.cam.setups {
            let Ok(frame) = work_frame(setup) else { continue };
            for op in &setup.operations {
                let points = build_path(self, setup, op)
                    .and_then(|path| path.checked_points(0.05, 200_000).map_err(|e| e.to_string()))
                    .map(|points| points.into_iter().map(|(p, cutting)| (frame.model(p), cutting)).collect());
                out.push(CamToolpath {
                    setup: setup.name.clone(),
                    operation: op.name().to_string(),
                    skipped: setup.skipped.iter().any(|n| n == op.name()),
                    points,
                });
            }
        }
        out
    }
}

impl Session {
    /// Setup `setup`'s program as it is exported (the setup's post, else its machine's, else
    /// LinuxCNC), each line's move in model coordinates, for the G-code preview.
    pub fn cam_program(&self, setup: &str) -> std::result::Result<CamProgram, String> {
        let found = self.doc.cam.setup(setup).ok_or_else(|| format!("no setup `{setup}`"))?;
        let post = chosen_post(&self.doc.cam, found).unwrap_or_else(|| "linuxcnc".into());
        let frame = work_frame(found)?;
        let out = post_program(self, &json!({"setup": setup, "post": post})).map_err(|e| e.to_string())?;
        let gcode = out.get("gcode").and_then(Value::as_str).ok_or("post returned no G-code")?.to_string();
        let extension = out.get("extension").and_then(Value::as_str).unwrap_or("ngc").to_string();
        let mut moves = Vec::new();
        let mut at: Option<Vec3> = None;
        for (line, m) in solidcraft_cam::parse_lines(&gcode).map_err(|e| e.to_string())? {
            let Some(to) = m.target() else { continue };
            if let Some(from) = at {
                let points = match m {
                    Move::Arc { center, ccw, .. } => arc_points(from, to, center, ccw),
                    _ => vec![from, to],
                };
                moves.push(CamProgramMove { line, points: points.into_iter().map(|p| frame.model(p)).collect(), cutting: m.feed().is_some() });
            }
            at = Some(to);
        }
        Ok(CamProgram { gcode, extension, moves })
    }
}

/// Points along an arc move (a helix when Z changes), about every 5 degrees.
fn arc_points(from: Vec3, to: Vec3, center: Vec2, ccw: bool) -> Vec<Vec3> {
    let sweep = solidcraft_cam::arc_sweep(from, to, center, ccw);
    let radius = (Vec2::new(from.x, from.y) - center).len();
    let start = (from.y - center.y).atan2(from.x - center.x);
    let steps = ((sweep.abs() / 5f64.to_radians()).ceil() as usize).clamp(1, 720);
    let mut points: Vec<Vec3> = (0..steps)
        .map(|k| {
            let f = k as f64 / steps as f64;
            let a = start + sweep * f;
            Vec3::new(center.x + radius * a.cos(), center.y + radius * a.sin(), from.z + (to.z - from.z) * f)
        })
        .collect();
    points.push(to);
    points
}

/// A setup's program for the G-code preview (see [`Session::cam_program`]).
pub struct CamProgram {
    pub gcode: String,
    /// The post's file extension, without the dot.
    pub extension: String,
    /// Every motion line's move, in program order.
    pub moves: Vec<CamProgramMove>,
}

/// One line's move: the line number (from 1), the tool's way in model coordinates, and whether
/// it cuts (a rapid does not).
pub struct CamProgramMove {
    pub line: usize,
    pub points: Vec<Vec3>,
    pub cutting: bool,
}

/// One operation's toolpath in model coordinates (see [`Session::cam_toolpaths`]).
pub struct CamToolpath {
    pub setup: String,
    pub operation: String,
    pub skipped: bool,
    pub points: std::result::Result<Vec<(Vec3, bool)>, String>,
}

/// Whether material stands on the face just inside `lp`, one of its inner loops: the foot of a
/// boss or wall rising from the face rather than an opening through it.
fn filled_above(body: &solidcraft_kernel::Body, plane: &Plane, lp: &Loop2) -> bool {
    let Some(seg) = lp.segs.first() else { return false };
    let Some(along) = (seg.point_at(0.51) - seg.point_at(0.49)).normalized() else { return false };
    let inset = (lp.signed_area().abs().sqrt() * 0.05).clamp(1e-3, 0.2);
    let mid = seg.point_at(0.5);
    [Vec2::new(-along.y, along.x), Vec2::new(along.y, -along.x)]
        .into_iter()
        .map(|side| mid + side * inset)
        .find(|p| lp.contains(*p))
        .is_some_and(|p| body.contains(plane.to_world(p) + plane.normal() * 0.1))
}

/// The regions to machine in work XY, and the openings: holes of a face picked as single
/// contours, which an automatic side machines from inside. For a contour (`contour`), an inner
/// loop with material standing on it (a boss's foot) is an outline, not an opening, so the
/// automatic side goes round the boss; a pocket keeps it as an island either way.
fn resolve_geometry(s: &Session, setup: &CamSetup, source: &CamGeometry, contour: bool) -> std::result::Result<(Vec<Region2>, Vec<Region2>), String> {
    let frame = work_frame(setup)?;
    let state = s.model.state();
    match source {
        CamGeometry::Points { .. } => Err("milling requires a region, not hole centres".into()),
        CamGeometry::Stock => Err("only facing takes the stock outline".into()),
        CamGeometry::Polygons { regions } => {
            let regions = regions.iter().map(|r| r.to_region().map_err(|e| e.to_string())).collect::<std::result::Result<_, _>>()?;
            Ok((regions, Vec::new()))
        }
        CamGeometry::Sketch { sketch, profiles } => {
            let sketch = state.sketch(*sketch).ok_or("its sketch is not built (deleted, rolled back, suppressed or failed)")?;
            let regions = select_profiles(sketch, profiles).map_err(|e| e.to_string())?;
            Ok((to_setup(&sketch.plane, &frame, &regions)?, Vec::new()))
        }
        CamGeometry::Edges { .. } => {
            let (loops, chains) = edge_chain(s, &frame, &edge_refs(source))?;
            if !chains.is_empty() && !contour {
                return Err(format!(
                    "the picked edges form no closed contour{}; only a contour can follow an open one",
                    open_end_note(&frame, &chains)
                ));
            }
            Ok((loops, Vec::new()))
        }
        CamGeometry::Faces { faces } => {
            let (mut regions, mut openings) = (Vec::new(), Vec::new());
            // A contour picked twice (both halves of a round rim, two edges of one outline) is cut once.
            let mut seen: Vec<(&str, usize, Option<usize>)> = Vec::new();
            for face in faces {
                let body = state.body(&face.body).ok_or_else(|| format!("body `{}` is gone", face.body))?;
                let index = match (face.name.is_empty(), face.contour_at) {
                    // A face the model gives no name: the one the picked edge or point is on.
                    (true, Some(c)) => edge_face_index(s, &frame, &face.body, Vec3::new(c[0], c[1], c[2]))?,
                    (true, None) => face_index_at(s, &face.body, face.at_vec())?,
                    (false, _) => {
                        solidcraft_doc::naming::face_on_body(body, &face.name, face.at_vec())
                            .ok_or_else(|| format!("its face on `{}` is gone", face.body))?
                            .0
                    }
                };
                let plane = body.body.planar_face(index).ok_or("a picked face is not planar")?;
                let resolved = resolve_face_indices(&state, &plane, LinkKind::Project, &face.body, &[index]).map_err(|e| e.to_string())?;
                let region = outline(&resolved.geom)?;
                let Some(at) = face.contour_at else {
                    if !seen.contains(&(face.body.as_str(), index, None)) {
                        seen.push((face.body.as_str(), index, None));
                        let mut region = region;
                        if contour {
                            // A boss's foot is gone round on its own, not cut into.
                            let (feet, holes): (Vec<Loop2>, Vec<Loop2>) =
                                region.holes.into_iter().partition(|lp| filled_above(&body.body, &plane, lp));
                            region.holes = holes;
                            let feet: Vec<Region2> = feet.into_iter().map(|lp| Region2 { outer: lp.ccw(), holes: Vec::new() }).collect();
                            regions.extend(to_setup(&plane, &frame, &feet)?);
                        }
                        regions.extend(to_setup(&plane, &frame, &[region])?);
                    }
                    continue;
                };
                let local = plane.to_local(Vec3::new(at[0], at[1], at[2]));
                let hole = region.holes.iter().map(|lp| loop_dist(lp, local)).enumerate().min_by(|a, b| a.1.total_cmp(&b.1));
                let hole = hole.filter(|(_, d)| *d < loop_dist(&region.outer, local)).map(|(k, _)| k);
                let key = (face.body.as_str(), index, Some(hole.map_or(0, |k| k + 1)));
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                match hole {
                    Some(k) => {
                        let lp = region.holes.get(k).cloned().ok_or("the face lost a hole")?;
                        if contour && filled_above(&body.body, &plane, &lp) {
                            regions.extend(to_setup(&plane, &frame, &[Region2 { outer: lp.ccw(), holes: Vec::new() }])?);
                        } else {
                            openings.extend(to_setup(&plane, &frame, &[Region2 { outer: lp, holes: Vec::new() }])?);
                        }
                    }
                    None => regions.extend(to_setup(&plane, &frame, &[Region2 { outer: region.outer, holes: Vec::new() }])?),
                }
            }
            Ok((regions, openings))
        }
    }
}

fn eval_len(s: &Session, e: &Expr, key: &str) -> std::result::Result<f64, String> {
    s.doc.eval(e.as_str(), Kind::Length).map_err(|err| format!("{key}: {err}"))
}

fn eval_ang(s: &Session, e: &Expr, key: &str) -> std::result::Result<f64, String> {
    s.doc.eval(e.as_str(), Kind::Angle).map_err(|err| format!("{key}: {err}"))
}

fn engine_cut(s: &Session, cut: &CamCut) -> std::result::Result<Cut, String> {
    let heights = Heights {
        clearance: eval_len(s, &cut.clearance, "clearance")?,
        retract: eval_len(s, &cut.retract, "retract")?,
        top: eval_len(s, &cut.top, "top")?,
        bottom: eval_len(s, &cut.bottom, "bottom")?,
    };
    let feeds = Feeds { cut: eval_len(s, &cut.feed, "feed")?, plunge: eval_len(s, &cut.plunge_feed, "plunge_feed")? };
    let stepdown = eval_len(s, &cut.stepdown, "stepdown")?;
    heights.validate().map_err(|e| e.to_string())?;
    feeds.validate().map_err(|e| e.to_string())?;
    if !(stepdown > 0.0) || !stepdown.is_finite() {
        return Err("stepdown must be above 0".into());
    }
    Ok(Cut { heights, feeds, stepdown })
}

/// The contour's params for the picked outlines (`opening` false) or the holes picked as their
/// own contours (true). Auto keeps the part whole: around an outline, inside a hole.
fn engine_contour(s: &Session, p: &CamContourParams, opening: bool) -> std::result::Result<ContourParams, String> {
    Ok(ContourParams {
        side: match (p.side, opening) {
            (CamSide::Auto, false) | (CamSide::Outside, _) => Side::Outside,
            (CamSide::Auto, true) | (CamSide::Inside, _) => Side::Inside,
            (CamSide::On, _) => Side::On,
        },
        climb: p.climb,
        stock_to_leave: eval_len(s, &p.stock_to_leave, "stock_to_leave")?,
        tolerance: eval_len(s, &p.tolerance, "tolerance")?,
        ramp_angle: eval_ang(s, &p.ramp_angle, "ramp_angle")?,
    })
}

fn engine_lead(s: &Session, lead: &CamLead, key: &str) -> std::result::Result<Lead, String> {
    Ok(match lead {
        CamLead::Direct => Lead::Direct,
        CamLead::Arc { radius, sweep } => Lead::Arc { radius: eval_len(s, radius, key)?, sweep: eval_ang(s, sweep, key)? },
        CamLead::Tangent { length } => Lead::Tangent { length: eval_len(s, length, key)? },
        CamLead::Perpendicular { length } => Lead::Perpendicular { length: eval_len(s, length, key)? },
    })
}

fn engine_tabs(s: &Session, tabs: Option<&CamTabs>) -> std::result::Result<Tabs, String> {
    let Some(t) = tabs else { return Ok(Tabs::default()) };
    Ok(Tabs {
        count: t.count,
        spacing: eval_len(s, &t.spacing, "tabs.spacing")?,
        width: eval_len(s, &t.width, "tabs.width")?,
        height: eval_len(s, &t.height, "tabs.height")?,
    })
}

fn engine_leads(s: &Session, p: &CamContourParams) -> std::result::Result<Leads, String> {
    Ok(Leads {
        lead_in: engine_lead(s, &p.lead_in, "lead_in")?,
        lead_out: engine_lead(s, &p.lead_out, "lead_out")?,
        overlap: eval_len(s, &p.overlap, "overlap")?,
    })
}

fn engine_pocket(s: &Session, p: &CamPocketParams) -> std::result::Result<PocketParams, String> {
    Ok(PocketParams {
        stepover: eval_len(s, &p.stepover, "stepover")?,
        climb: p.climb,
        stock_to_leave: eval_len(s, &p.stock_to_leave, "stock_to_leave")?,
        tolerance: eval_len(s, &p.tolerance, "tolerance")?,
        ramp_angle: eval_ang(s, &p.ramp_angle, "ramp_angle")?,
    })
}

pub(super) fn build_path(s: &Session, setup: &CamSetup, op: &CamOperation) -> std::result::Result<solidcraft_cam::Toolpath, String> {
    let mut cut = engine_cut(s, op.cut())?;
    // Material left on the floor: the passes stop that far above the bottom.
    let floor = match op {
        CamOperation::Contour { params, .. } => eval_len(s, &params.floor_stock, "floor_stock")?,
        CamOperation::Pocket { params, .. } => eval_len(s, &params.floor_stock, "floor_stock")?,
        CamOperation::Drill { .. } | CamOperation::Face { .. } | CamOperation::Slot { .. } => 0.0,
    };
    if !floor.is_finite() || floor < 0.0 {
        return Err("the floor stock must be 0 or more".into());
    }
    if floor > 0.0 {
        cut.heights.bottom += floor;
        if cut.heights.bottom >= cut.heights.top {
            return Err("the floor stock leaves nothing to cut between top and bottom".into());
        }
    }
    let tool = s.doc.cam.tool(op.tool()).ok_or_else(|| format!("there is no tool {}", op.tool()))?;
    if cut.heights.top - cut.heights.bottom > tool.flute_length {
        return Err("cutting depth exceeds the tool's flute length".into());
    }
    if let CamOperation::Drill { geometry: CamGeometry::Points { points }, dwell, .. } = op {
        let points: Vec<_> = points.iter().map(|p| Vec2::new(p[0], p[1])).collect();
        return drill(&points, tool, &cut, *dwell).map_err(|e| e.to_string());
    }
    if let CamOperation::Face { geometry, params, .. } = op {
        let params = FaceParams { stepover: eval_len(s, &params.stepover, "stepover")?, overrun: eval_len(s, &params.overrun, "overrun")? };
        let (lo, hi) = match geometry {
            CamGeometry::Stock => {
                let (lo, hi) = stock_extent(s, setup, &work_frame(setup)?)?;
                (Vec2::new(lo.x, lo.y), Vec2::new(hi.x, hi.y))
            }
            CamGeometry::Polygons { regions } => {
                let points = regions.iter().flat_map(|r| r.outer.iter());
                let fold = |f: fn(f64, f64) -> f64, start: f64, i: usize| points.clone().fold(start, |a, q| f(a, q[i]));
                (
                    Vec2::new(fold(f64::min, f64::INFINITY, 0), fold(f64::min, f64::INFINITY, 1)),
                    Vec2::new(fold(f64::max, f64::NEG_INFINITY, 0), fold(f64::max, f64::NEG_INFINITY, 1)),
                )
            }
            // A picked face (the top, or the base for the whole footprint) or sketch profile:
            // the box round its outline.
            CamGeometry::Faces { .. } | CamGeometry::Edges { .. } | CamGeometry::Sketch { .. } => {
                let (regions, openings) = resolve_geometry(s, setup, geometry, false)?;
                let mut area = solidcraft_geom::Aabb2::EMPTY;
                for seg in regions.iter().chain(&openings).flat_map(|r| r.outer.segs.iter()) {
                    for i in 0..=16 {
                        area.add(seg.point_at(f64::from(i) / 16.0));
                    }
                }
                if area.is_empty() {
                    return Err("the picked face has no outline to face".into());
                }
                (area.min, area.max)
            }
            CamGeometry::Points { .. } => return Err("facing takes the stock outline, a face or a rectangle".into()),
        };
        return face(lo, hi, tool, &params, &cut).map_err(|e| e.to_string());
    }
    if let CamOperation::Slot { geometry, params, .. } = op {
        let params = SlotParams {
            width: params.width.as_ref().map(|w| eval_len(s, w, "width")).transpose()?,
            stepover: eval_len(s, &params.stepover, "stepover")?,
            stock_to_leave: eval_len(s, &params.stock_to_leave, "stock_to_leave")?,
            tolerance: eval_len(s, &params.tolerance, "tolerance")?,
            ramp_angle: eval_ang(s, &params.ramp_angle, "ramp_angle")?,
        };
        let path = slot(&slot_paths(s, setup, geometry)?, tool, &params, &cut).map_err(|e| e.to_string())?;
        solidcraft_cam::verify(&path, &cut.heights).map_err(|e| e.to_string())?;
        return Ok(path);
    }
    let (regions, openings) = resolve_geometry(s, setup, op.geometry(), matches!(op, CamOperation::Contour { .. }))?;
    match op {
        CamOperation::Contour { params: p, .. } => {
            let (params, other) = (engine_contour(s, p, false)?, engine_contour(s, p, true)?);
            let start = match p.start {
                Some(a) => {
                    let w = work_frame(setup)?.local(Vec3::new(a[0], a[1], a[2]));
                    Some(Vec2::new(w.x, w.y))
                }
                None => None,
            };
            let leads = engine_leads(s, p)?;
            let tabs = engine_tabs(s, p.tabs.as_deref())?;
            let mut moves = Vec::new();
            for (group, params) in [(&regions, &params), (&openings, &other)] {
                if !group.is_empty() {
                    moves.extend(contour_with_tabs(group, tool, params, &leads, &tabs, &cut, start).map_err(|e| e.to_string())?.moves);
                }
            }
            // An edge chain that does not close is followed as an open contour.
            if let CamGeometry::Edges { .. } = op.geometry() {
                let (_, chains) = edge_chain(s, &work_frame(setup)?, &edge_refs(op.geometry()))?;
                if !chains.is_empty() {
                    if !tabs.is_empty() {
                        return Err("holding tabs are for closed contours: the picked edges leave this one open".into());
                    }
                    let paths: Vec<OpenPath> = chains.into_iter().map(|segs| OpenPath { segs }).collect();
                    let open = OpenContourParams {
                        side: match p.open_side {
                            CamOpenSide::Left => OpenSide::Left,
                            CamOpenSide::Right => OpenSide::Right,
                            CamOpenSide::On => OpenSide::On,
                        },
                        reverse: p.reverse,
                        stock_to_leave: params.stock_to_leave,
                        tolerance: params.tolerance,
                        extend_start: eval_len(s, &p.extend_start, "extend_start")?,
                        extend_end: eval_len(s, &p.extend_end, "extend_end")?,
                    };
                    moves.extend(contour_open(&paths, tool, &open, &leads, &cut).map_err(|e| e.to_string())?.moves);
                }
            }
            if moves.is_empty() {
                return Err("there is nothing to contour".into());
            }
            let path = solidcraft_cam::Toolpath { moves };
            solidcraft_cam::verify(&path, &cut.heights).map_err(|e| e.to_string())?;
            Ok(path)
        }
        CamOperation::Pocket { params, .. } => {
            let params = engine_pocket(s, params)?;
            let all: Vec<Region2> = regions.into_iter().chain(openings).collect();
            pocket(&all, tool, &params, &cut).map_err(|e| e.to_string())
        }
        CamOperation::Drill { .. } => Err("drilling needs hole centres".into()),
        CamOperation::Face { .. } => Err("facing takes the stock outline or a rectangle".into()),
        CamOperation::Slot { .. } => Err("a slot takes edges or contours".into()),
    }
}

/// A slot's paths in work XY: picked edges joined into chains (closed or open), else every
/// contour of the picked faces, sketch profiles or polygons, each a closed path.
fn slot_paths(s: &Session, setup: &CamSetup, geometry: &CamGeometry) -> std::result::Result<Vec<OpenPath>, String> {
    let (regions, open) = match geometry {
        CamGeometry::Edges { .. } => edge_chain(s, &work_frame(setup)?, &edge_refs(geometry))?,
        _ => {
            let (regions, openings) = resolve_geometry(s, setup, geometry, false)?;
            (regions.into_iter().chain(openings).collect(), Vec::new())
        }
    };
    let mut paths: Vec<OpenPath> =
        regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).map(|lp| OpenPath { segs: lp.segs.clone() }).collect();
    paths.extend(open.into_iter().map(|segs| OpenPath { segs }));
    if paths.is_empty() {
        return Err("there is no path for the slot: pick edges or contours".into());
    }
    Ok(paths)
}

fn list_cmd(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({
        "tools": s.doc.cam.tools,
        "machines": s.doc.cam.machines,
        "setups": s.doc.cam.setups.iter().map(setup_json).collect::<Vec<_>>(),
        "posts": s.doc.cam.posts.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        "builtin_posts": Post::BUILTIN,
    }))
}

fn setup_json(setup: &CamSetup) -> Value {
    json!({
        "name": setup.name,
        "work_offset": setup.work_offset,
        "tool_axis": setup.tool_axis,
        "x_axis": setup.x_axis,
        "work_origin": setup.work_origin,
        "stock": setup.stock,
        "stock_spec": setup.stock_spec,
        "wcs": setup.wcs,
        "model_bodies": setup.model_bodies,
        "material": setup.material,
        "max_rpm": setup.max_rpm,
        "machine": setup.machine,
        "skipped": setup.skipped,
        "operations": setup.operations.iter().map(|op| match op {
            CamOperation::Drill { name, tool, geometry, cut, dwell, spindle_rpm, coolant } => json!({
                "type": "drill", "name": name, "tool": tool, "geometry": geometry,
                "cut": cut, "dwell": dwell, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
            CamOperation::Contour { name, tool, geometry, params, cut, spindle_rpm, coolant } => json!({
                "type": "contour", "name": name, "tool": tool, "geometry": geometry,
                "params": params, "cut": cut, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
            CamOperation::Pocket { name, tool, geometry, params, cut, spindle_rpm, coolant } => json!({
                "type": "pocket", "name": name, "tool": tool, "geometry": geometry,
                "params": params, "cut": cut, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
            CamOperation::Face { name, tool, geometry, params, cut, spindle_rpm, coolant } => json!({
                "type": "face", "name": name, "tool": tool, "geometry": geometry,
                "params": params, "cut": cut, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
            CamOperation::Slot { name, tool, geometry, params, cut, spindle_rpm, coolant } => json!({
                "type": "slot", "name": name, "tool": tool, "geometry": geometry,
                "params": params, "cut": cut, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
        }).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guided(kind: &str) -> Value {
        json!({"setup":"Top", "name":"Cut", "kind":kind, "tool":1, "diameter":4,
            "flute_length":20, "x":0, "y":0, "width":20, "height":15,
            "top":0, "bottom":-3, "clearance":10, "retract":2, "stepdown":1,
            "stepover":2, "feed":600, "plunge_feed":150, "spindle_rpm":3000,
            "work_offset":2, "points":[[5,5],[15,5]], "dwell":0.1})
    }

    #[test]
    fn guided_linuxcnc_jobs_preview_save_roundtrip_and_undo() {
        for kind in ["contour", "pocket", "drill"] {
            let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Default::default() };
            let before = serde_json::to_value(&s.doc).unwrap();
            let preview = run(&mut s, "cam.job.preview", guided(kind));
            assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
            assert!(preview["points"].as_array().unwrap().len() > 2);
            assert!(preview["gcode"].as_str().unwrap().contains("G91.1 G61 G97"));
            assert!(preview["gcode"].as_str().unwrap().contains("G55"));
            let saved = run(&mut s, "cam.job.create", guided(kind));
            assert_eq!(preview, saved);
            assert_eq!(s.doc.cam.setups.len(), 1);
            let encoded = serde_json::to_vec(&s.doc).unwrap();
            let decoded: solidcraft_doc::Document = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(s.doc.cam, decoded.cam);
            run(&mut s, "edit.undo", json!({}));
            assert!(s.doc.cam.is_empty());
            run(&mut s, "edit.redo", json!({}));
            assert_eq!(s.doc.cam.setups.len(), 1);
        }
    }

    #[test]
    fn invalid_guided_jobs_leave_the_document_unchanged() {
        let mut s = Session::default();
        let before = serde_json::to_value(&s.doc).unwrap();
        for (key, value) in [
            ("bottom", json!(3)),
            ("diameter", json!(0)),
            ("work_offset", json!(257)),
            ("clearance", json!(-1)),
            ("stepdown", json!(1e-30)),
            ("spindle_rpm", json!(-5)),
        ] {
            let mut job = guided("pocket");
            job[key] = value;
            assert!(s.execute("cam.job.create", &job).is_err(), "{key}");
            assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
        }
    }

    #[test]
    fn linuxcnc_export_refuses_overwrite_and_keeps_backup() {
        let mut s = Session::default();
        run(&mut s, "cam.job.create", guided("drill"));
        let folder = std::env::temp_dir().join(format!(
            "solidcraft-cam-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("holes.ngc");
        let p = json!({"setup":"Top", "path":path});
        run(&mut s, "cam.export", p.clone());
        let original = std::fs::read(&path).unwrap();
        assert!(s.execute("cam.export", &p).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        run(&mut s, "cam.export", json!({"setup":"Top", "path":path, "overwrite":true}));
        assert_eq!(std::fs::read(solidcraft_io::backup_path(&path)).unwrap(), original);
        std::fs::remove_dir_all(folder).unwrap();
    }

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    #[test]
    fn contour_and_pocket_post_to_verified_gcode() {
        let mut s = Session::default();
        run(
            &mut s,
            "cam.tool.define",
            json!({"number": 1, "diameter": 6, "flute_length": 20, "spindle_rpm": 8000, "feed": 600, "plunge_feed": 200, "stepdown": 3}),
        );
        run(&mut s, "cam.setup", json!({"name": "Top", "work_offset": 1, "stock": {"min": [0, 0, -10], "max": [40, 30, 0]}}));
        run(
            &mut s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Outside", "tool": 1,
                "outer": [[0,0],[40,0],[40,30],[0,30]],
                "side": "outside", "bottom": -6, "stepdown": 3,
            }),
        );
        run(
            &mut s,
            "cam.op.pocket",
            json!({
                "setup": "Top", "name": "Pocket", "tool": 1,
                "outer": [[5,5],[35,5],[35,25],[5,25]],
                "stepover": 2.5, "bottom": -3, "stepdown": 3,
            }),
        );
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let gcode = posted["gcode"].as_str().unwrap();
        assert!(gcode.contains("G21"));
        assert!(gcode.contains("G54"));
        assert!(gcode.contains("M30"));
        assert!(posted["readback"]["within"].as_bool().unwrap());
        assert_eq!(posted["operations"], 2);

        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(cold.doc.cam.tools.len(), 1);
        assert_eq!(cold.doc.cam.setups[0].operations.len(), 2);
        assert!(cold.doc.format_pending.iter().any(|n| n == "cam-milling"));
        let again = run(&mut Session::new(cold.doc.as_ref().clone()), "cam.post", json!({"setup": "Top"}));
        assert!(again["readback"]["within"].as_bool().unwrap());

        let forced = run(&mut Session::new(s.doc.as_ref().clone()), "cam.post", json!({"setup": "Top", "verify": false}));
        assert!(forced["readback"]["within"].as_bool().unwrap());
    }

    #[test]
    fn setup_stores_model_bodies_and_wcs() {
        let mut s = Session::default();
        run(
            &mut s,
            "cam.setup",
            json!({
                "name": "Bodies",
                "model_bodies": ["Part"],
                "wcs": {"origin": [1.0, 2.0, 3.0], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]}
            }),
        );
        assert_eq!(s.doc.cam.setups[0].model_bodies, ["Part"]);
        assert_eq!(s.doc.cam.setups[0].work_origin, [1.0, 2.0, 3.0]);
        assert_eq!(s.doc.cam.setups[0].wcs.origin, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn setup_keeps_room_for_tool_axis_and_work_origin() {
        let mut s = Session::default();
        run(&mut s, "cam.setup", json!({"name": "Side", "tool_axis": [1, 0, 0], "x_axis": [0, 1, 0], "work_origin": [10, 20, 30]}));
        assert_eq!(s.doc.cam.setups[0].tool_axis, [1.0, 0.0, 0.0]);
        assert_eq!(s.doc.cam.setups[0].x_axis, Some([0.0, 1.0, 0.0]));
        assert_eq!(s.doc.cam.setups[0].work_origin, [10.0, 20.0, 30.0]);
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(cold.doc.cam.setups[0].tool_axis, [1.0, 0.0, 0.0]);
        assert_eq!(cold.doc.cam.setups[0].x_axis, Some([0.0, 1.0, 0.0]));
        assert_eq!(cold.doc.cam.setups[0].work_origin, [10.0, 20.0, 30.0]);
        assert!(s.execute("cam.setup", &json!({"name": "Bad", "tool_axis": [0, 0, 0]})).is_err());
        assert!(s.execute("cam.setup", &json!({"name": "Skew", "x_axis": [0, 0, 1]})).is_err());
    }

    fn gcode_x_words(gcode: &str) -> Vec<i64> {
        gcode
            .split_whitespace()
            .filter_map(|w| w.strip_prefix('X'))
            .filter_map(|n| n.parse::<f64>().ok())
            .map(|x| (x * 100.0).round() as i64)
            .collect()
    }

    fn sketch_outside_gcode(s: &mut Session, setup: Value) -> String {
        run(s, "sketch.create", json!({"plane": "XY"}));
        run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        let sketch = s.active_sketch.expect("sketch");
        run(s, "sketch.finish", json!({}));
        run(s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(s, "cam.setup", setup);
        run(
            s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Outside", "tool": 1,
                "sketch": sketch, "side": "outside", "bottom": -3, "stepdown": 3,
            }),
        );
        run(s, "cam.post", json!({"setup": "Top"}))["gcode"].as_str().unwrap().to_string()
    }

    #[test]
    fn work_origin_shifts_sketch_xy_and_offset_writes_g55() {
        let g0 = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top"}));
        let g10 = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top", "work_origin": [10, 0, 0]}));
        let g55 = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top", "work_offset": 2}));
        assert!(g0.contains("G54"), "{g0}");
        assert!(g55.contains("G55"), "{g55}");
        assert!(!g55.contains("G54"), "{g55}");
        let x0 = gcode_x_words(&g0);
        let x10 = gcode_x_words(&g10);
        assert!(!x0.is_empty(), "{g0}");
        assert_ne!(x0, x10, "origin must move posted X\n{g0}\n{g10}");
        let shifted: Vec<i64> = x0.iter().map(|x| x - 1000).collect();
        assert_eq!(shifted, x10, "X(origin 10) should be X(origin 0) minus 10 mm\n{g0}\n{g10}");
    }

    #[test]
    fn x_axis_rotates_work_xy() {
        let g = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top"}));
        let r = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top", "x_axis": [0, 1, 0]}));
        assert_ne!(gcode_x_words(&g), gcode_x_words(&r), "a work X along model Y must change posted X\n{g}\n{r}");
        assert!(r.contains("G54"), "{r}");
    }

    #[test]
    fn refuses_unknown_tool_and_empty_post() {
        let mut s = Session::default();
        assert!(s.execute("cam.op.contour", &json!({"setup": "A", "name": "C", "tool": 1, "outer": [[0,0],[10,0],[10,10]], "bottom": -1})).is_err());
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20}));
        run(&mut s, "cam.setup", json!({"name": "A"}));
        assert!(s.execute("cam.post", &json!({"setup": "A"})).is_err());
        assert!(s.execute("cam.op.remove", &json!({"setup": "A", "name": "missing"})).is_err());
    }

    #[test]
    fn setup_without_name_uses_setup_n() {
        let mut s = Session::default();
        assert_eq!(run(&mut s, "cam.setup", json!({}))["setup"]["name"], "Setup 1");
        assert_eq!(run(&mut s, "cam.setup", json!({}))["setup"]["name"], "Setup 2");
        assert_eq!(s.doc.cam.setups.len(), 2);
        assert!(s.execute("cam.setup", &json!({"delete": true})).is_err());
    }

    #[test]
    fn a_slot_runs_the_tool_centre_along_picked_edges_or_around_a_contour() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200}));
        run(&mut s, "cam.setup", json!({"name": "Top", "work_origin": [0, 0, 8]}));
        let edge = |at: [f64; 3]| json!({"body": "Plate", "at": at});
        // The front and right top edges, an open L: the tool centre on them, no offset.
        let l_path = json!([edge([20.0, 0.0, 8.0]), edge([40.0, 15.0, 8.0])]);
        run(&mut s, "cam.op.slot", json!({"setup": "Top", "name": "S", "tool": 1, "edges": l_path, "bottom": -2, "stepdown": 1}));
        let op = s.doc.cam.setup("Top").and_then(|x| x.operations.first()).cloned();
        assert!(matches!(op, Some(CamOperation::Slot { geometry: CamGeometry::Edges { .. }, .. })), "{op:?}");
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        assert!(posted["readback"]["within"].as_bool().unwrap(), "{posted}");
        let xs = gcode_x_words(posted["gcode"].as_str().unwrap());
        assert!(xs.contains(&0) && xs.contains(&4000) && xs.iter().all(|x| (0..=4000).contains(x)), "{xs:?}");
        // 10 mm wide round a rectangle: side passes reach 2 mm either side of the path.
        run(
            &mut s,
            "cam.op.slot",
            json!({"setup": "Top", "name": "S", "tool": 1, "outer": [[0,0],[40,0],[40,30],[0,30]], "width": 10, "bottom": -1}),
        );
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let xs = gcode_x_words(posted["gcode"].as_str().unwrap());
        assert!(xs.contains(&-200) && xs.contains(&4200) && xs.iter().all(|x| (-200..=4200).contains(x)), "{xs:?}");
        // The guided form's job takes the slot's width and lists the step as a slot.
        let listed = run(&mut s, "cam.list", json!({}));
        assert_eq!(listed["setups"][0]["operations"][0]["type"], "slot");
        // Narrower than the tool, or no path at all, is refused and keeps the step as it was.
        let before = s.doc.cam.clone();
        for bad in [
            json!({"setup": "Top", "name": "S", "tool": 1, "outer": [[0,0],[40,0],[40,30],[0,30]], "width": 4, "bottom": -1}),
            json!({"setup": "Top", "name": "S", "tool": 1, "edges": [], "bottom": -1}),
        ] {
            assert!(s.execute("cam.op.slot", &bad).is_err(), "{bad}");
            assert_eq!(s.doc.cam, before, "{bad}");
        }
    }

    #[test]
    fn a_renamed_setup_keeps_its_steps_settings_and_place() {
        let mut s = Session::default();
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 4, "flute_length": 12}));
        run(&mut s, "cam.setup", json!({"name": "Setup 1", "work_offset": 2}));
        run(&mut s, "cam.setup", json!({"name": "Setup 2"}));
        run(&mut s, "cam.op.contour", json!({"setup": "Setup 1", "name": "C", "tool": 1, "outer": [[0,0],[20,0],[20,10],[0,10]], "bottom": -2}));
        let r = run(&mut s, "cam.setup", json!({"name": "Setup 1", "rename": " Top side "}));
        assert_eq!(r["setup"]["name"], "Top side");
        let names: Vec<&str> = s.doc.cam.setups.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["Top side", "Setup 2"]);
        let top = s.doc.cam.setup("Top side").unwrap();
        assert_eq!((top.work_offset, top.operations.len()), (2, 1));
        // The same name again is no change; a taken, empty or missing one is refused and changes nothing.
        run(&mut s, "cam.setup", json!({"name": "Top side", "rename": "Top side"}));
        let before = s.doc.cam.clone();
        for bad in [
            json!({"name": "Top side", "rename": "Setup 2"}),
            json!({"name": "Top side", "rename": "  "}),
            json!({"name": "Top side", "rename": "x".repeat(129)}),
            json!({"name": "Nothing", "rename": "Other"}),
        ] {
            assert!(s.execute("cam.setup", &bad).is_err(), "{bad}");
            assert_eq!(s.doc.cam, before, "{bad}");
        }
    }

    #[test]
    fn list_and_delete() {
        let mut s = Session::default();
        run(&mut s, "cam.tool.define", json!({"number": 2, "diameter": 3, "flute_length": 12, "kind": "ball_endmill"}));
        run(&mut s, "cam.setup", json!({"name": "S"}));
        run(&mut s, "cam.op.contour", json!({"setup": "S", "name": "C", "tool": 2, "outer": [[0,0],[20,0],[20,10],[0,10]], "bottom": -2}));
        let list = run(&mut s, "cam.list", json!({}));
        assert_eq!(list["tools"].as_array().unwrap().len(), 1);
        assert_eq!(list["setups"][0]["operations"].as_array().unwrap().len(), 1);
        run(&mut s, "cam.op.remove", json!({"setup": "S", "name": "C"}));
        run(&mut s, "cam.setup", json!({"name": "S", "delete": true}));
        run(&mut s, "cam.tool.remove", json!({"number": 2}));
        assert!(s.doc.cam.is_empty());
    }

    #[test]
    fn sketch_profile_follows_the_design_and_exprs_eval() {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "depth", "expression": "6 mm"}));
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        let sketch = s.active_sketch.expect("sketch");
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(&mut s, "cam.setup", json!({"name": "Top"}));
        run(
            &mut s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Outside", "tool": 1,
                "sketch": sketch, "side": "outside", "bottom": "-depth", "stepdown": 3,
            }),
        );
        match &s.doc.cam.setups[0].operations[0] {
            CamOperation::Contour { geometry: CamGeometry::Sketch { .. }, cut, .. } => {
                assert_eq!(cut.bottom.as_str(), "-depth");
            }
            other => panic!("{other:?}"),
        }
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let gcode = posted["gcode"].as_str().unwrap();
        assert!(gcode.contains("Z-6"), "{gcode}");
        assert!(posted["readback"]["within"].as_bool().unwrap());

        run(&mut s, "parameters.change", json!({"name": "depth", "expression": "8 mm"}));
        let again = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let g2 = again["gcode"].as_str().unwrap();
        assert!(g2.contains("Z-8"), "{g2}");
        assert!(!g2.contains("Z-6"), "{g2}");
    }

    /// A 40 × 30 × 8 plate with a 10 mm hole at (20, 15), set up on its top face, and the X words
    /// of a contour whose geometry `pick` names.
    fn plate_contour_x(pick: Value) -> Vec<i64> {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(&mut s, "cam.setup", json!({"name": "Top", "work_origin": [0, 0, 8]}));
        let mut op = json!({"setup": "Top", "name": "Contour", "tool": 1, "bottom": -3, "stepdown": 3});
        for (k, v) in pick.as_object().unwrap() {
            op[k.as_str()] = v.clone();
        }
        run(&mut s, "cam.op.contour", op);
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        assert!(posted["readback"]["within"].as_bool().unwrap());
        gcode_x_words(posted["gcode"].as_str().unwrap())
    }

    #[test]
    fn picked_edges_choose_single_contours_and_start_points() {
        // The outline alone: around the plate, not inside the hole.
        let outer = plate_contour_x(json!({"edges": [{"body": "Plate", "at": [20, 0, 8]}]}));
        assert!(outer.contains(&-300) && outer.contains(&4300), "{outer:?}");
        assert!(!outer.contains(&1800) && !outer.contains(&2200), "{outer:?}");
        // The hole alone: inside it, a radius in from its wall, and nothing around the plate.
        let hole = plate_contour_x(json!({"edges": [{"body": "Plate", "at": [25, 15, 8]}]}));
        assert!(hole.contains(&1800) && hole.contains(&2200), "{hole:?}");
        assert!(!hole.contains(&-300) && !hole.contains(&4300), "{hole:?}");
        // A face the model gives no name is found again by the picked point: the same hole, and
        // the whole top face (outline round the plate, the hole as an island).
        let unnamed = |pick: Value| plate_contour_x(json!({"geometry": {"type": "faces", "faces": [pick]}}));
        assert_eq!(unnamed(json!({"body": "Plate", "name": "", "at": [25, 15, 8], "contour_at": [25, 15, 8]})), hole);
        let top = unnamed(json!({"body": "Plate", "name": "", "at": [5, 5, 8]}));
        assert!(top.contains(&-300) && top.contains(&4300), "{top:?}");
        assert_eq!(plate_contour_x(json!({"faces": [{"body": "Plate", "at": [5, 5, 8]}]})), top);
        // Both, starting mid-edge at the front instead of at a corner.
        let both =
            plate_contour_x(json!({"edges": [{"body": "Plate", "at": [20, 0, 8]}, {"body": "Plate", "at": [25, 15, 8]}], "start": [20, 0, 8]}));
        assert_eq!(both.first(), Some(&2000), "{both:?}");
        assert!(both.contains(&-300) && both.contains(&1800), "{both:?}");
        // Inside and outside are of the picked contour: inside the hole, outside it (8 mm round
        // the centre), inside the plate's outline.
        let in_hole = plate_contour_x(json!({"edges": [{"body": "Plate", "at": [25, 15, 8]}], "side": "inside"}));
        assert_eq!(in_hole, hole);
        let round_hole = plate_contour_x(json!({"edges": [{"body": "Plate", "at": [25, 15, 8]}], "side": "outside"}));
        assert!(round_hole.contains(&1200) && round_hole.contains(&2800) && !round_hole.contains(&1800), "{round_hole:?}");
        let in_plate = plate_contour_x(json!({"edges": [{"body": "Plate", "at": [20, 0, 8]}], "side": "inside"}));
        assert!(in_plate.contains(&300) && in_plate.contains(&3700) && !in_plate.contains(&-300), "{in_plate:?}");
        // Material left on the wall for finishing: half a millimetre further out.
        let left = plate_contour_x(json!({"edges": [{"body": "Plate", "at": [20, 0, 8]}], "stock_to_leave": 0.5}));
        assert!(left.contains(&-350) && left.contains(&4350), "{left:?}");
        // The same contour picked twice is cut once.
        let twice = plate_contour_x(json!({"edges": [{"body": "Plate", "at": [20, 0, 8]}, {"body": "Plate", "at": [40, 15, 8]}]}));
        assert_eq!(twice, outer);
        // An edge that bounds no face square to the tool axis is refused.
        let mut s = Session::default();
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20}));
        run(&mut s, "cam.setup", json!({"name": "Top"}));
        let bad = json!({"setup": "Top", "name": "C", "tool": 1, "bottom": -3, "edges": [{"body": "None", "at": [0, 0, 0]}]});
        assert!(s.execute("cam.op.contour", &bad).is_err());
    }

    #[test]
    fn picked_edges_chain_into_contours_on_request_and_where_no_face_is_flat() {
        let edge = |at: [f64; 3]| json!({"body": "Plate", "at": at});
        // On request: the four top edges are the outline, the hole's two halves its circle.
        let around = [edge([20.0, 0.0, 8.0]), edge([40.0, 15.0, 8.0]), edge([20.0, 30.0, 8.0]), edge([0.0, 15.0, 8.0])];
        let outline = plate_contour_x(json!({"edges": around, "chain": true}));
        assert!(outline.contains(&-300) && outline.contains(&4300) && !outline.contains(&1800), "{outline:?}");
        let r = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
        let quarters =
            [edge([20.0 + r, 15.0 + r, 8.0]), edge([20.0 - r, 15.0 + r, 8.0]), edge([20.0 - r, 15.0 - r, 8.0]), edge([20.0 + r, 15.0 - r, 8.0])];
        let rim = plate_contour_x(json!({"edges": quarters, "chain": true}));
        assert!(rim.contains(&1200) && rim.contains(&2800) && !rim.contains(&-300), "{rim:?}");
        // One piece of the rim picked is the whole rim, as the viewport shows it.
        let piece = plate_contour_x(json!({"edges": [edge([20.0 + r, 15.0 + r, 8.0])], "chain": true}));
        assert_eq!(piece, rim);
        // An open chain is refused.
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(&mut s, "cam.setup", json!({"name": "Top", "work_origin": [0, 0, 8]}));
        let op =
            |edges: Value, chain: bool| json!({"setup": "Top", "name": "C", "tool": 1, "bottom": -3, "stepdown": 3, "edges": edges, "chain": chain});
        // An open chain is an open contour: front edge, then right edge (counter-clockwise round
        // the part), so the part is on the left and the tool rides on the right, outside.
        let l_path = json!([edge([20.0, 0.0, 8.0]), edge([40.0, 15.0, 8.0])]);
        let open_x = |s: &mut Session, extra: Value| {
            let mut p = op(l_path.clone(), true);
            for (k, v) in extra.as_object().unwrap() {
                p[k.as_str()] = v.clone();
            }
            run(s, "cam.op.contour", p);
            let posted = run(s, "cam.post", json!({"setup": "Top"}));
            assert!(posted["readback"]["within"].as_bool().unwrap(), "{posted}");
            gcode_x_words(posted["gcode"].as_str().unwrap())
        };
        let right = open_x(&mut s, json!({"open_side": "right"}));
        assert!(right.contains(&0) && right.contains(&4300) && !right.contains(&3700), "{right:?}");
        let left = open_x(&mut s, json!({"open_side": "left"}));
        assert!(left.contains(&3700) && !left.contains(&4300), "{left:?}");
        // Run on 5 mm before the start, with a 4 mm straight lead-in in line with it.
        let led = open_x(&mut s, json!({"open_side": "right", "extend_start": 5, "lead_in": {"kind": "tangent", "length": 4}}));
        assert!(led.contains(&-900) && led.iter().all(|x| *x >= -900), "{led:?}");
        // Reversed: from the right edge's far end back along the front.
        let back = open_x(&mut s, json!({"open_side": "left", "reverse": true}));
        assert_eq!(back.iter().copied().filter(|x| *x != 0).take(1).collect::<Vec<_>>(), vec![4300], "{back:?}");
        // A pocket needs a closed outline.
        let mut pocket = op(l_path.clone(), true);
        pocket["name"] = json!("P");
        let refused = s.execute("cam.op.pocket", &pocket);
        assert!(refused.is_err_and(|e| e.to_string().contains("no closed contour") && e.to_string().contains("open near")));
        // A closed contour with tangent arcs in and out: the same wall, more arc moves.
        let plain = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let _ = plain;
        let around = json!([edge([20.0, 0.0, 8.0]), edge([40.0, 15.0, 8.0]), edge([20.0, 30.0, 8.0]), edge([0.0, 15.0, 8.0])]);
        let arcs = |g: &str| g.split_whitespace().filter(|w| *w == "G2" || *w == "G3").count();
        run(&mut s, "cam.op.contour", op(around.clone(), true));
        let bare = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let mut with = op(around, true);
        with["lead_in"] = json!({"kind": "arc", "radius": 4});
        with["lead_out"] = json!({"kind": "arc", "radius": 4});
        run(&mut s, "cam.op.contour", with);
        let led = run(&mut s, "cam.post", json!({"setup": "Top"}));
        assert!(led["readback"]["within"].as_bool().unwrap());
        let (g0, g1) = (bare["gcode"].as_str().unwrap(), led["gcode"].as_str().unwrap());
        assert!(arcs(g1) > arcs(g0), "{} vs {}", arcs(g1), arcs(g0));
        // A lead that would run into the part is refused.
        let mut deep = op(json!([edge([20.0, 0.0, 8.0]), edge([40.0, 15.0, 8.0]), edge([20.0, 30.0, 8.0]), edge([0.0, 15.0, 8.0])]), true);
        deep["side"] = json!("inside");
        deep["lead_in"] = json!({"kind": "perpendicular", "length": 40});
        assert!(s.execute("cam.op.contour", &deep).is_err());
        // A corner rounded about the tool axis: its arc closes the chain of the top edges.
        let mut round = Session::default();
        run(&mut round, "sketch.create", json!({"plane": "XY"}));
        run(&mut round, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut round, "sketch.finish", json!({}));
        run(&mut round, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        run(&mut round, "solid.fillet", json!({"body": "Plate", "radius": 5, "edges": [[40, 30, 4]]}));
        run(&mut round, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(&mut round, "cam.setup", json!({"name": "Top", "work_origin": [0, 0, 8]}));
        let r = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
        let top = [edge([20.0, 0.0, 8.0]), edge([40.0, 12.0, 8.0]), edge([35.0 + r, 25.0 + r, 8.0]), edge([17.0, 30.0, 8.0]), edge([0.0, 15.0, 8.0])];
        run(&mut round, "cam.op.contour", op(json!(top), true));
        let posted = run(&mut round, "cam.post", json!({"setup": "Top"}));
        assert!(posted["readback"]["within"].as_bool().unwrap());
        let x = gcode_x_words(posted["gcode"].as_str().unwrap());
        assert!(x.contains(&-300) && x.contains(&4300), "{x:?}");
        // One edge picked, the chain round the top is suggested: four sides and the rounded
        // corner, closed, and it mills as the same contour.
        let found = run(&mut round, "cam.edges.chain", json!({"setup": "Top", "edges": [edge([20.0, 0.0, 8.0])]}));
        assert_eq!((found["edges"].as_array().unwrap().len(), found["closed"].as_bool()), (5, Some(true)), "{found}");
        assert!(found["edges"].as_array().unwrap().iter().all(|e| (e["at"][2].as_f64().unwrap() - 8.0).abs() < 1e-6), "{found}");
        let suggested: Vec<Value> = found["edges"].as_array().unwrap().iter().map(|e| json!({"body": e["body"], "at": e["at"]})).collect();
        run(&mut round, "cam.op.contour", op(json!(suggested), true));
        let again = run(&mut round, "cam.post", json!({"setup": "Top"}));
        assert_eq!(gcode_x_words(again["gcode"].as_str().unwrap()), x);
        // A straight edge along the tool axis gives no plane on its own.
        let upright = round.execute("cam.edges.chain", &json!({"setup": "Top", "edges": [edge([0.0, 0.0, 4.0])]}));
        assert!(upright.is_err_and(|e| e.to_string().contains("pick a second edge")));
        // The front top edge rounded: the fillet's foot bounds no flat face, so the picked edges
        // are taken as they are, all the way round, and stored as a chain.
        run(&mut s, "solid.fillet", json!({"body": "Plate", "radius": 2, "edges": [[20, 0, 8]]}));
        let (d, h) = (2.0 - std::f64::consts::SQRT_2, 6.0 + std::f64::consts::SQRT_2);
        let foot =
            [edge([20.0, 0.0, 6.0]), edge([40.0, d, h]), edge([40.0, 16.0, 8.0]), edge([20.0, 30.0, 8.0]), edge([0.0, 16.0, 8.0]), edge([0.0, d, h])];
        run(&mut s, "cam.op.contour", op(json!(foot), false));
        let setup = s.doc.cam.setup("Top").unwrap();
        assert!(matches!(setup.operations[0].geometry(), CamGeometry::Edges { edges } if edges.len() == 6));
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let x = gcode_x_words(posted["gcode"].as_str().unwrap());
        assert!(x.contains(&-300) && x.contains(&4300), "{x:?}");
    }

    #[test]
    fn a_boss_foot_picked_as_contour_is_gone_round_not_cut_into() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 14, "operation": "join"}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(&mut s, "cam.setup", json!({"name": "Top", "work_origin": [0, 0, 14]}));
        let x_words = |s: &mut Session, extra: Value| {
            let mut op = json!({"setup": "Top", "name": "Zapfen", "tool": 1, "bottom": -6, "stepdown": 3,
                "edges": [{"body": "Plate", "at": [25, 15, 8]}]});
            for (k, v) in extra.as_object().unwrap() {
                op[k.as_str()] = v.clone();
            }
            run(s, "cam.op.contour", op);
            let posted = run(s, "cam.post", json!({"setup": "Top"}));
            gcode_x_words(posted["gcode"].as_str().unwrap())
        };
        // Automatic: round the boss, a radius out from its wall (X 12 and 28), never inside it.
        let auto = x_words(&mut s, json!({}));
        assert!(auto.contains(&1200) && auto.contains(&2800), "{auto:?}");
        assert!(!auto.contains(&1800) && !auto.contains(&2200), "{auto:?}");
        // Outside is the same; inside is what was asked for.
        assert_eq!(x_words(&mut s, json!({"side": "outside"})), auto);
        let inside = x_words(&mut s, json!({"side": "inside"}));
        assert!(inside.contains(&1800) && inside.contains(&2200), "{inside:?}");
    }

    #[test]
    fn drill_at_picked_bores() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
        run(&mut s, "sketch.circle.center", json!({"center": [32, 8], "radius": 2}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        // A round boss on the top face: its rim is no bore.
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.circle.center", json!({"center": [8, 22], "radius": 3}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 12, "operation": "join"}));
        // A bore through the boss, as in the sample part: its centre is empty, its wall is not.
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.circle.center", json!({"center": [8, 22], "radius": 1}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 12, "operation": "cut"}));
        run(&mut s, "cam.tool.define", json!({"number": 2, "kind": "drill", "diameter": 4, "flute_length": 30}));
        run(&mut s, "cam.setup", json!({"name": "Top", "work_origin": [0, 0, 8]}));
        let points = |s: &Session, name: &str| match &s.doc.cam.setup("Top").unwrap().operations.iter().find(|o| o.name() == name).unwrap() {
            CamOperation::Drill { geometry: CamGeometry::Points { points }, .. } => {
                let mut v: Vec<[i64; 2]> = points.iter().map(|p| [(p[0] * 1000.0).round() as i64, (p[1] * 1000.0).round() as i64]).collect();
                v.sort_unstable();
                v
            }
            other => panic!("{other:?}"),
        };
        // A bore's rim gives its centre; a picked face every round hole in it, each once.
        let rim = json!({"setup": "Top", "name": "Rim", "tool": 2, "bottom": -8, "holes": [{"body": "Plate", "at": [25, 15, 8], "edge": true}]});
        run(&mut s, "cam.op.drill", rim);
        assert_eq!(points(&s, "Rim"), [[20000, 15000]]);
        let face = json!({"setup": "Top", "name": "Face", "tool": 2, "bottom": -8,
            "holes": [{"body": "Plate", "at": [5, 5, 8]}, {"body": "Plate", "at": [25, 15, 8], "edge": true}]});
        run(&mut s, "cam.op.drill", face);
        assert_eq!(points(&s, "Face"), [[20000, 15000], [32000, 8000]]);
        // The guided job takes picked edges as bores instead of its typed points.
        let job = json!({"setup": "Top", "name": "Job", "kind": "drill", "tool": 2, "diameter": 4, "flute_length": 30,
            "x": 0, "y": 0, "width": 1, "height": 1, "points": [[1, 1]], "edges": [{"body": "Plate", "at": [34, 8, 8]}],
            "top": 0, "bottom": -8, "clearance": 10, "retract": 2, "stepdown": 2, "stepover": 1, "feed": 100, "plunge_feed": 100,
            "spindle_rpm": 2000, "work_offset": 1, "exact_regions": true});
        // The dialogs add `exact_regions` to sketch picks; the job takes it.
        run(&mut s, "cam.job.create", job);
        assert_eq!(points(&s, "Job"), [[32000, 8000]]);
        // A face without round holes, or no body, is refused.
        let before = s.doc.to_json();
        for holes in [json!([{"body": "Plate", "at": [0, 15, 4]}]), json!([{"body": "Nope", "at": [5, 5, 8]}])] {
            assert!(s.execute("cam.op.drill", &json!({"setup": "Top", "name": "Bad", "tool": 2, "bottom": -8, "holes": holes})).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
    }

    #[test]
    fn setup_frame_from_picked_face_edge_and_box_points() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        let setup = |s: &Session| s.doc.cam.setup("Top").unwrap().clone();
        // Top face: +Z up; the front edge gives X (either way along it); top-left-front corner of the plate.
        let corner = json!({"x": "negative", "y": "negative", "z": "positive"});
        run(
            &mut s,
            "cam.setup",
            json!({"name": "Top", "z_face": {"body": "Plate", "at": [5, 5, 8]},
            "x_edge": {"body": "Plate", "at": [20, 0, 8]}, "wcs_origin_mode": "model_box_point", "box_point": corner}),
        );
        let top = setup(&s);
        assert!(near(top.tool_axis, [0.0, 0.0, 1.0]), "{:?}", top.tool_axis);
        let x = top.x_axis.unwrap();
        assert!(near(x, [1.0, 0.0, 0.0]) || near(x, [-1.0, 0.0, 0.0]), "{x:?}");
        run(&mut s, "cam.setup", json!({"name": "Top", "x_axis": [1, 0, 0]}));
        assert!(near(setup(&s).work_origin, [0.0, 0.0, 8.0]), "{:?}", setup(&s).work_origin);
        assert!(near(setup(&s).wcs.origin, [0.0, 0.0, 8.0]));
        // Stock 2 mm round the sides and 1 mm on top: part zero at the stock's corner.
        let stock =
            json!({"kind": "relative_box", "mode": "sides_and_top_bottom", "side_offset": 2, "top_offset": 1, "bottom_offset": 0, "round_up": 0});
        run(&mut s, "cam.setup", json!({"name": "Top", "stock_spec": stock, "wcs_origin_mode": "stock_box_point"}));
        assert!(near(setup(&s).work_origin, [-2.0, -2.0, 9.0]), "{:?}", setup(&s).work_origin);
        let outline = s.cam_setup_outline("Top").unwrap();
        let corners = outline.stock.unwrap();
        assert!(corners.iter().any(|c| c.dist(Vec3::new(42.0, 32.0, 0.0)) < 1e-6), "{corners:?}");
        // Turned on its side: +Z out of the face at x = 0, X derived, zero again at a box corner.
        run(
            &mut s,
            "cam.setup",
            json!({"name": "Side", "z_face": {"body": "Plate", "at": [0, 15, 4]}, "wcs_origin_mode": "model_box_point", "box_point": corner}),
        );
        let side = s.doc.cam.setup("Side").unwrap();
        assert!(near(side.tool_axis, [-1.0, 0.0, 0.0]), "{:?}", side.tool_axis);
        assert!(near(side.work_origin, [0.0, 30.0, 0.0]), "{:?}", side.work_origin);
        // Part zero at the bore's centre, or mid-edge on a straight edge.
        run(&mut s, "cam.setup", json!({"name": "Top", "zero_edge": {"body": "Plate", "at": [25, 15, 8]}}));
        assert!(near(setup(&s).work_origin, [20.0, 15.0, 8.0]), "{:?}", setup(&s).work_origin);
        assert_eq!(setup(&s).wcs_origin_mode, WcsOriginMode::SelectedPoint);
        run(&mut s, "cam.setup", json!({"name": "Top", "zero_edge": {"body": "Plate", "at": [10, 0, 8]}}));
        assert!(near(setup(&s).work_origin, [20.0, 0.0, 8.0]), "{:?}", setup(&s).work_origin);
        // Turning an axis round happens once and is kept.
        run(&mut s, "cam.setup", json!({"name": "Top", "flip_x": true}));
        assert!(near(setup(&s).x_axis.unwrap(), [-1.0, 0.0, 0.0]));
        run(&mut s, "cam.setup", json!({"name": "Top", "work_offset": 2}));
        assert!(near(setup(&s).x_axis.unwrap(), [-1.0, 0.0, 0.0]));
        run(&mut s, "cam.setup", json!({"name": "Top", "flip_z": true}));
        assert!(near(setup(&s).tool_axis, [0.0, 0.0, -1.0]));
        // Zero axis by axis: X on the left face, Y on the front face, Z on the top; then X on the bore's centre.
        let axes =
            json!({"x": {"body": "Plate", "at": [0, 15, 4]}, "y": {"body": "Plate", "at": [20, 0, 4]}, "z": {"body": "Plate", "at": [5, 5, 8]}});
        run(&mut s, "cam.setup", json!({"name": "Axes", "zero_axes": axes}));
        let axes = s.doc.cam.setup("Axes").unwrap();
        assert!(near(axes.work_origin, [0.0, 0.0, 8.0]) && axes.wcs_origin_mode == WcsOriginMode::SelectedPoint, "{:?}", axes.work_origin);
        run(&mut s, "cam.setup", json!({"name": "Axes", "zero_axes": {"x": {"body": "Plate", "at": [25, 15, 8], "edge": true}}}));
        assert!(near(s.doc.cam.setup("Axes").unwrap().work_origin, [20.0, 0.0, 8.0]));
        // X on the bore's centre, Y and Z from the box: high Y, the top.
        let mixed = json!({"name": "Mixed", "wcs_origin_mode": "model_box_point", "box_point": {"x": "negative", "y": "positive", "z": "positive"},
            "zero_axes": {"x": {"body": "Plate", "at": [25, 15, 8], "edge": true}}});
        run(&mut s, "cam.setup", mixed);
        let mixed = s.doc.cam.setup("Mixed").unwrap();
        assert!(near(mixed.work_origin, [20.0, 30.0, 8.0]) && mixed.wcs_origin_mode == WcsOriginMode::SelectedPoint, "{:?}", mixed.work_origin);
        // Bad picks change nothing: a curved face, a round edge, an edge along the tool axis, no body.
        let before = s.doc.to_json();
        for p in [
            json!({"name": "Top", "z_face": {"body": "Plate", "at": [25, 15, 4]}}),
            json!({"name": "Top", "x_edge": {"body": "Plate", "at": [25, 15, 8]}}),
            json!({"name": "Top", "x_edge": {"body": "Plate", "at": [0, 0, 4]}}),
            json!({"name": "Top", "z_face": {"body": "Nope", "at": [5, 5, 8]}}),
            json!({"name": "Top", "z_face": {"body": "Plate"}}),
            json!({"name": "Top", "zero_axes": {"w": {"body": "Plate", "at": [0, 0, 0]}}}),
        ] {
            assert!(s.execute("cam.setup", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc.to_json(), before);
    }

    #[test]
    fn facing_takes_the_stock_from_its_top_and_runs_past_its_edges() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "new", "body_name": "Plate"}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 10, "flute_length": 30}));
        // Stock 2 mm over the part on top and 5 mm at the sides; part zero on the part's top.
        let stock =
            json!({"kind": "relative_box", "mode": "sides_and_top_bottom", "side_offset": 5, "top_offset": 2, "bottom_offset": 0, "round_up": 0});
        run(&mut s, "cam.setup", json!({"name": "Top", "work_origin": [0, 0, 8], "stock_spec": stock}));
        run(&mut s, "cam.op.face", json!({"setup": "Top", "name": "Planen", "tool": 1, "bottom": 0, "overrun": 1}));
        let Some(CamOperation::Face { cut, params, geometry, .. }) = s.doc.cam.setup("Top").and_then(|x| x.operations.first()) else {
            panic!("a facing step")
        };
        // From the stock's top, a full diameter deep at most, 70 % of it apart.
        assert_eq!((cut.top.as_str(), cut.stepdown.as_str(), params.stepover.as_str(), geometry), ("2 mm", "10 mm", "7 mm", &CamGeometry::Stock));
        // The retract and clearance heights stay above the start.
        assert_eq!((cut.retract.as_str(), cut.clearance.as_str()), ("4 mm", "12 mm"));
        let paths = s.cam_toolpaths();
        let points = paths[0].points.as_ref().unwrap();
        let cuts: Vec<Vec3> = points.iter().filter(|(_, c)| *c).map(|(p, _)| *p).collect();
        // Stock X -5…45: lanes from 1 + 5 mm before it to 1 + 5 mm after it; nothing below the top face.
        let (lo, hi) = cuts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        assert!((lo - -11.0).abs() < 1e-6 && (hi - 51.0).abs() < 1e-6, "{lo} {hi}");
        assert!(cuts.iter().all(|p| p.z >= 8.0 - 1e-6));
        run(&mut s, "cam.post", json!({"setup": "Top"}));
        // A rectangle instead of the stock; nothing to face when the top is not above the bottom.
        run(
            &mut s,
            "cam.op.face",
            json!({"setup": "Top", "name": "Feld", "tool": 1, "outer": [[0, 0], [20, 0], [20, 10], [0, 10]], "top": 1, "bottom": 0}),
        );
        assert!(s.execute("cam.op.face", &json!({"setup": "Top", "name": "Nichts", "tool": 1, "top": 0, "bottom": 0})).is_err());
        // The guided job faces the stock too.
        let job = json!({"setup": "Top", "name": "J", "kind": "face", "tool": 1, "diameter": 10, "flute_length": 30,
            "x": 0, "y": 0, "width": 40, "height": 30, "top": 2, "bottom": 0, "clearance": 10, "retract": 3, "stepdown": 1,
            "stepover": 6, "feed": 800, "plunge_feed": 200, "spindle_rpm": 8000, "work_offset": 1, "overrun": 3});
        run(&mut s, "cam.job.create", job);
        let Some(CamOperation::Face { params, geometry, .. }) = s.doc.cam.setup("Top").and_then(|x| x.operations.iter().find(|o| o.name() == "J"))
        else {
            panic!("no facing J");
        };
        assert_eq!((params.overrun.as_str(), geometry), ("3", &CamGeometry::Stock));
        // A picked face says where to face: the base gives the whole footprint (X 0…40). The
        // allowance says how much lies above the finished height; safety heights given below the
        // start are raised above it.
        let base = json!({"setup": "Top", "name": "Grund", "tool": 1, "faces": [{"body": "Plate", "at": [5, 5, 0]}],
            "bottom": 1, "allowance": 2, "retract": 2, "clearance": 2, "overrun": 1});
        run(&mut s, "cam.op.face", base);
        let Some(CamOperation::Face { cut, geometry, .. }) = s.doc.cam.setup("Top").and_then(|x| x.operations.iter().find(|o| o.name() == "Grund"))
        else {
            panic!("no facing Grund");
        };
        assert!(matches!(geometry, CamGeometry::Faces { .. }), "{geometry:?}");
        assert_eq!((cut.top.as_str(), cut.bottom.as_str(), cut.retract.as_str(), cut.clearance.as_str()), ("3 mm", "1", "5 mm", "13 mm"));
        let paths = s.cam_toolpaths();
        let grund = paths.iter().find(|p| p.operation == "Grund").and_then(|p| p.points.as_ref().ok()).unwrap();
        let (lo, hi) = grund.iter().filter(|(_, c)| *c).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (p, _)| (a.min(p.x), b.max(p.x)));
        assert!((lo - -6.0).abs() < 1e-6 && (hi - 46.0).abs() < 1e-6, "{lo} {hi}");
        // A finished height at or above the start leaves nothing to face, and says so.
        let err = s.execute("cam.op.face", &json!({"setup": "Top", "name": "Hoch", "tool": 1, "bottom": 3})).unwrap_err().to_string();
        assert!(err.contains("nothing to face"), "{err}");
        assert!(s.execute("cam.op.face", &json!({"setup": "Top", "name": "Null", "tool": 1, "bottom": 0, "allowance": 0})).is_err());
    }

    #[test]
    fn holding_tabs_lift_over_the_bottom_of_a_closed_contour_only() {
        let mut s = Session::default();
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "spindle_rpm": 8000, "feed": 600, "plunge_feed": 200}));
        run(&mut s, "cam.setup", json!({"name": "Top", "stock": {"min": [-10, -10, -6], "max": [50, 40, 0]}}));
        let op = |tabs: Value| {
            json!({"setup": "Top", "name": "Outside", "tool": 1, "outer": [[0, 0], [40, 0], [40, 30], [0, 30]],
                "side": "outside", "bottom": -6, "stepdown": 3, "tabs": tabs})
        };
        let lowest_heights = |s: &mut Session| {
            let paths = s.cam_toolpaths();
            let points = paths[0].points.clone().unwrap();
            let mut zs: Vec<f64> = points.iter().filter(|(_, cutting)| *cutting).map(|(p, _)| (p.z * 1000.0).round() / 1000.0).collect();
            zs.sort_by(f64::total_cmp);
            zs.dedup();
            zs
        };
        run(&mut s, "cam.op.contour", op(Value::Null));
        assert!(!lowest_heights(&mut s).contains(&-4.0));
        // Four tabs 2 mm high: the last pass crosses each at -4.
        run(&mut s, "cam.op.contour", op(json!({"count": 4, "width": 5, "height": 2})));
        let zs = lowest_heights(&mut s);
        assert!(zs.contains(&-6.0) && zs.contains(&-4.0), "{zs:?}");
        let listed = run(&mut s, "cam.list", json!({}));
        assert_eq!(listed["setups"][0]["operations"][0]["params"]["tabs"]["count"], 4, "{listed}");
        assert!(run(&mut s, "cam.post", json!({"setup": "Top"}))["readback"]["within"].as_bool().unwrap());
        // Tabs need a size and a count or spacing, and must be lower than the cut.
        for bad in [
            json!({"width": 5, "height": 2}),
            json!({"count": 4, "height": 2}),
            json!({"count": -1, "width": 5, "height": 2}),
            json!({"count": 4, "width": 5, "height": 9}),
        ] {
            let e = s.execute("cam.op.contour", &op(bad)).unwrap_err().to_string();
            assert!(e.contains("tab"), "{e}");
        }
        // Null takes them off again.
        run(&mut s, "cam.op.contour", op(Value::Null));
        assert!(s.doc.cam.setups[0].operations.iter().all(|o| !matches!(o, CamOperation::Contour { params, .. } if params.tabs.is_some())));
    }

    #[test]
    fn machines_hold_speeds_and_refuse_programs_beyond_them() {
        let mut s = Session::default();
        let cutting = json!({"aluminium": {"vc": 188.4955592153876, "fz": 0.05, "ap": 0.5, "ae": 0.4}});
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "flutes": 2, "cutting": cutting}));
        let mill = json!({"name": "Fräse", "x": [0, 300], "y": [0, 200], "z": [-100, 0], "a": {"kind": "endless"},
            "spindle_min": 2000, "spindle_max": 8000, "max_feed": 2000});
        // A setup can take the machine in full: it is copied into the design.
        run(&mut s, "cam.setup", json!({"name": "Top", "material": "aluminium", "machine": mill}));
        assert_eq!(s.doc.cam.setup("Top").unwrap().machine.as_deref(), Some("Fräse"));
        assert_eq!(s.doc.cam.machine("Fräse").unwrap().a, Some(solidcraft_cam::RotaryAxis::Endless));
        assert!(run(&mut s, "cam.list", json!({}))["machines"].as_array().is_some_and(|m| m.len() == 1));
        // Cutting data are held to the machine: 10000 rev/min becomes 8000, the feed follows.
        let speeds = run(&mut s, "cam.tool.speeds", json!({"tool": 1, "setup": "Top"}));
        assert_eq!((speeds["spindle_rpm"].as_f64(), speeds["feed"].as_f64()), (Some(8000.0), Some(800.0)));
        // A program within the machine posts; a faster spindle, a faster feed or a wider reach does not.
        let op = |name: &str, x1: f64, rpm: f64, feed: f64| {
            json!({"setup": "Top", "name": name, "tool": 1, "outer": [[0, 0], [x1, 0], [x1, 30], [0, 30]],
                "bottom": -1, "stepdown": 1, "spindle_rpm": rpm, "feed": feed})
        };
        run(&mut s, "cam.op.contour", op("A", 40.0, 6000.0, 800.0));
        assert_eq!(run(&mut s, "cam.post", json!({"setup": "Top"}))["post"], "rs274");
        // The machine's post writes its programs; the setup's own and an explicit one come first.
        let with_post = |post: &str| {
            let mut m = mill.clone();
            m["post"] = json!(post);
            m
        };
        run(&mut s, "cam.machine.define", with_post("linuxcnc"));
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        assert_eq!(posted["post"], "linuxcnc");
        assert!(posted["gcode"].as_str().unwrap().contains("G91.1"));
        assert_eq!(run(&mut s, "cam.post", json!({"setup": "Top", "post": "rs274"}))["post"], "rs274");
        run(&mut s, "cam.setup", json!({"name": "Top", "post": "rs274"}));
        assert_eq!(run(&mut s, "cam.post", json!({"setup": "Top"}))["post"], "rs274");
        run(&mut s, "cam.setup", json!({"name": "Top", "post": null}));
        // An unknown post is refused and changes nothing.
        let before = s.doc.to_json();
        assert!(s.execute("cam.machine.define", &with_post("Heidenhain 9000")).is_err());
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "cam.post", json!({"setup": "Top"}));
        for (bad, why) in
            [(op("A", 40.0, 9000.0, 800.0), "spindle"), (op("A", 40.0, 6000.0, 2500.0), "feed"), (op("A", 400.0, 6000.0, 800.0), "along X")]
        {
            run(&mut s, "cam.op.contour", bad);
            let e = s.execute("cam.post", &json!({"setup": "Top"})).unwrap_err().to_string();
            assert!(e.contains(why) && e.contains("Fräse"), "{e}");
        }
        // Material left on the floor: the last pass stops above the bottom.
        run(
            &mut s,
            "cam.op.contour",
            json!({"setup": "Top", "name": "A", "tool": 1, "outer": [[0, 0], [40, 0], [40, 30], [0, 30]],
            "bottom": -2, "stepdown": 1, "floor_stock": 0.5, "spindle_rpm": 6000, "feed": 800}),
        );
        let paths = s.cam_toolpaths();
        let lowest = paths[0].points.as_ref().unwrap().iter().map(|(p, _)| p.z).fold(f64::INFINITY, f64::min);
        assert!((lowest - -1.5).abs() < 1e-9, "{lowest}");
        let too_much = json!({"setup": "Top", "name": "B", "tool": 1, "outer": [[0, 0], [40, 0], [40, 30], [0, 30]], "bottom": -1, "floor_stock": 2});
        assert!(s.execute("cam.op.contour", &too_much).is_err());
        // A used machine stays; an unknown name is refused; null clears it.
        assert!(s.execute("cam.machine.remove", &json!({"name": "Fräse"})).is_err());
        assert!(s.execute("cam.setup", &json!({"name": "Top", "machine": "Drehbank"})).is_err());
        run(&mut s, "cam.setup", json!({"name": "Top", "machine": null}));
        run(&mut s, "cam.machine.remove", json!({"name": "Fräse"}));
        assert!(s.doc.cam.machines.is_empty());
        // Bad limits are refused and change nothing.
        let before = s.doc.to_json();
        for bad in [
            json!({"name": "M", "x": [10, 0], "y": [0, 1], "z": [0, 1], "spindle_min": 0, "spindle_max": 1, "max_feed": 1}),
            json!({"name": "M", "x": [0, 1], "y": [0, 1], "z": [0, 1], "spindle_min": 5, "spindle_max": 1, "max_feed": 1}),
        ] {
            assert!(s.execute("cam.machine.define", &bad).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
        // The guided job hands on the side, the direction and both allowances.
        let job = json!({"setup": "Top", "name": "J", "kind": "contour", "tool": 1, "diameter": 6, "flute_length": 20,
            "x": 0, "y": 0, "width": 40, "height": 30, "top": 0, "bottom": -2, "clearance": 10, "retract": 2, "stepdown": 1,
            "stepover": 2, "feed": 600, "plunge_feed": 150, "spindle_rpm": 6000, "work_offset": 1,
            "side": "inside", "climb": false, "stock_to_leave": 0.3, "floor_stock": 0.2});
        run(&mut s, "cam.job.create", job);
        let Some(CamOperation::Contour { params, .. }) = s.doc.cam.setup("Top").and_then(|x| x.operations.iter().find(|o| o.name() == "J")) else {
            panic!("no contour J");
        };
        assert_eq!((params.side, params.climb, params.stock_to_leave.as_str(), params.floor_stock.as_str()), (CamSide::Inside, false, "0.3", "0.2"));
    }

    #[test]
    fn job_order_skips_material_and_cutting_speeds() {
        let mut s = Session::default();
        let cutting = json!({"aluminium": {"vc": 188.4955592153876, "fz": 0.05, "ap": 0.5, "ae": 0.4}});
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "flutes": 2, "cutting": cutting}));
        run(&mut s, "cam.tool.define", json!({"number": 2, "diameter": 3, "flute_length": 12}));
        run(&mut s, "cam.setup", json!({"name": "Top", "material": "aluminium", "max_rpm": 24000}));
        for (name, tool) in [("A", 1), ("B", 2), ("C", 1)] {
            let outer = json!([[0, 0], [40, 0], [40, 30], [0, 30]]);
            run(&mut s, "cam.op.contour", json!({"setup": "Top", "name": name, "tool": tool, "outer": outer, "bottom": -1, "stepdown": 1}));
        }
        // Own data: n = 188.5 · 1000 / (π · 6) = 10000, vf = 10000 · 2 · 0.05.
        let own = run(&mut s, "cam.tool.speeds", json!({"tool": 1, "setup": "Top"}));
        assert_eq!((own["spindle_rpm"].as_f64(), own["feed"].as_f64(), own["own"].as_bool()), (Some(10000.0), Some(1000.0), Some(true)));
        // Starting values, held to the machine's highest speed.
        let start = run(&mut s, "cam.tool.speeds", json!({"tool": 2, "setup": "Top"}));
        assert_eq!((start["spindle_rpm"].as_f64(), start["own"].as_bool()), (Some(24000.0), Some(false)));
        assert!(s.execute("cam.tool.speeds", &json!({"tool": 2, "material": "unobtainium"})).is_err());
        // Reorder and skip; the post follows the order and leaves skipped steps out.
        let order = run(&mut s, "cam.op.move", json!({"setup": "Top", "name": "C", "to": 0}));
        assert_eq!(order["order"], json!(["C", "A", "B"]));
        run(&mut s, "cam.op.skip", json!({"setup": "Top", "name": "A", "skip": true}));
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        assert_eq!(posted["operations"], 2);
        // The viewport draws every step's path, the skipped one marked.
        let paths = s.cam_toolpaths();
        assert_eq!(paths.iter().map(|p| (p.operation.as_str(), p.skipped)).collect::<Vec<_>>(), [("C", false), ("A", true), ("B", false)]);
        assert!(paths.iter().all(|p| p.points.as_ref().is_ok_and(|v| v.iter().any(|(_, cutting)| *cutting))));
        let gcode = posted["gcode"].as_str().unwrap();
        let (c, b) = (gcode.find("(C:").unwrap_or(usize::MAX), gcode.find("(B:").unwrap_or(usize::MAX));
        assert!(c < b && !gcode.contains("(A:"), "{gcode}");
        // The setup keeps order, skips, material and speed through a setup edit, undo and a save.
        run(&mut s, "cam.setup", json!({"name": "Top", "work_offset": 2}));
        let setup = s.doc.cam.setup("Top").unwrap();
        assert_eq!((setup.skipped.clone(), setup.material.clone(), setup.max_rpm), (vec!["A".to_string()], Some("aluminium".into()), Some(24000.0)));
        let cold = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(cold.cam.setup("Top").unwrap().operations[0].name(), "C");
        assert_eq!(cold.cam.tool(1).unwrap().cutting.len(), 1);
        run(&mut s, "cam.op.skip", json!({"setup": "Top", "name": "A", "skip": false}));
        assert!(s.doc.cam.setup("Top").unwrap().skipped.is_empty());
        // Bad input changes nothing.
        let before = s.doc.to_json();
        for (id, p) in [
            ("cam.op.move", json!({"setup": "Top", "name": "Z", "to": 0})),
            ("cam.op.skip", json!({"setup": "Nope", "name": "A", "skip": true})),
            ("cam.setup", json!({"name": "Top", "material": " "})),
            ("cam.setup", json!({"name": "Top", "max_rpm": -5})),
            (
                "cam.tool.define",
                json!({"number": 3, "diameter": 6, "flute_length": 20, "cutting": {"wood": {"vc": -1, "fz": 0.1, "ap": 1, "ae": 0.5}}}),
            ),
        ] {
            assert!(s.execute(id, &p).is_err(), "{id} {p}");
        }
        assert_eq!(s.doc.to_json(), before);
    }

    #[test]
    fn circle_sketch_posts_g2_or_g3() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
        let sketch = s.active_sketch.expect("sketch");
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 4, "flute_length": 16, "feed": 400, "plunge_feed": 100, "stepdown": 2}));
        run(&mut s, "cam.setup", json!({"name": "Top"}));
        run(
            &mut s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Boss", "tool": 1,
                "sketch": sketch, "side": "outside", "bottom": -2, "stepdown": 2,
            }),
        );
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let gcode = posted["gcode"].as_str().unwrap();
        assert!(gcode.contains("G2 ") || gcode.contains("G3 "), "{gcode}");
        assert!(posted["readback"]["within"].as_bool().unwrap());
    }
}
