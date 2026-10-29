//! Guided 2.5D jobs and LinuxCNC file export. Preview uses an isolated document.
use super::{CommandSpec, cam};
use crate::params::bad;
use crate::schema::cam::{ExportParams, ExportResult, JobKind, JobParams, JobResult, PreviewPoint};
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};

const JOB: crate::schema::TypedContract<JobParams> = crate::schema::TypedContract::new();
const EXPORT: crate::schema::TypedContract<ExportParams> = crate::schema::TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.job.create", "2.5D CAM — LinuxCNC", crate::checked!("cam.job.create", create))
        .icon("cut").contract(&JOB)
        .output(crate::schema::output_schema_for::<JobResult>)
        .params("Guided rectangle contour/pocket or hole-pattern job in work XY; dimensions mm, feeds mm/min. Creates or replaces one named operation; returns LinuxCNC G-code and preview points."),
    CommandSpec::new("cam.job.preview", "Preview CAM Job", crate::checked!("cam.job.preview", preview))
        .icon("cut").noundo().contract(&JOB).output(crate::schema::output_schema_for::<JobResult>)
        .params("Same inputs as cam.job.create; computes in an isolated document without changing the design."),
    CommandSpec::new("cam.export", "Export LinuxCNC G-code", crate::checked!("cam.export", export))
        .icon("export").noundo().contract(&EXPORT).output(crate::schema::output_schema_for::<ExportResult>)
        .params("setup; path; overwrite?: false. Posts the complete setup using LinuxCNC, verifies the read-back, then writes atomically."),
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
    cam::tool_cmd(
        &mut work,
        &json!({"number": a.tool, "kind": if matches!(a.kind, JobKind::Drill) { "drill" } else { "flat_endmill" }, "diameter": a.diameter, "flute_length": a.flute_length, "spindle_rpm": a.spindle_rpm}),
    )?;
    let mut setup = json!({"name": a.setup, "work_offset": a.work_offset});
    if let Some(material) = &a.material {
        setup["material"] = json!(material);
    }
    cam::setup_cmd(&mut work, &setup)?;
    let mut op = json!({"setup": a.setup, "name": a.name, "tool": a.tool,
        "outer": [[a.x,a.y],[a.x+a.width,a.y],[a.x+a.width,a.y+a.height],[a.x,a.y+a.height]],
        "top": a.top, "bottom": a.bottom, "clearance": a.clearance, "retract": a.retract,
        "stepdown": a.stepdown, "stepover": a.stepover, "feed": a.feed,
        "plunge_feed": a.plunge_feed, "spindle_rpm": a.spindle_rpm, "coolant": a.coolant});
    if picked && !matches!(a.kind, JobKind::Drill | JobKind::Face) {
        let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
        map.remove("outer");
        for (key, value) in [("sketch", &a.sketch), ("profiles", &a.profiles), ("faces", &a.faces), ("edges", &a.edges), ("geometry", &a.geometry)] {
            if let Some(value) = value {
                map.insert(key.into(), value.clone());
            }
        }
    }
    if let (Some(start), JobKind::Contour) = (a.start, &a.kind) {
        op["start"] = json!(start);
    }
    if !matches!(a.kind, JobKind::Drill | JobKind::Face) {
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
        JobKind::Pocket => {
            cam::pocket_cmd(&mut work, &op)?;
        }
        JobKind::Face => {
            // The setup's stock outline, overrun on every side.
            let map = op.as_object_mut().ok_or_else(|| bad(cmd, "invalid operation"))?;
            map.remove("outer");
            if let Some(v) = a.overrun {
                if !v.is_finite() || !(0.0..=1.0e6).contains(&v) {
                    return Err(bad(cmd, "`overrun` must be 0 or more"));
                }
                map.insert("overrun".into(), json!(v));
            }
            cam::face_cmd(&mut work, &op)?;
        }
        JobKind::Drill => {
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
            if holes.is_empty() {
                map.insert("points".into(), json!(a.points));
            } else {
                map.insert("holes".into(), json!(holes));
            }
            map.insert("dwell".into(), json!(a.dwell));
            cam::drill_cmd(&mut work, &op)?;
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
    let posted = cam::post_cmd(&mut work, &json!({"setup": a.setup, "post": "linuxcnc"}))?;
    let result = JobResult {
        setup: a.setup,
        operation: a.name,
        gcode: posted.get("gcode").and_then(Value::as_str).ok_or_else(|| bad(cmd, "post returned no G-code"))?.into(),
        extension: "ngc".into(),
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
    let result = cam::post_cmd(s, &json!({"setup": a.setup, "post": "linuxcnc"}))?;
    let text = result.get("gcode").and_then(Value::as_str).ok_or_else(|| bad(cmd, "post returned no G-code"))?;
    if a.overwrite {
        solidcraft_io::write_atomic(std::path::Path::new(&a.path), text.as_bytes(), true)
    } else {
        solidcraft_io::safe_write::create_atomic(std::path::Path::new(&a.path), text.as_bytes())
    }
    .map_err(|e| EngineError::Other(format!("{}: {e}", a.path)))?;
    Ok(json!({"path": a.path, "bytes": text.len()}))
}
