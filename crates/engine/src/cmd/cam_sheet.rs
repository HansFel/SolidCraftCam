//! The manufacturing documents of a design's CAM data: the tool list (every tool its setups use,
//! with its sizes and holder and the steps it cuts in) and a setup sheet per setup (part zero,
//! axes, stock, material, fixtures, machine, its steps in order and their run times), as data,
//! or written as JSON, a CSV tool list or a printable HTML page.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::cam::{CamOperation, CamSetup};

use super::CommandSpec;
use super::cam;
use crate::params::bad;
use crate::schema::cam::{SheetHolder, SheetParams, SheetResult, SheetSetup, SheetStep, SheetStock, SheetTool, SheetTorch};
use crate::{EngineError, Result, Session};

const SHEET: crate::schema::TypedContract<SheetParams> = crate::schema::TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("cam.sheet", "Tool List and Setup Sheet", crate::checked!("cam.sheet", sheet))
    .icon("bom")
    .noundo()
    .contract(&SHEET)
    .output(crate::schema::output_schema_for::<SheetResult>)
    .params(
        "setup?: one setup; part?: the setups of one part (\"\" for setups that name none); default every setup; path?: write a .html (printable setup sheet and tool list), .csv (tool \
         list) or .json file; format?: html|csv|json (default from the path); overwrite?: false; title?: the page's heading; \
         labels?: {key: text} for the page's headings (`tools`, `setups`, `number`, `kind`, … and `kind.<tool kind>`, \
         `op.<step type>`) → tools: [{number, kind, description, diameter, flute_length, flutes, corner_radius, tip_angle, \
         tip_diameter, pitch, shank_diameter, overall_length, stick_out, holder, used_in: [\"setup / step\"]}] by number, \
         setups: [{name, part, operation_type, machine_kind, work_offset, origin, tool_axis, x_axis, part_zero, material, machine, post, fixtures, model_bodies, \
         stock: {kind, min, max, size} (work coordinates), steps: [{name, type, tool (none for a plasma cut), torch?: {kerf, pierce_height, cut_height, pierce_delay, feed}, compensation?: control|wear, skipped, seconds}], seconds, problem}], \
         seconds (all setups), path?, bytes?. A setup whose program cannot be posted has no run time and says why in `problem`.",
    )];

fn sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.sheet";
    let a: SheetParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if a.labels.len() > 512 || a.labels.iter().any(|(k, v)| k.len() > 128 || v.chars().count() > 256) {
        return Err(bad(cmd, "at most 512 labels, each key up to 128 bytes and text up to 256 characters"));
    }
    if a.title.as_ref().is_some_and(|t| t.chars().count() > 256) {
        return Err(bad(cmd, "the title is at most 256 characters"));
    }
    let setups: Vec<CamSetup> = match (&a.setup, &a.part) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `setup` or `part`, not both")),
        (Some(name), None) => vec![s.doc.cam.setup(name).cloned().ok_or_else(|| bad(cmd, format!("no setup `{name}`")))?],
        (None, Some(part)) => {
            let part = part.trim();
            let found: Vec<CamSetup> = s.doc.cam.setups.iter().filter(|x| x.part.as_deref().unwrap_or("") == part).cloned().collect();
            if found.is_empty() {
                return Err(bad(cmd, format!("no setup makes part `{part}`")));
            }
            found
        }
        (None, None) => s.doc.cam.setups.clone(),
    };
    let mut out = SheetResult { tools: Vec::new(), setups: Vec::new(), seconds: None, path: None, bytes: None };
    let mut used: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut all_timed = !setups.is_empty();
    let mut total = 0.0;
    for setup in &setups {
        for op in setup.operations.iter().filter(|op| op.uses_tool()) {
            used.entry(op.tool()).or_default().push(format!("{} / {}", setup.name, op.name()));
        }
        let row = setup_row(s, setup);
        match row.seconds {
            Some(t) => total += t,
            None => all_timed = false,
        }
        out.setups.push(row);
    }
    out.seconds = all_timed.then_some(total);
    for (number, steps) in used {
        let Some(tool) = s.doc.cam.tool(number) else {
            return Err(bad(cmd, format!("a step needs tool {number}, which the design lacks")));
        };
        out.tools.push(SheetTool {
            number: tool.number,
            kind: tool.kind.key().to_string(),
            description: tool.description.clone(),
            diameter: tool.diameter,
            flute_length: tool.flute_length,
            flutes: tool.flutes,
            corner_radius: tool.corner_radius,
            tip_angle: tool.tip_angle,
            tip_diameter: tool.tip_diameter,
            pitch: tool.pitch,
            shank_diameter: tool.shank_diameter,
            overall_length: tool.overall_length,
            stick_out: tool.stick_out,
            holder: tool.holder.as_ref().map(|h| SheetHolder { description: h.description.clone(), diameter: h.diameter, length: h.length }),
            used_in: steps,
        });
    }
    if let Some(path) = &a.path {
        solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
        let format = match a.format.as_deref() {
            Some(f) => f.to_ascii_lowercase(),
            None => std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase(),
        };
        let text = match format.as_str() {
            "html" | "htm" => html(&out, a.title.as_deref().unwrap_or("CAM"), &a.labels),
            "csv" => csv(&out),
            "json" => serde_json::to_string_pretty(&out).map_err(|e| bad(cmd, e.to_string()))?,
            other => return Err(bad(cmd, format!("format `{other}`: give html, csv or json"))),
        };
        if a.overwrite {
            solidcraft_io::write_atomic(std::path::Path::new(path), text.as_bytes(), true)
        } else {
            solidcraft_io::safe_write::create_atomic(std::path::Path::new(path), text.as_bytes())
        }
        .map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        out.path = Some(path.clone());
        out.bytes = Some(text.len());
    }
    serde_json::to_value(&out).map_err(|e| bad(cmd, e.to_string()))
}

