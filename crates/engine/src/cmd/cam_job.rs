//! Guided 2.5D jobs and LinuxCNC file export. Preview uses an isolated document.
use super::{CommandSpec, cam};
use crate::params::bad;
use crate::schema::cam::{ExportParams, ExportResult, JobKind, JobParams, JobResult, PreviewPoint};
use solidcraft_doc::cam::SetupOperationType;
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};

const JOB: crate::schema::TypedContract<JobParams> = crate::schema::TypedContract::new();
const EXPORT: crate::schema::TypedContract<ExportParams> = crate::schema::TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.job.create", "2.5D CAM — LinuxCNC", crate::checked!("cam.job.create", create))
        .icon("cam_contour").contract(&JOB)
        .output(crate::schema::output_schema_for::<JobResult>)
        .params("Guided rectangle contour/pocket or hole-pattern job in work XY; dimensions mm, feeds mm/min. Creates or replaces one named operation; returns LinuxCNC G-code and preview points."),
    CommandSpec::new("cam.job.preview", "Preview CAM Job", crate::checked!("cam.job.preview", preview))
        .icon("cut").noundo().contract(&JOB).output(crate::schema::output_schema_for::<JobResult>)
        .params("Same inputs as cam.job.create; computes in an isolated document without changing the design."),
    CommandSpec::new("cam.export", "Export LinuxCNC G-code", crate::checked!("cam.export", export))
        .icon("export").noundo().contract(&EXPORT).output(crate::schema::output_schema_for::<ExportResult>)
        .params("setup; path; overwrite?: false; text?: the program as edited by hand, written as it is (at most 64 MB), neither posted nor read back → {path, bytes, checked}. Without text: posts the complete setup with its post, verifies the read-back, then writes atomically."),
];