/// One setup's sheet: its frame and stock as the model has them now, and its run time from its
/// program (none, with why, when it cannot be posted).
fn setup_row(s: &mut Session, setup: &CamSetup) -> SheetSetup {
    let mut problems = Vec::new();
    let (frame, live) = match cam::live_frame(s, setup) {
        Ok(f) => (Some(f.0), Some(f.1)),
        Err(e) => {
            problems.push(e);
            (None, None)
        }
    };
    let stock = match (&frame, &live) {
        (Some(frame), Some(live)) => match cam::sheet_stock(s, live, frame) {
            Ok((lo, hi)) => Some(SheetStock {
                kind: tag(&setup.stock_spec, "kind"),
                min: [lo.x, lo.y, lo.z],
                max: [hi.x, hi.y, hi.z],
                size: [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z],
            }),
            Err(e) => {
                problems.push(format!("stock: {e}"));
                None
            }
        },
        _ => None,
    };
    let post = cam::chosen_post(&s.doc.cam, setup).unwrap_or_else(|| cam::default_post(setup.operation_type).into());
    let skipped = |op: &CamOperation| setup.skipped.iter().any(|n| n == op.name());
    let mut per_step: BTreeMap<String, f64> = BTreeMap::new();
    let mut seconds = None;
    if setup.operations.iter().any(|op| !skipped(op)) {
        match cam::post_cmd(s, &json!({"setup": setup.name, "post": post})) {
            Ok(posted) => {
                let time = &posted["time"];
                seconds = time["total"].as_f64();
                if let (Some(names), Some(times)) = (time["operations"].as_array(), time["per_operation"].as_array()) {
                    for (n, t) in names.iter().zip(times) {
                        if let (Some(n), Some(t)) = (n.as_str(), t.as_f64()) {
                            per_step.insert(n.to_string(), t);
                        }
                    }
                }
                if seconds.is_none() {
                    problems.push("the run time could not be estimated".into());
                }
            }
            Err(e) => problems.push(e.to_string()),
        }
    } else {
        seconds = Some(0.0);
    }
    let live = live.as_ref().unwrap_or(setup);
    let machine = s.doc.cam.setup_machine(setup);
    SheetSetup {
        name: setup.name.clone(),
        part: setup.part.clone(),
        operation_type: setup.operation_type.key().to_string(),
        machine_kind: machine.map(|m| m.kind.key().to_string()),
        work_offset: format!("G{}", 53 + u32::from(setup.work_offset)),
        origin: live.work_origin,
        tool_axis: frame.as_ref().map_or(live.tool_axis, |f| [f.z.x, f.z.y, f.z.z]),
        x_axis: frame.as_ref().map(|f| [f.x.x, f.x.y, f.x.z]),
        part_zero: part_zero(live),
        material: setup.material.clone(),
        machine: setup.machine.clone(),
        post,
        fixtures: setup.fixtures.clone(),
        model_bodies: setup.model_bodies.clone(),
        stock,
        steps: setup
            .operations
            .iter()
            .map(|op| SheetStep {
                name: op.name().to_string(),
                kind: tag(op, "type"),
                tool: op.uses_tool().then(|| op.tool()),
                torch: torch_row(s, op),
                compensation: match op {
                    CamOperation::Contour { params, .. } if !params.compensation.is_cam() => {
                        serde_json::to_value(params.compensation).ok().and_then(|v| v.as_str().map(str::to_string))
                    }
                    _ => None,
                },
                skipped: skipped(op),
                seconds: per_step.get(op.name()).copied(),
            })
            .collect(),
        seconds,
        problem: (!problems.is_empty()).then(|| problems.join("; ")),
    }
}

/// A plasma cut's torch data as the design has them now; None for a step with a tool, or when an
/// expression does not evaluate (the setup's note says why).
fn torch_row(s: &Session, op: &CamOperation) -> Option<SheetTorch> {
    let CamOperation::Plasma { torch, cut, .. } = op else { return None };
    let len = |e: &solidcraft_doc::Expr| s.doc.eval(e.as_str(), solidcraft_doc::expr::Kind::Length).ok();
    Some(SheetTorch {
        kerf: len(&torch.kerf)?,
        pierce_height: len(&torch.pierce_height)?,
        cut_height: len(&torch.cut_height)?,
        pierce_delay: torch.pierce_delay,
        feed: len(&cut.feed)?,
    })
}

/// How part zero is found: the origin mode, and the box point it names.
fn part_zero(setup: &CamSetup) -> String {
    let mode = serde_json::to_value(setup.wcs_origin_mode).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    if !mode.ends_with("box_point") {
        return mode;
    }
    let point = serde_json::to_value(setup.box_point).unwrap_or(Value::Null);
    let axis = |a: &str| point[a].as_str().map(|v| format!("{a} {v}")).unwrap_or_default();
    let follows = if setup.zero_follows { ", follows the model" } else { "" };
    format!("{mode} ({}, {}, {}){follows}", axis("x"), axis("y"), axis("z"))
}

/// The serde tag `key` of a tagged value (an operation's `type`, a stock's `kind`).
fn tag<T: serde::Serialize>(value: &T, key: &str) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.get(key).and_then(Value::as_str).map(str::to_string)).unwrap_or_default()
}