fn build(s: &Session, p: &Value, cmd: &str) -> Result<(Session, JobResult)> {
    let a: JobParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let values = [
        a.x,
        a.y,
        a.width,
        a.height,
        a.top,
        a.bottom,
        a.clearance,
        a.retract,
        a.stepdown,
        a.stepover,
        a.feed,
        a.plunge_feed,
        a.spindle_rpm,
        a.diameter,
        a.flute_length,
        a.dwell,
    ];
    if values.iter().any(|v| !v.is_finite() || v.abs() > 1.0e6) || !(1..=6).contains(&a.work_offset) {
        return Err(bad(cmd, "finite values within 1e6 and a work offset 1 to 6 are required"));
    }
    let picked = a.sketch.is_some() || a.faces.is_some() || a.edges.is_some() || a.geometry.is_some();
    if (!picked && (a.width <= 0.0 || a.height <= 0.0)) || a.bottom >= a.top {
        return Err(bad(cmd, "width, height and cutting depth must be positive"));
    }
    let mut work = s.scratch();
    // The design's own tool of that number keeps its kind (a tap, a chamfer cutter, ...) and the
    // sizes that go with it, as long as it cuts the way the job does; otherwise a plain drill
    // or end mill of the typed size.
    let at_points = matches!(a.kind, JobKind::Drill | JobKind::Tap);
    let kind = match work.doc.cam.tool(a.tool) {
        Some(t) if t.kind.cuts_down() == at_points => t.kind.key(),
        _ if at_points => "drill",
        _ => "flat_endmill",
    };
    // A plasma cut needs no tool; a new setup for it is a plasma setup.
    let plasma = matches!(a.kind, JobKind::Plasma);
    if !plasma {
        cam::tool_cmd(
            &mut work,
            &json!({"number": a.tool, "kind": kind, "diameter": a.diameter, "flute_length": a.flute_length, "spindle_rpm": a.spindle_rpm}),
        )?;
    }
    let mut setup = json!({"name": a.setup, "work_offset": a.work_offset});
    if plasma && work.doc.cam.setup(&a.setup).is_none() {
        setup["operation_type"] = json!("plasma");
    }
    if let Some(material) = &a.material {
        setup["material"] = json!(material);
    }
    cam::setup_cmd(&mut work, &setup)?;
    let mut op = json!({"setup": a.setup, "name": a.name, "tool": a.tool,
        "outer": [[a.x,a.y],[a.x+a.width,a.y],[a.x+a.width,a.y+a.height],[a.x,a.y+a.height]],
        "top": a.top, "bottom": a.bottom, "clearance": a.clearance, "retract": a.retract,
        "stepdown": a.stepdown, "stepover": a.stepover, "feed": a.feed,
        "plunge_feed": a.plunge_feed, "spindle_rpm": a.spindle_rpm, "coolant": a.coolant});
    let holes_job = matches!(a.kind, JobKind::Drill | JobKind::Helix | JobKind::Thread | JobKind::Tap);
    if picked && !holes_job && !matches!(a.kind, JobKind::Face) {
        let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
        map.remove("outer");
        for (key, value) in [("sketch", &a.sketch), ("profiles", &a.profiles), ("faces", &a.faces), ("edges", &a.edges), ("geometry", &a.geometry)] {
            if let Some(value) = value {
                map.insert(key.into(), value.clone());
            }
        }
        if let Some(chain) = a.chain {
            map.insert("chain".into(), json!(chain));
        }
    }
    if let (JobKind::Contour | JobKind::Plasma, Some(map)) = (&a.kind, op.as_object_mut()) {
        let extra = [
            ("lead_in", a.lead_in.clone()),
            ("lead_out", a.lead_out.clone()),
            ("overlap", a.overlap.map(|v| json!(v))),
            ("open_side", a.open_side.clone().map(|v| json!(v))),
            ("reverse", a.reverse.map(|v| json!(v))),
            ("extend_start", a.extend_start.map(|v| json!(v))),
            ("extend_end", a.extend_end.map(|v| json!(v))),
            ("tabs", a.tabs.clone()),
            ("compensation", a.compensation.clone().map(|v| json!(v))),
        ];
        for (key, value) in extra {
            if let Some(value) = value {
                map.insert(key.into(), value);
            }
        }
    }
    if let (Some(start), JobKind::Contour | JobKind::Plasma) = (a.start, &a.kind) {
        op["start"] = json!(start);
    }
    if matches!(a.kind, JobKind::Contour | JobKind::Pocket | JobKind::Slot) {
        if let (Some(side), JobKind::Contour) = (&a.side, &a.kind) {
            op["side"] = json!(side);
        }
        if let Some(climb) = a.climb {
            op["climb"] = json!(climb);
        }
        for (key, value) in [("stock_to_leave", a.stock_to_leave), ("floor_stock", a.floor_stock)] {
            match value {
                Some(v) if !v.is_finite() || !(0.0..=1.0e6).contains(&v) => return Err(bad(cmd, format!("`{key}` must be 0 or more"))),
                // Nothing left is the default.
                Some(v) if v > 0.0 => op[key] = json!(v),
                _ => {}
            }
        }
    }
    match a.kind {
        JobKind::Contour => {
            cam::contour_cmd(&mut work, &op)?;
        }
        JobKind::Plasma => {
            let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
            for key in ["tool", "stepdown", "stepover", "spindle_rpm", "coolant", "tabs"] {
                map.remove(key);
            }
            if let Some(side) = &a.side {
                map.insert("side".into(), json!(side));
            }
            let torch = [("kerf", a.kerf), ("pierce_height", a.pierce_height), ("cut_height", a.cut_height), ("pierce_delay", a.pierce_delay)];
            for (key, value) in torch {
                if let Some(v) = value {
                    map.insert(key.into(), json!(v));
                }
            }
            if let Some(slow) = &a.slow_holes {
                map.insert("slow_holes".into(), slow.clone());
            }
            cam::plasma_cmd(&mut work, &op)?;
        }
        JobKind::Pocket => {
            cam::pocket_cmd(&mut work, &op)?;
        }
        JobKind::Slot => {
            let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
            map.remove("floor_stock");
            map.remove("climb");
            if let Some(v) = a.slot_width {
                if !v.is_finite() || !(v > 0.0 && v <= 1.0e4) {
                    return Err(bad(cmd, "`slot_width` must be more than 0 mm"));
                }
                map.insert("width".into(), json!(v));
            }
            cam::slot_cmd(&mut work, &op)?;
        }
        JobKind::Face => {
            // A picked face's outline, else the setup's stock outline, overrun on every side;
            // facing starts `top - bottom` above the finished height.
            let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
            map.remove("outer");
            map.remove("top");
            map.insert("allowance".into(), json!(a.top - a.bottom));
            for (key, value) in
                [("sketch", &a.sketch), ("profiles", &a.profiles), ("faces", &a.faces), ("edges", &a.edges), ("geometry", &a.geometry)]
            {
                if let Some(value) = value {
                    map.insert(key.into(), value.clone());
                }
            }
            if let Some(v) = a.overrun {
                if !v.is_finite() || !(0.0..=1.0e6).contains(&v) {
                    return Err(bad(cmd, "`overrun` must be 0 or more"));
                }
                map.insert("overrun".into(), json!(v));
            }
            cam::face_cmd(&mut work, &op)?;
        }
        JobKind::Chamfer | JobKind::Engrave => {
            let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
            map.remove("stepover");
            if matches!(a.kind, JobKind::Chamfer) {
                // Its depth follows from its width; the stored bottom is the edge's height.
                map.remove("bottom");
                map.remove("stepdown");
                let width = a.chamfer_width.filter(|w| w.is_finite() && *w > 0.0).ok_or_else(|| bad(cmd, "a chamfer needs its `chamfer_width` above 0"))?;
                map.insert("width".into(), json!(width));
                if let Some(v) = a.tip_offset {
                    map.insert("tip_offset".into(), json!(v));
                }
                if let Some(side) = &a.side {
                    map.insert("side".into(), json!(side));
                }
                cam::chamfer_cmd(&mut work, &op)?;
            } else {
                cam::engrave_cmd(&mut work, &op)?;
            }
        }
        JobKind::Drill | JobKind::Helix | JobKind::Thread | JobKind::Tap => {
            let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
            map.remove("outer");
            map.remove("stepover");
            // Picked bores give the hole centres; typed points are used without picks.
            let mut holes: Vec<Value> = Vec::new();
            for e in a.edges.as_ref().and_then(Value::as_array).into_iter().flatten() {
                holes.push(json!({"body": e.get("body"), "at": e.get("at"), "edge": true}));
            }
            for f in a.faces.as_ref().and_then(Value::as_array).into_iter().flatten() {
                holes.push(json!({"body": f.get("body"), "at": f.get("at")}));
            }
            // A picked sketch profile drills its circles; picked sketch points are holes too.
            if let Some(sketch) = &a.sketch {
                let profiles =
                    a.profiles.as_ref().and_then(Value::as_array).ok_or_else(|| bad(cmd, "drilling a sketch needs `profiles`: profile indices"))?;
                for index in profiles {
                    let index = index.as_u64().ok_or_else(|| bad(cmd, "drilling a sketch takes `profiles` as profile indices"))?;
                    holes.push(json!({"sketch": sketch, "profile": index}));
                }
            }
            for point in a.sketch_points.iter().flatten() {
                holes.push(json!({"sketch": point.sketch, "point": point.point}));
            }
            if holes.is_empty() {
                map.insert("points".into(), json!(a.points));
            } else {
                map.insert("holes".into(), json!(holes));
            }
            match a.kind {
                JobKind::Helix | JobKind::Thread => {
                    // A helix at picked circles may leave its diameter to them.
                    match a.hole_diameter.filter(|d| d.is_finite() && *d > 0.0) {
                        Some(diameter) => {
                            map.insert("diameter".into(), json!(diameter));
                        }
                        None if matches!(a.kind, JobKind::Helix) && map.contains_key("holes") => {}
                        None => return Err(bad(cmd, "give the `hole_diameter`")),
                    }
                    if matches!(a.kind, JobKind::Thread) {
                        map.remove("stepdown");
                        for (key, value) in [("pitch", a.pitch.map(|v| json!(v))), ("internal", a.internal.map(|v| json!(v))), ("left_hand", a.left_hand.map(|v| json!(v)))] {
                            if let Some(value) = value {
                                map.insert(key.into(), value);
                            }
                        }
                        cam::thread_cmd(&mut work, &op)?;
                    } else {
                        // A pitch replaces the stepdown as the rise per turn; `internal: false` mills a boss.
                        for (key, value) in [("climb", a.climb.map(|v| json!(v))), ("pitch", a.pitch.map(|v| json!(v))), ("internal", a.internal.map(|v| json!(v)))] {
                            if let Some(value) = value {
                                map.insert(key.into(), value);
                            }
                        }
                        cam::helix_cmd(&mut work, &op)?;
                    }
                }
                JobKind::Tap => {
                    map.remove("stepdown");
                    map.remove("feed");
                    cam::tap_cmd(&mut work, &op)?;
                }
                _ => {
                    map.insert("dwell".into(), json!(a.dwell));
                    cam::drill_cmd(&mut work, &op)?;
                }
            }
        }
    }
    let setup = work.doc.cam.setup(&a.setup).ok_or_else(|| bad(cmd, "missing setup"))?;
    let operation = setup.operations.iter().find(|op| op.name() == a.name).ok_or_else(|| bad(cmd, "missing operation"))?;
    let path = cam::build_path(&work, setup, operation).map_err(|e| bad(cmd, e))?;
    let raw_points = path.checked_points(0.05, 100_000).map_err(|e| bad(cmd, e.to_string()))?;
    let points = raw_points
        .into_iter()
        .map(|(p, cutting)| {
            let m = cam::work_to_model(setup, p).map_err(|e| bad(cmd, e))?;
            Ok(PreviewPoint { position: [p.x, p.y, p.z], model: [m.x, m.y, m.z], cutting })
        })
        .collect::<Result<Vec<_>>>()?;
    let cut_minutes = path.stats().cut_minutes;
    let post = cam::chosen_post(&work.doc.cam, setup).unwrap_or_else(|| match setup.operation_type {
        SetupOperationType::Plasma => "plasma".into(),
        _ => "linuxcnc".into(),
    });
    let posted = cam::post_cmd(&mut work, &json!({"setup": a.setup, "post": post}))?;
    let result = JobResult {
        setup: a.setup,
        operation: a.name,
        gcode: posted.get("gcode").and_then(Value::as_str).ok_or_else(|| bad(cmd, "post returned no G-code"))?.into(),
        extension: posted.get("extension").and_then(Value::as_str).unwrap_or("ngc").into(),
        points,
        cut_minutes,
    };
    Ok((work, result))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let (work, result) = build(s, p, "cam.job.create")?;
    let out = serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))?;
    *s.doc_mut() = work.doc.as_ref().clone();
    Ok(out)
}

fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    let (_, result) = build(s, p, "cam.job.preview")?;
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.export";
    let a: ExportParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if a.path.is_empty() {
        return Err(bad(cmd, "path is required"));
    }
    solidcraft_io::vfs::check(&a.path).map_err(|e| EngineError::Other(e.to_string()))?;
    // Hand-edited text goes out as it is; the user has been told it is not checked.
    let posted;
    let (text, checked) = match &a.text {
        Some(text) => {
            if text.len() > 64 * 1024 * 1024 {
                return Err(bad(cmd, "the edited program is larger than 64 MB"));
            }
            if s.doc.cam.setup(&a.setup).is_none() {
                return Err(bad(cmd, format!("no setup `{}`", a.setup)));
            }
            (text.as_str(), false)
        }
        None => {
            let post = s.doc.cam.setup(&a.setup).and_then(|setup| cam::chosen_post(&s.doc.cam, setup)).unwrap_or_else(|| "linuxcnc".into());
            posted = cam::post_cmd(s, &json!({"setup": a.setup, "post": post}))?;
            (posted.get("gcode").and_then(Value::as_str).ok_or_else(|| bad(cmd, "post returned no G-code"))?, true)
        }
    };
    if a.overwrite {
        solidcraft_io::write_atomic(std::path::Path::new(&a.path), text.as_bytes(), true)
    } else {
        solidcraft_io::safe_write::create_atomic(std::path::Path::new(&a.path), text.as_bytes())
    }
    .map_err(|e| EngineError::Other(format!("{}: {e}", a.path)))?;
    Ok(json!({"path": a.path, "bytes": text.len(), "checked": checked}))
}