fn mm(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn opt(v: Option<f64>) -> String {
    v.map(mm).unwrap_or_default()
}

fn clock(seconds: f64) -> String {
    let total = if seconds.is_finite() { seconds.max(0.0).round() as u64 } else { 0 };
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The tool list as CSV: one row a tool, fields quoted where they need it.
fn csv(sheet: &SheetResult) -> String {
    let field = |s: &str| {
        // A leading = + - @ would be a formula to a spreadsheet.
        let s = if s.starts_with(['=', '+', '-', '@']) && s.parse::<f64>().is_err() { format!("'{s}") } else { s.to_string() };
        if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let mut out = String::from(
        "number,kind,description,diameter,flute_length,flutes,corner_radius,tip_angle,tip_diameter,pitch,shank_diameter,overall_length,stick_out,holder,holder_diameter,holder_length,used_in\n",
    );
    for t in &sheet.tools {
        let row = [
            t.number.to_string(),
            t.kind.clone(),
            t.description.clone(),
            mm(t.diameter),
            mm(t.flute_length),
            t.flutes.to_string(),
            opt(t.corner_radius),
            opt(t.tip_angle),
            opt(t.tip_diameter),
            opt(t.pitch),
            opt(t.shank_diameter),
            opt(t.overall_length),
            opt(t.stick_out),
            t.holder.as_ref().map(|h| h.description.clone()).unwrap_or_default(),
            opt(t.holder.as_ref().map(|h| h.diameter)),
            opt(t.holder.as_ref().map(|h| h.length)),
            t.used_in.join("; "),
        ];
        out.push_str(&row.iter().map(|f| field(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// English headings, which `labels` replace key by key.
const HEADINGS: &[(&str, &str)] = &[
    ("axes", "Axes"),
    ("bodies", "Model bodies"),
    ("comp_control", "the control compensates the radius: D = measured diameter"),
    ("comp_wear", "the control compensates wear: D = measured minus nominal diameter"),
    ("description", "Description"),
    ("diameter", "Diameter"),
    ("fixtures", "Fixtures"),
    ("flute_length", "Flute length"),
    ("flutes", "Flutes"),
    ("heights", "Pierce / cut height"),
    ("holder", "Holder"),
    ("kerf", "Kerf"),
    ("kind", "Kind"),
    ("machine", "Machine"),
    ("material", "Material"),
    ("none", "none"),
    ("number", "T"),
    ("operation_type", "Machining"),
    ("origin", "Part zero (model)"),
    ("part", "Part"),
    ("part_zero", "Part zero"),
    ("post", "Post"),
    ("problem", "Note"),
    ("run_time", "Run time"),
    ("setups", "Setup sheets"),
    ("sizes", "Sizes"),
    ("skipped", "skipped"),
    ("step", "Step"),
    ("steps", "Steps"),
    ("stick_out", "Stick-out"),
    ("stock", "Stock (work coordinates)"),
    ("tool", "Tool"),
    ("tools", "Tool list"),
    ("total", "Total"),
    ("used_in", "Used in"),
    ("work_offset", "Work offset"),
];

/// The sheet as one printable HTML page, its own styles inline.
fn html(sheet: &SheetResult, title: &str, labels: &BTreeMap<String, String>) -> String {
    let l = |key: &str| -> String {
        labels
            .get(key)
            .cloned()
            .or_else(|| HEADINGS.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_string()))
            .unwrap_or_else(|| key.to_string())
    };
    let kind = |prefix: &str, key: &str| labels.get(&format!("{prefix}.{key}")).cloned().unwrap_or_else(|| key.replace('_', " "));
    let v3 = |v: [f64; 3]| format!("{} / {} / {}", mm(v[0]), mm(v[1]), mm(v[2]));
    let mut h = String::new();
    h.push_str("<!doctype html>\n<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str(&format!("<title>{}</title>", esc(title)));
    h.push_str(
        "<style>body{font:13px/1.4 system-ui,sans-serif;margin:16px;color:#111;background:#fff}h1{font-size:20px}h2{font-size:16px;margin-top:24px}\
         h3{font-size:14px;margin:20px 0 6px}table{border-collapse:collapse;width:100%;margin:6px 0}th,td{border:1px solid #999;padding:3px 6px;\
         text-align:left;vertical-align:top}th{background:#eee}td.n{text-align:right;white-space:nowrap}.note{color:#a00}.dim{color:#666}\
         section{break-inside:avoid}@media print{body{margin:0}section.setup{break-before:page}}</style></head><body>\n",
    );
    h.push_str(&format!("<h1>{}</h1>\n", esc(title)));
    // The tool list, when a step uses a tool (a plasma cut needs none).
    if !sheet.tools.is_empty() {
        h.push_str(&format!("<section><h2>{}</h2>\n<table><tr>", esc(&l("tools"))));
        for key in ["number", "kind", "description", "diameter", "flute_length", "flutes", "sizes", "stick_out", "holder", "used_in"] {
            h.push_str(&format!("<th>{}</th>", esc(&l(key))));
        }
        h.push_str("</tr>\n");
        for t in &sheet.tools {
            let mut sizes = Vec::new();
            for (label, value) in [
                ("R", t.corner_radius),
                ("∠", t.tip_angle.map(|a| a.round())),
                ("⌀tip", t.tip_diameter),
                ("P", t.pitch),
                ("⌀shank", t.shank_diameter),
                ("L", t.overall_length),
            ] {
                if let Some(v) = value {
                    sizes.push(format!("{label} {}", mm(v)));
                }
            }
            let holder = t.holder.as_ref().map_or(String::new(), |h| format!("{} ⌀{} × {}", h.description, mm(h.diameter), mm(h.length)));
            h.push_str(&format!(
            "<tr><td class=\"n\">T{}</td><td>{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td>{}</td><td class=\"n\">{}</td><td>{}</td><td>{}</td></tr>\n",
            t.number,
            esc(&kind("kind", &t.kind)),
            esc(&t.description),
            mm(t.diameter),
            mm(t.flute_length),
            t.flutes,
            esc(&sizes.join(", ")),
            opt(t.stick_out),
            esc(holder.trim()),
            t.used_in.iter().map(|u| esc(u)).collect::<Vec<_>>().join("<br>"),
        ));
        }
        h.push_str("</table></section>\n");
    }
    // A sheet per setup.
    h.push_str(&format!("<h2>{}</h2>\n", esc(&l("setups"))));
    for s in &sheet.setups {
        h.push_str(&format!("<section class=\"setup\"><h3>{}</h3>\n<table>", esc(&s.name)));
        let none = l("none");
        let list = |v: &[String]| if v.is_empty() { none.clone() } else { v.join(", ") };
        let mut rows: Vec<(String, String)> = Vec::new();
        if let Some(part) = &s.part {
            rows.push((l("part"), part.clone()));
        }
        rows.extend([
            (l("operation_type"), kind("type", &s.operation_type)),
            (l("work_offset"), s.work_offset.clone()),
            (l("part_zero"), s.part_zero.replace('_', " ")),
            (l("origin"), v3(s.origin)),
            (l("axes"), format!("Z {}{}", v3(s.tool_axis), s.x_axis.map(|x| format!(" · X {}", v3(x))).unwrap_or_default())),
            (l("material"), s.material.clone().unwrap_or_else(|| none.clone())),
            (
                l("stock"),
                s.stock.as_ref().map_or(none.clone(), |st| {
                    format!(
                        "{}: {} × {} × {} ({} … {})",
                        kind("stock", &st.kind),
                        mm(st.size[0]),
                        mm(st.size[1]),
                        mm(st.size[2]),
                        v3(st.min),
                        v3(st.max)
                    )
                }),
            ),
            (l("fixtures"), list(&s.fixtures)),
            (l("bodies"), list(&s.model_bodies)),
            (
                l("machine"),
                match (&s.machine, &s.machine_kind) {
                    (Some(m), Some(k)) => format!("{m} ({})", kind("machine", k)),
                    (Some(m), None) => m.clone(),
                    (None, _) => none.clone(),
                },
            ),
            (l("post"), s.post.clone()),
            (l("run_time"), s.seconds.map(clock).unwrap_or_else(|| "—".into())),
        ]);
        if let Some(p) = &s.problem {
            rows.push((l("problem"), p.clone()));
        }
        for (k, v) in rows {
            let class = if Some(&v) == s.problem.as_ref() { " class=\"note\"" } else { "" };
            h.push_str(&format!("<tr><th>{}</th><td{class}>{}</td></tr>", esc(&k), esc(&v)));
        }
        h.push_str("</table>\n<table><tr>");
        for key in ["step", "kind", "tool", "run_time"] {
            h.push_str(&format!("<th>{}</th>", esc(&l(key))));
        }
        h.push_str("</tr>\n");
        for (i, step) in s.steps.iter().enumerate() {
            let skipped = if step.skipped { format!(" <span class=\"dim\">({})</span>", esc(&l("skipped"))) } else { String::new() };
            h.push_str(&format!(
                "<tr><td>{}. {}{skipped}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>\n",
                i + 1,
                esc(&step.name),
                esc(&kind("op", &step.kind)),
                match (&step.tool, &step.torch) {
                    (Some(t), _) => match step.compensation.as_deref() {
                        Some(mode) => format!("T{t} <span class=\"dim\">D{t}: {}</span>", esc(&l(&format!("comp_{mode}")))),
                        None => format!("T{t}"),
                    },
                    (None, Some(t)) => esc(&format!(
                        "{} {} · {} {} / {} · {} s · {} mm/min",
                        l("kerf"),
                        mm(t.kerf),
                        l("heights"),
                        mm(t.pierce_height),
                        mm(t.cut_height),
                        mm(t.pierce_delay),
                        mm(t.feed)
                    )),
                    (None, None) => String::new(),
                },
                step.seconds.map(clock).unwrap_or_default(),
            ));
        }
        h.push_str("</table></section>\n");
    }
    if let Some(total) = sheet.seconds {
        h.push_str(&format!("<p><b>{}:</b> {}</p>\n", esc(&l("total")), clock(total)));
    }
    h.push_str("</body></html>\n");
    h
}
