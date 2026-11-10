use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use super::*;
use crate::lead::{lead_in, lead_out};
use crate::offset::Shape;
use crate::piece::Piece;

#[test]
fn peck_drilling_reaches_depth_retracts_and_reads_back_for_linuxcnc() {
    let tool = Tool::new(3, ToolKind::Drill, 4.0, 20.0);
    let cut = cut(0.0, -5.0, 2.0);
    let path = drill(&[v(10.0, 15.0), v(25.0, 15.0)], &tool, &cut, 0.25).unwrap();
    verify(&path, &cut.heights).unwrap();
    let depths: Vec<_> = path
        .moves
        .iter()
        .filter_map(|m| match m {
            Move::Feed { to, .. } => Some(to.z),
            _ => None,
        })
        .collect();
    assert_eq!(depths.len(), 6);
    close(depths[0], -5.0 / 3.0, 1e-9);
    close(depths[2], -5.0, 1e-9);
    assert_eq!(path.moves.iter().filter(|m| matches!(m, Move::Dwell { .. })).count(), 2);
    assert_eq!(path.moves.last().and_then(Move::target).unwrap().z, 10.0);
    let program = Program {
        name: "Drill test".into(),
        operations: vec![Operation { name: "Holes".into(), tool, spindle_rpm: 2400.0, coolant: true, path }],
        work_offset: Some(2),
    };
    let post = Post::builtin("linuxcnc").unwrap();
    let text = emit(&program, &post).unwrap();
    assert!(text.contains("G91.1 G61 G97"));
    assert!(text.contains("G55"));
    assert!(text.contains("G43 H3"));
    assert!(text.contains("G4 P0.25"));
    assert!(readback(&text, &program, &post).unwrap().within());
}

#[test]
fn canned_cycles_write_each_hole_as_one_block_and_read_back() {
    let tool = Tool::new(3, ToolKind::Drill, 4.0, 20.0);
    let holes = [v(10.0, 15.0), v(25.0, 15.0)];
    let program = |cut: &Cut, dwell: f64| {
        let path = drill(&holes, &tool, cut, dwell).unwrap();
        assert_eq!(path.cycles.len(), 2);
        Program {
            name: "Cycles".into(),
            operations: vec![Operation { name: "Holes".into(), tool: tool.clone(), spindle_rpm: 2400.0, coolant: false, path }],
            work_offset: None,
        }
    };
    let mut post = Post::linuxcnc();
    post.canned_cycles = true;
    let cases = [
        (cut(0.0, -5.0, 2.0), 0.0, "G98 G83 X10 Y15 Z-5 R2 Q2 F200"),
        (cut(0.0, -5.0, 10.0), 0.0, "G98 G81 X10 Y15 Z-5 R2 F200"),
        (cut(0.0, -5.0, 10.0), 0.5, "G98 G82 X10 Y15 Z-5 R2 P0.5 F200"),
    ];
    for (cut, dwell, block) in cases {
        let program = program(&cut, dwell);
        let text = emit(&program, &post).unwrap();
        assert!(text.contains(block), "{text}");
        assert_eq!(
            text.matches(
                "G80
"
            )
            .count(),
            2,
            "{text}"
        );
        assert!(
            !text.lines().any(|l| l.starts_with("G1 ")),
            "every hole is a cycle:
{text}"
        );
        let back = readback(&text, &program, &post).unwrap();
        assert!(back.within(), "{back:?}");
        // The cycle returns to the initial height (G98): the next hole is reached up there.
        let moves = parse(&text).unwrap();
        assert_eq!(moves.iter().filter_map(Move::target).map(|p| p.z).fold(f64::INFINITY, f64::min), -5.0);
        assert!(moves.windows(2).all(|w| match (w[0].target(), w[1].target()) {
            (Some(a), Some(b)) if Vec2::new(a.x, a.y).dist(Vec2::new(b.x, b.y)) > 1e-9 => a.z >= 10.0 - 1e-9 && b.z >= 10.0 - 1e-9,
            _ => true,
        }));
        // Without canned cycles the same program is written move by move, as before.
        let plain = emit(&program, &Post::linuxcnc()).unwrap();
        assert!(!plain.contains("G98") && plain.contains("G1 "), "{plain}");
    }
    // G83 has no dwell: a pecked hole with a dwell stays move by move.
    let text = emit(&program(&cut(0.0, -5.0, 2.0), 0.25), &post).unwrap();
    assert!(!text.contains("G83") && text.contains("G4 P0.25"), "{text}");
    // A post that counts dwells in milliseconds gets P in milliseconds.
    let mut ms = post.clone();
    ms.dwell = "G4 P{milliseconds}".into();
    assert!(emit(&program(&cut(0.0, -5.0, 10.0), 0.5), &ms).unwrap().contains("G82 X10 Y15 Z-5 R2 P500 F200"));
}

#[test]
fn g83_reads_back_as_pecks_from_r_with_full_retracts() {
    let moves = parse(
        "G21 G90
G0 Z10
G0 X5 Y5
G98 G83 X5 Y5 Z-5 R2 Q3 F100
G80
G0 X20
",
    )
    .unwrap();
    let zs: Vec<(f64, bool)> = moves.iter().filter_map(|m| m.target().map(|p| (p.z, m.feed().is_some()))).collect();
    let feeds: Vec<f64> = zs.iter().filter(|(_, f)| *f).map(|(z, _)| *z).collect();
    assert_eq!(feeds, vec![-1.0, -4.0, -5.0]);
    // Back to R after each peck, down to just above the last bottom, home to Z10 at the end.
    assert!(zs.contains(&(-1.0 + 0.254, false)) && zs.contains(&(-4.0 + 0.254, false)));
    assert_eq!(zs.iter().filter(|(z, f)| *z == 2.0 && !*f).count(), 4);
    assert_eq!(moves.last().and_then(Move::target), Some(Vec3::new(20.0, 5.0, 10.0)));
    // G99 stays at R; G81 and G82 feed once.
    let moves = parse(
        "G0 X0 Y0 Z10
G99 G82 Z-3 R1 P0.5 F50
G80
",
    )
    .unwrap();
    assert!(moves.iter().any(|m| matches!(m, Move::Dwell { seconds } if *seconds == 0.5)));
    assert_eq!(moves.last().and_then(Move::target).map(|p| p.z), Some(1.0));
    for bad in [
        "G0 X0 Y0 Z10
G83 Z-3 R1 F50
",
        "G0 X0 Y0 Z10
G81 Z-3 F50
",
        "G81 X0 Y0 Z-3 R1 F50
",
        "G0 X0 Y0 Z10
G81 Z3 R1 F50
",
        "G0 X0 Y0 Z10
G83 Z-1000000 R1 Q0.001 F50
",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn drill_cycles_that_do_not_match_their_moves_are_written_move_by_move() {
    let tool = Tool::new(3, ToolKind::Drill, 4.0, 20.0);
    let mut post = Post::linuxcnc();
    post.canned_cycles = true;
    let good = drill(&[v(10.0, 15.0)], &tool, &cut(0.0, -5.0, 2.0), 0.0).unwrap();
    let c = good.cycles[0];
    let bad = [
        DrillCycle { end: 10_000, ..c },
        DrillCycle { start: c.end, end: c.start, ..c },
        DrillCycle { at: v(11.0, 15.0), ..c },
        DrillCycle { bottom: -6.0, ..c },
        DrillCycle { retract: f64::NAN, ..c },
        DrillCycle { feed: 0.0, ..c },
        DrillCycle { peck: Some(-1.0), ..c },
        DrillCycle { start: 0, ..c },
    ];
    for cycle in bad {
        let path = Toolpath { moves: good.moves.clone(), cycles: vec![cycle, cycle], comp: Vec::new() };
        let program = Program {
            name: "x".into(),
            operations: vec![Operation { name: "o".into(), tool: tool.clone(), spindle_rpm: 2000.0, coolant: false, path }],
            work_offset: None,
        };
        let text = emit(&program, &post).unwrap();
        assert!(
            !text.contains("G98"),
            "{cycle:?}
{text}"
        );
        assert!(readback(&text, &program, &post).unwrap().within());
    }
}

#[test]
fn drilling_rejects_invalid_tools_depths_and_unbounded_work() {
    let drill_tool = Tool::new(1, ToolKind::Drill, 3.0, 10.0);
    assert!(drill(&[], &drill_tool, &cut(0.0, -5.0, 1.0), 0.0).is_err());
    assert!(drill(&[v(0.0, 0.0)], &tool(3.0), &cut(0.0, -5.0, 1.0), 0.0).is_err());
    assert!(drill(&[v(0.0, 0.0)], &drill_tool, &cut(0.0, -11.0, 1.0), 0.0).is_err());
    assert!(drill(&[v(f64::NAN, 0.0)], &drill_tool, &cut(0.0, -5.0, 1.0), 0.0).is_err());
    assert!(drill(&[v(0.0, 0.0)], &drill_tool, &cut(0.0, -5.0, 1e-12), 0.0).is_err());
    assert!(drill(&[v(0.0, 0.0)], &drill_tool, &cut(0.0, -5.0, 1.0), f64::INFINITY).is_err());
}

#[test]
fn arc_preview_is_bounded_before_allocation() {
    let path = Toolpath::new(vec![
        Move::Rapid { to: Vec3::new(100.0, 0.0, 0.0) },
        Move::Arc { to: Vec3::new(-100.0, 0.0, 0.0), center: v(0.0, 0.0), ccw: true, feed: 100.0 },
    ]);
    assert!(path.checked_points(0.001, 10).is_err());
    assert_eq!(path.checked_points(0.1, 1000).unwrap(), path.points(0.1));
}

#[test]
fn linuxcnc_post_refuses_rates_that_round_to_zero() {
    let mut program = drill_program();
    program.operations[0].spindle_rpm = 0.01;
    assert!(emit(&program, &Post::linuxcnc()).is_err());
    program.operations[0].spindle_rpm = 2000.0;
    for movement in &mut program.operations[0].path.moves {
        if let Move::Feed { feed, .. } | Move::Arc { feed, .. } = movement {
            *feed = 0.001;
        }
    }
    assert!(emit(&program, &Post::linuxcnc()).is_err());
}

#[test]
fn linuxcnc_establishes_coolant_state_and_restarts_it_after_a_tool_change() {
    let mut program = drill_program();
    let mut second = program.operations[0].clone();
    program.operations[0].coolant = true;
    second.tool.number = 2;
    second.coolant = true;
    program.operations = vec![program.operations[0].clone(), second];
    let text = emit(&program, &Post::linuxcnc()).unwrap();
    assert!(text.contains("M5 M9"));
    assert_eq!(text.lines().filter(|line| *line == "M8").count(), 2);
    assert!(text.contains("M9\nM5\nT2 M6"));
    assert_eq!(text.matches("G0 Z10\nG0 X10 Y10").count(), 2);
    assert!(readback(&text, &program, &Post::linuxcnc()).unwrap().within());
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn polygon(pts: &[(f64, f64)]) -> Region2 {
    Region2 { outer: Loop2::polygon(&pts.iter().map(|p| v(p.0, p.1)).collect::<Vec<_>>()), holes: vec![] }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn area(loops: &[Loop2]) -> f64 {
    loops.iter().map(Loop2::signed_area).sum()
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} is not within {tol} of {b}");
}

/// Every point of the offset loops is `d` from the boundary, and every loop is closed.
fn check_offset(regions: &[Region2], d: f64, loops: &[Loop2]) {
    let shape = Shape::from_regions(regions, 0.01).unwrap();
    for lp in loops {
        assert!(lp.is_closed(1e-3), "open loop");
        for p in lp.polyline(0.001) {
            close(shape.dist(p), d.abs(), 1e-3);
        }
    }
}

fn tool(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 20.0)
}

fn cut(top: f64, bottom: f64, stepdown: f64) -> Cut {
    Cut { heights: Heights { clearance: 10.0, retract: 2.0, top, bottom }, feeds: Feeds { cut: 600.0, plunge: 200.0 }, stepdown }
}

fn cutting_points(path: &Toolpath, below: f64) -> Vec<Vec3> {
    path.points(0.005).into_iter().filter(|(p, cutting)| *cutting && p.z < below).map(|(p, _)| p).collect()
}

/// The positions along the tool's side cuts (straight feeds that change no height), no more
/// than `s` apart: where the tool's disc swept the plane it was cutting.
fn feed_xy(path: &Toolpath, s: f64) -> Vec<Vec2> {
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        if let Move::Feed { to, .. } = *m
            && let Some(p) = pos
        {
            let (a, b) = (Vec2::new(p.x, p.y), Vec2::new(to.x, to.y));
            if a.dist(b) > 1e-9 && (p.z - to.z).abs() < 1e-9 {
                let n = (a.dist(b) / s).ceil().max(1.0) as usize;
                for i in 0..=n {
                    out.push(a.lerp(b, i as f64 / n as f64));
                }
            }
        }
        pos = m.target().or(pos);
    }
    out
}

/// Whether each of the given points lies within `d` (XY) of some sample.
fn all_covered(samples: &[Vec2], pts: &[(f64, f64)], d: f64) -> Vec<(f64, f64)> {
    pts.iter().filter(|(x, y)| !samples.iter().any(|q| q.dist(Vec2::new(*x, *y)) <= d)).copied().collect()
}

/// Distance in XY from `q` to the nearest cut made entirely below `below`.
fn dist_to_cuts(path: &Toolpath, below: f64, q: Vec2) -> f64 {
    let flat = |p: Vec3| Vec3::new(p.x, p.y, 0.0);
    let pts = path.points(0.005);
    pts.windows(2)
        .filter(|w| w[1].1 && w[0].0.z < below && w[1].0.z < below)
        .map(|w| q.extend(0.0).dist_to_segment(flat(w[0].0), flat(w[1].0)))
        .fold(f64::INFINITY, f64::min)
}

fn comp(mode: Compensation) -> CompParams {
    CompParams { mode, register: 1, lead: 0.0, overlap: 0.0 }
}

fn contour_params(side: Side) -> ContourParams {
    ContourParams { side, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }
}

fn one_op(path: Toolpath, tool: Tool) -> Program {
    Program { name: "Comp".into(), operations: vec![Operation { name: "Contour".into(), tool, spindle_rpm: 8000.0, coolant: false, path }], work_offset: None }
}

#[test]
fn a_contour_compensated_on_the_control_writes_the_contour_with_g41_and_g40_round_each_pass() {
    let part = [rect(0.0, 0.0, 40.0, 30.0)];
    let shape = Shape::from_regions(&part, 0.01).unwrap();
    let mill = tool(6.0);
    let c = cut(0.0, -4.0, 2.0);
    let path = contour_compensated(&part, &mill, &contour_params(Side::Outside), &comp(Compensation::Control), &c, None).unwrap();
    // One compensated run per depth pass, climb milling round the outside: the tool left (G41).
    assert_eq!(path.comp.len(), 2);
    assert!(path.comp.iter().all(|r| r.side == CompSide::Left && r.register == 1));
    // The tool centre stays a radius off the part, and touches it all round.
    let below = cutting_points(&path, -1.0);
    assert!(below.iter().all(|p| shape.dist(Vec2::new(p.x, p.y)) >= 3.0 - 1e-6));
    for q in [v(20.0, -3.0), v(43.0, 15.0), v(20.0, 33.0), v(-3.0, 15.0)] {
        assert!(dist_to_cuts(&path, -1.0, q) < 1e-3, "{q:?}");
    }
    // The program carries the contour itself, entered and left on a line 1.25 diameters long
    // square to the longest side's middle.
    for run in &path.comp {
        let Some(Move::Feed { to: on, .. }) = run.programmed.first() else { panic!("an entry line") };
        let Some(Move::Feed { to: off, .. }) = run.programmed.last() else { panic!("an exit line") };
        close(on.x, 20.0, 1e-9);
        close(on.y, 0.0, 1e-9);
        close(off.x, 20.0, 1e-9);
        close(off.y, -7.5, 1e-9);
        let contour = &run.programmed[1..run.programmed.len() - 1];
        assert!(contour.iter().filter_map(Move::target).all(|p| shape.dist(Vec2::new(p.x, p.y)) < 1e-9));
        // Down to the pass depth at the entry point, before compensation goes on.
        assert!(matches!(path.moves.get(run.start - 1), Some(Move::Feed { to, .. }) if (to.x - 20.0).abs() < 1e-9 && (to.y + 7.5).abs() < 1e-9));
    }
    let program = one_op(path.clone(), mill.clone());
    for (post, on, off) in [(Post::linuxcnc(), "G41 D1 G1 Y0", "G40 G1 Y-7.5"), (Post::fanuc(), "G41 D1 G01 Y0.", "G40 G01 Y-7.5"), (Post::sinumerik(), "G41 G1 Y0", "G40 G1 Y-7.5")] {
        let text = emit(&program, &post).unwrap();
        assert_eq!(text.matches(on).count(), 2, "{}: {text}", post.name);
        assert_eq!(text.matches(off).count(), 2, "{}: {text}", post.name);
        assert!(readback(&text, &program, &post).unwrap().within(), "{}", post.name);
        // Between switching on and off only cutting moves in the plane: no Z, no M words.
        for block in text.split("G41").skip(1) {
            let inside = block.split("G40").next().unwrap_or_default();
            assert!(!inside.contains('Z') && !inside.contains('M') && !inside.contains("G0 ") && !inside.contains("G00"), "{inside}");
        }
    }
    // Heidenhain: RL along the contour, R0 on the exit line.
    let text = emit(&program, &Post::heidenhain()).unwrap();
    assert!(text.contains("L Y+0 RL") && text.contains("L Y-7.5 R0"), "{text}");
    assert!(readback(&text, &program, &Post::heidenhain()).unwrap().within());
    // GRBL has no compensation: refused, saying what to do instead.
    let e = emit(&program, &Post::grbl()).unwrap_err().to_string();
    assert!(e.contains("writes no cutter compensation"), "{e}");
    // Conventional milling puts the tool on the right (G42).
    let conventional = ContourParams { climb: false, ..contour_params(Side::Outside) };
    let path = contour_compensated(&part, &mill, &conventional, &comp(Compensation::Control), &c, None).unwrap();
    assert!(path.comp.iter().all(|r| r.side == CompSide::Right));
    // Inside an opening the corners are cut back and the entry stays inside.
    let path = contour_compensated(&part, &mill, &contour_params(Side::Inside), &comp(Compensation::Control), &c, None).unwrap();
    assert!(path.comp.iter().all(|r| r.side == CompSide::Left));
    let below = cutting_points(&path, -1.0);
    assert!(below.iter().all(|p| shape.clear(Vec2::new(p.x, p.y), 3.0 - 1e-6)));
    assert!(below.iter().any(|p| (p.x - 3.0).abs() < 1e-6 && (p.y - 3.0).abs() < 1e-6), "the corner is cut to the tool");
    let text = emit(&one_op(path.clone(), mill.clone()), &Post::linuxcnc()).unwrap();
    assert!(readback(&text, &one_op(path, mill.clone()), &Post::linuxcnc()).unwrap().within());
}

#[test]
fn wear_compensation_writes_the_nominal_centre_path_and_stock_and_overlap_are_kept() {
    let part = [rect(0.0, 0.0, 40.0, 30.0)];
    let shape = Shape::from_regions(&part, 0.01).unwrap();
    let mill = tool(6.0);
    let c = cut(0.0, -2.0, 2.0);
    let path = contour_compensated(&part, &mill, &contour_params(Side::Outside), &comp(Compensation::Wear), &c, None).unwrap();
    let run = &path.comp[0];
    // The program is the tool centre's own path: what the CAM would write, with G41 round it.
    assert_eq!(run.programmed.as_slice(), &path.moves[run.start..run.end]);
    let contour = &run.programmed[1..run.programmed.len() - 1];
    assert!(contour.iter().filter_map(Move::target).all(|p| (shape.dist(Vec2::new(p.x, p.y)) - 3.0).abs() < 1e-6));
    let text = emit(&one_op(path.clone(), mill.clone()), &Post::linuxcnc()).unwrap();
    assert!(text.contains("G41 D1 G1"));
    // Stock on the wall: the program is the contour moved out by it, the tool a radius beyond.
    let leave = ContourParams { stock_to_leave: 0.5, ..contour_params(Side::Outside) };
    let path = contour_compensated(&part, &mill, &leave, &CompParams { overlap: 5.0, lead: 10.0, ..comp(Compensation::Control) }, &c, None).unwrap();
    let run = &path.comp[0];
    let contour = &run.programmed[1..run.programmed.len() - 1];
    assert!(contour.iter().filter_map(Move::target).all(|p| (shape.dist(Vec2::new(p.x, p.y)) - 0.5).abs() < 1e-6));
    assert!(cutting_points(&path, -1.0).iter().all(|p| shape.dist(Vec2::new(p.x, p.y)) >= 3.5 - 1e-6));
    // The longer lead: 3 + 10 mm off the contour; the overlap runs 5 mm past the start.
    let (Some(Move::Feed { to: on, .. }), Some(Move::Feed { to: off, .. })) = (run.programmed.first(), run.programmed.last()) else { panic!("entry and exit lines") };
    close(shape.dist(Vec2::new(off.x, off.y)), 13.5, 1e-6);
    let last_on_contour = contour.last().and_then(Move::target).unwrap();
    close(Vec2::new(on.x, on.y).dist(Vec2::new(last_on_contour.x, last_on_contour.y)), 5.0, 1e-6);
    let text = emit(&one_op(path.clone(), mill.clone()), &Post::linuxcnc()).unwrap();
    assert!(readback(&text, &one_op(path, mill), &Post::linuxcnc()).unwrap().within());
}

#[test]
fn compensation_the_control_could_not_make_is_refused_before_anything_is_written() {
    let mill = tool(6.0);
    let c = cut(0.0, -2.0, 2.0);
    let control = comp(Compensation::Control);
    let outside = contour_params(Side::Outside);
    // A notch of radius 2 in the top edge: the CAM's tool bridges it, the control would gouge.
    let notch = |radius: f64| Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: v(0.0, 0.0), b: v(40.0, 0.0) },
                Seg2::Line { a: v(40.0, 0.0), b: v(40.0, 30.0) },
                Seg2::Line { a: v(40.0, 30.0), b: v(20.0 + radius, 30.0) },
                Seg2::Arc { center: v(20.0, 30.0), radius, start: 0.0, sweep: -PI },
                Seg2::Line { a: v(20.0 - radius, 30.0), b: v(0.0, 30.0) },
                Seg2::Line { a: v(0.0, 30.0), b: v(0.0, 0.0) },
            ],
        },
        holes: vec![],
    };
    let e = contour_compensated(&[notch(2.0)], &mill, &outside, &control, &c, None).unwrap_err().to_string();
    assert!(e.contains("tighter than the tool") && e.contains("(20.000, 28.000)"), "{e}");
    // A notch the tool fits: the control's offset follows the CAM's.
    let path = contour_compensated(&[notch(4.0)], &mill, &outside, &control, &c, None).unwrap();
    assert!(dist_to_cuts(&path, -1.0, v(20.0, 29.0)) < 1e-3, "down into the notch");
    // Two parts closer together than the tool: the CAM's tool goes round both at once.
    let pair = [rect(0.0, 0.0, 20.0, 20.0), rect(22.0, 0.0, 42.0, 20.0)];
    let e = contour_compensated(&pair, &mill, &outside, &control, &c, None).unwrap_err().to_string();
    assert!(e.contains("closer together than the tool"), "{e}");
    // An opening too small for the entry line square to its wall.
    let e = contour_compensated(&[rect(0.0, 0.0, 40.0, 10.0)], &mill, &contour_params(Side::Inside), &control, &c, None).unwrap_err().to_string();
    assert!(e.contains("no room"), "{e}");
    // Ramps, contours on the line, bad registers and the CAM's own mode are refused.
    let part = [rect(0.0, 0.0, 40.0, 30.0)];
    for (params, comp, says) in [
        (ContourParams { ramp_angle: 0.1, ..outside }, control, "ramping"),
        (contour_params(Side::On), control, "no side"),
        (outside, CompParams { register: 0, ..control }, "register"),
        (outside, CompParams { lead: f64::NAN, ..control }, "entry and exit"),
        (outside, comp(Compensation::Cam), "nothing for the control"),
    ] {
        let e = contour_compensated(&part, &mill, &params, &comp, &c, None).unwrap_err().to_string();
        assert!(e.contains(says), "{says}: {e}");
    }
    // A start point near a corner is moved a tool diameter along the side.
    let path = contour_compensated(&part, &mill, &outside, &control, &c, Some(v(39.0, -1.0))).unwrap();
    let Some(Move::Feed { to, .. }) = path.comp[0].programmed.first() else { panic!("an entry line") };
    close(to.x, 34.0, 1e-9);
    close(to.y, 0.0, 1e-9);
}

#[test]
fn compensated_runs_follow_their_moves_when_paths_are_joined() {
    let part = [rect(0.0, 0.0, 40.0, 30.0)];
    let mill = tool(6.0);
    let c = cut(0.0, -2.0, 2.0);
    let one = contour_compensated(&part, &mill, &contour_params(Side::Outside), &comp(Compensation::Control), &c, None).unwrap();
    let mut joined = Toolpath::new(vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) }]);
    joined.append(one.clone());
    joined.append(one.clone());
    assert_eq!(joined.comp.len(), 2);
    assert_eq!(joined.comp[0].start, one.comp[0].start + 1);
    assert_eq!(joined.comp[1].start, one.comp[0].start + 1 + one.moves.len());
    // The programmed moves replace the tool centre's run by run.
    let programmed = joined.programmed();
    let lengths = |p: &Toolpath| p.comp.iter().map(|r| r.programmed.len() as i64 - (r.end - r.start) as i64).sum::<i64>();
    assert_eq!(programmed.len() as i64, joined.moves.len() as i64 + lengths(&joined));
    assert_eq!(Toolpath::new(joined.moves.clone()).programmed(), joined.moves);
    // A run that no longer fits its moves is refused when posting.
    let mut broken = one.clone();
    broken.comp[0].end = broken.moves.len() + 5;
    let e = emit(&one_op(broken, mill), &Post::linuxcnc()).unwrap_err().to_string();
    assert!(e.contains("not written as one cut"), "{e}");
}

#[test]
fn square_offsets_to_a_smaller_square_and_to_a_rounded_larger_one() {
    let r = [rect(0.0, 0.0, 40.0, 30.0)];
    let inner = offset_regions(&r, 5.0, 0.01).unwrap();
    assert_eq!(inner.len(), 1);
    close(area(&inner), 30.0 * 20.0, 1e-6);
    check_offset(&r, 5.0, &inner);
    // Grown by d: area + perimeter × d + π d² (four quarter circles at the corners).
    let outer = offset_regions(&r, -5.0, 0.01).unwrap();
    assert_eq!(outer.len(), 1);
    close(area(&outer), 1200.0 + 140.0 * 5.0 + PI * 25.0, 1e-6);
    check_offset(&r, 5.0, &outer);
}

#[test]
fn inside_corner_of_an_l_shape_is_rounded() {
    // Two 30×10 arms that share a 10×10 square, plus the part of the 5×5 square at the inside
    // corner that is at least 5 from the corner.
    let l = [polygon(&[(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (20.0, 20.0), (20.0, 40.0), (0.0, 40.0)])];
    let inner = offset_regions(&l, 5.0, 0.01).unwrap();
    assert_eq!(inner.len(), 1);
    close(area(&inner), 500.0 + 25.0 - 25.0 * PI / 4.0, 1e-6);
    check_offset(&l, 5.0, &inner);
}

#[test]
fn circles_and_holes_offset_exactly() {
    let disc = [Region2 { outer: Loop2::circle(v(3.0, -2.0), 10.0), holes: vec![] }];
    close(area(&offset_regions(&disc, 3.0, 0.01).unwrap()), 49.0 * PI, 1e-6);
    close(area(&offset_regions(&disc, -3.0, 0.01).unwrap()), 169.0 * PI, 1e-6);
    // A hole grows as its region shrinks.
    let plate = [Region2 { outer: rect(0.0, 0.0, 40.0, 40.0).outer, holes: vec![Loop2::circle(v(20.0, 20.0), 5.0).reversed()] }];
    let inner = offset_regions(&plate, 3.0, 0.01).unwrap();
    assert_eq!(inner.len(), 2);
    close(area(&inner), 34.0 * 34.0 - 64.0 * PI, 1e-6);
    check_offset(&plate, 3.0, &inner);
    // Hole loops given counter-clockwise are turned round.
    let ccw_hole = [Region2 { outer: rect(0.0, 0.0, 40.0, 40.0).outer, holes: vec![Loop2::circle(v(20.0, 20.0), 5.0)] }];
    close(area(&offset_regions(&ccw_hole, 3.0, 0.01).unwrap()), 34.0 * 34.0 - 64.0 * PI, 1e-6);
}

#[test]
fn narrow_neck_splits_the_offset_and_a_small_region_vanishes() {
    let dumbbell = [polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 8.0),
        (30.0, 8.0),
        (30.0, 0.0),
        (50.0, 0.0),
        (50.0, 20.0),
        (30.0, 20.0),
        (30.0, 12.0),
        (20.0, 12.0),
        (20.0, 20.0),
        (0.0, 20.0),
    ])];
    let inner = offset_regions(&dumbbell, 3.0, 0.01).unwrap();
    assert_eq!(inner.len(), 2, "the 4 mm neck is too narrow for a 3 mm offset");
    check_offset(&dumbbell, 3.0, &inner);
    for lp in &inner {
        // A 14×14 square plus a small bulge towards the neck.
        let a = lp.signed_area();
        assert!(a > 196.0 && a < 200.0, "{a}");
    }
    // At 1.5 the neck stays open.
    assert_eq!(offset_regions(&dumbbell, 1.5, 0.01).unwrap().len(), 1);
    assert!(offset_regions(&[rect(0.0, 0.0, 10.0, 10.0)], 6.0, 0.01).unwrap().is_empty());
}

#[test]
fn polygon_with_many_shallow_corners_offsets_to_one_loop() {
    // A regular 720-gon: neighbouring sides turn by half a degree.
    let (n, radius) = (720usize, 25.0);
    let pts: Vec<(f64, f64)> =
        (0..n).map(|i| (radius * (i as f64 * 2.0 * PI / n as f64).cos(), radius * (i as f64 * 2.0 * PI / n as f64).sin())).collect();
    let gon = [polygon(&pts)];
    let half = PI / n as f64;
    let (apothem, perimeter) = (radius * half.cos(), 2.0 * n as f64 * radius * half.sin());
    let inner = offset_regions(&gon, 4.0, 0.01).unwrap();
    assert_eq!(inner.len(), 1);
    close(area(&inner), n as f64 * (apothem - 4.0).powi(2) * half.tan(), 1e-3);
    let outer = offset_regions(&gon, -4.0, 0.01).unwrap();
    assert_eq!(outer.len(), 1);
    close(area(&outer), n as f64 * apothem.powi(2) * half.tan() + perimeter * 4.0 + PI * 16.0, 1e-3);
}

#[test]
fn pocket_stays_inside_the_walls_and_clears_everything_the_tool_can_reach() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let params = PocketParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -5.0, 2.0)).unwrap();
    let stats = path.stats();
    assert_eq!(stats.lowest_z, Some(-5.0));
    // Three passes of 5/3 mm.
    let mut levels: Vec<i64> = cutting_points(&path, 0.0).iter().map(|p| (p.z * 1e6).round() as i64).collect();
    levels.sort();
    levels.dedup();
    assert!(levels.len() >= 3 && levels.contains(&-5_000_000) && levels.contains(&-1_666_667), "{levels:?}");
    for p in cutting_points(&path, 0.0) {
        assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "gouge at {p:?}");
    }
    for i in 0..=34 {
        for j in 0..=24 {
            let q = v(3.0 + i as f64, 3.0 + j as f64);
            let d = dist_to_cuts(&path, -5.0 + 1e-6, q);
            assert!(d <= 3.0 + 0.01, "{q:?} is {d} from the nearest pass");
        }
    }
}

#[test]
fn pocket_goes_round_an_island() {
    let island = Loop2::circle(v(30.0, 20.0), 5.0);
    let regions = [Region2 { outer: rect(0.0, 0.0, 60.0, 40.0).outer, holes: vec![island] }];
    let params = PocketParams { stepover: 3.0, climb: false, stock_to_leave: 0.5, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let pts = cutting_points(&path, 0.0);
    for p in &pts {
        // Tool radius 3 plus 0.5 left on the walls.
        assert!(shape.clear(v(p.x, p.y), 3.5 - 1e-4), "gouge at {p:?}");
    }
    for i in 0..=60 {
        for j in 0..=40 {
            let q = v(i as f64, j as f64);
            if shape.clear(q, 3.5 + 1e-3) {
                let d = dist_to_cuts(&path, 0.0, q);
                assert!(d <= 3.0 + 0.01, "{q:?} is {d} from the nearest pass");
            }
        }
    }
}

#[test]
fn contour_runs_round_the_part_with_the_wall_on_the_chosen_side() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 3.0);
    let run = |climb: bool| {
        let params = ContourParams { side: Side::Outside, climb, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
        contour(&regions, &tool(6.0), &params, &c).unwrap()
    };
    let climb = run(true);
    // Two passes round the rounded rectangle 3 mm outside the part, and 8 mm of plunging
    // (retract 2 → −3 → −6).
    close(climb.stats().cut_length, 2.0 * (140.0 + 6.0 * PI) + 8.0, 1e-6);
    close(climb.stats().cut_minutes, 2.0 * (140.0 + 6.0 * PI) / 600.0 + 8.0 / 200.0, 1e-9);
    let turns = |path: &Toolpath| {
        let pts = cutting_points(path, -5.9);
        pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - a.y * b.x).sum::<f64>()
    };
    // Climb round the outside of a part is clockwise (wall on the right of travel).
    assert!(turns(&climb) < 0.0);
    assert!(turns(&run(false)) > 0.0);
    for p in cutting_points(&climb, 0.0) {
        let outside = (p.x.clamp(0.0, 40.0) - p.x).hypot(p.y.clamp(0.0, 30.0) - p.y);
        close(outside, 3.0, 1e-6);
    }
    // Inside an opening the tool must fit.
    let inside = ContourParams { side: Side::Inside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(contour(&[rect(0.0, 0.0, 5.0, 5.0)], &tool(6.0), &inside, &c).is_err());
    let inner = contour(&regions, &tool(6.0), &inside, &c).unwrap();
    close(inner.stats().cut_length, 2.0 * 2.0 * (34.0 + 24.0) + 8.0, 1e-6);
    // Climb inside an opening is counter-clockwise.
    assert!(turns(&inner) > 0.0);
}

/// The steepest descent of a cutting move (radians), and whether any goes straight down below `top`.
fn steepest(path: &Toolpath, top: f64) -> (f64, bool) {
    let pts = path.points(0.002);
    let (mut angle, mut plunges) = (0.0f64, false);
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        if !w[1].1 || b.z >= a.z - 1e-9 || a.z > top + 1e-9 {
            continue;
        }
        let run = (b.x - a.x).hypot(b.y - a.y);
        plunges |= run < 1e-9;
        angle = angle.max((a.z - b.z).atan2(run));
    }
    (angle, plunges)
}

#[test]
fn ramps_replace_plunges_into_material() {
    let ramp = 3.0f64.to_radians();
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    // Round the part: each pass ramps once round the 140 + 6π loop, then cuts it flat.
    let c = cut(0.0, -6.0, 3.0);
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: ramp };
    let path = contour(&regions, &tool(6.0), &params, &c).unwrap();
    let lap = 140.0 + 6.0 * PI;
    close(path.stats().cut_length, 2.0 * (lap.hypot(3.0) + lap) + 2.0, 1e-6);
    let (angle, plunges) = steepest(&path, 0.0);
    assert!(!plunges && angle <= ramp + 1e-6 && angle > 0.01, "{angle} {plunges}");
    assert_eq!(path.stats().lowest_z, Some(-6.0));

    // In a pocket the first ring is short, so the ramp goes round it several times.
    let params = PocketParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: ramp, entry: None, adaptive: false };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -5.0, 2.5)).unwrap();
    let (angle, plunges) = steepest(&path, 0.0);
    assert!(!plunges && angle <= ramp + 1e-6 && angle > 0.01, "{angle} {plunges}");
    assert_eq!(path.stats().lowest_z, Some(-5.0));
    for p in cutting_points(&path, 0.0) {
        assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "gouge at {p:?}");
    }
    // The floor is still cleared: the flat pass follows the ramp.
    for i in 0..=34 {
        for j in 0..=24 {
            let q = v(3.0 + i as f64, 3.0 + j as f64);
            assert!(dist_to_cuts(&path, -5.0 + 1e-6, q) <= 3.0 + 0.01, "{q:?}");
        }
    }
    // A ring too short to ramp along is plunged into rather than refused.
    let tight = pocket(&[rect(0.0, 0.0, 6.004, 6.004)], &tool(6.0), &params, &cut(0.0, -5.0, 2.5)).unwrap();
    assert_eq!(tight.stats().lowest_z, Some(-5.0));
    assert!(steepest(&tight, 0.0).1);
    assert!(pocket(&regions, &tool(6.0), &PocketParams { ramp_angle: 2.0, entry: None, ..params }, &c).is_err());
}

#[test]
fn pocket_with_passes_up_to_a_diameter_apart_still_clears_the_middle() {
    // A 40 × 30 pocket, a 6 mm tool, passes 5 apart: rings 3, 8 and 13 from the walls. The last
    // leaves a strip down the middle (15 from the long walls) more than a radius from it.
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    for (stepover, name) in [(5.0, "wide"), (6.0, "a diameter"), (4.0, "moderate")] {
        let params = PocketParams { stepover, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
        let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
        for p in cutting_points(&path, 0.0) {
            assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "{name}: gouge at {p:?}");
        }
        for i in 0..=68 {
            for j in 0..=48 {
                let q = v(3.0 + i as f64 * 0.5, 3.0 + j as f64 * 0.5);
                let d = dist_to_cuts(&path, -2.0 + 1e-6, q);
                assert!(d <= 3.0 + 0.01, "{name}: {q:?} is {d} from the nearest pass");
            }
        }
    }
    // Two rooms joined by a wide door, one room narrower: each is cleared to its own middle, round an island too.
    let rooms = Region2 {
        outer: polygon(&[(0.0, 0.0), (30.0, 0.0), (30.0, 8.0), (60.0, 8.0), (60.0, 26.0), (30.0, 26.0), (30.0, 40.0), (0.0, 40.0)]).outer,
        holes: vec![Loop2::circle(v(15.0, 20.0), 4.0)],
    };
    let params = PocketParams { stepover: 5.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
    let path = pocket(std::slice::from_ref(&rooms), &tool(6.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
    let shape = Shape::from_regions(std::slice::from_ref(&rooms), 0.01).unwrap();
    for p in cutting_points(&path, 0.0) {
        assert!(shape.clear(v(p.x, p.y), 3.0 - 1e-4), "gouge at {p:?}");
    }
    for i in 0..=120 {
        for j in 0..=80 {
            let q = v(i as f64 * 0.5, j as f64 * 0.5);
            if shape.clear(q, 3.0 + 1e-3) {
                let d = dist_to_cuts(&path, -2.0 + 1e-6, q);
                assert!(d <= 3.0 + 0.01, "{q:?} is {d} from the nearest pass");
            }
        }
    }
    let too_wide = PocketParams { stepover: 6.5, entry: None, ..params };
    assert!(pocket(&regions, &tool(6.0), &too_wide, &cut(0.0, -2.0, 2.0)).is_err(), "passes more than a diameter apart");
}

#[test]
fn open_pocket_enters_through_the_air_and_feeds_one_way() {
    // A 20 × 10 pocket open on its left side, entered from the lower left along one line.
    // The passes nest against the closed wall, deepest first; every one descends in the
    // air outside the opening, and the level feeds as one unbroken path because the way
    // from a pass's end to the next pass's beginning crosses the mouth in the air.
    let regions = [rect(0.0, 0.0, 20.0, 10.0)];
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let entry = v(-12.0, -2.0);
    let params =
        PocketParams { stepover: 0.4, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: Some(entry), adaptive: false };
    let c = cut(0.0, -3.0, 1.5);
    let path = pocket(&regions, &tool(1.0), &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let d = (v(10.0, 5.0) - entry).normalized().unwrap();
    // The closed walls; the tool's centre never comes nearer to them than its own radius.
    let walls = [
        Piece::Line { a: v(20.0, 0.0), b: v(20.0, 10.0) },
        Piece::Line { a: v(0.0, 10.0), b: v(20.0, 10.0) },
        Piece::Line { a: v(0.0, 0.0), b: v(20.0, 0.0) },
    ];
    let mut pos = v(0.0, 0.0);
    let (mut side, mut down, mut air_entries) = (0usize, 0usize, 0usize);
    // The feeds made so far and the height each ran at: a feed that runs against the
    // entry direction is a pass turning back along a wall or a link crossing the mouth,
    // allowed only where the disc stands over ground those feeds have already cut.
    let mut feeds: Vec<(Vec2, Vec2, f64)> = Vec::new();
    for m in &path.moves {
        if let Move::Rapid { to } = *m {
            pos = v(to.x, to.y);
        }
        if let Move::Feed { to, .. } = *m {
            let q = v(to.x, to.y);
            for w in &walls {
                assert!(w.dist(q) >= 0.5 - 1e-6, "the disc touches a closed wall at ({}, {})", to.x, to.y);
            }
            if q.dist(pos) < 1e-9 {
                down += 1;
                // A level goes down in the air in front of the mouth; nothing stands
                // under the tool on the way there.
                if q.x <= 1e-6 || shape.dist(q) >= 0.5 - 1e-6 {
                    air_entries += 1;
                }
            } else {
                side += 1;
                if (q - pos).normalized().unwrap().dot(d) <= 1e-6 {
                    // A pass never feeds against the entry with metal beside it, so this
                    // is a turn or a crossing: check the disc runs over ground already
                    // cut at this very height, within its own radius of a feed there.
                    for i in 0..=4 {
                        let p = pos.lerp(q, i as f64 / 4.0);
                        assert!(
                            feeds
                                .iter()
                                .any(|(a, b, fz)| (*fz - to.z).abs() <= 1e-9
                                    && p.extend(0.0).dist_to_segment(a.extend(0.0), b.extend(0.0)) <= 0.5 - 1e-6),
                            "a feed runs the other way, off the cut floor: ({}, {}) to ({}, {})",
                            pos.x,
                            pos.y,
                            to.x,
                            to.y
                        );
                    }
                }
                feeds.push((pos, q, to.z));
            }
            pos = q;
        }
    }
    assert!(side > 50 && down > 0, "{side} side feeds and {down} plunges");
    // One chain per depth: the whole level feeds at one height, and the tool's only
    // plunges are the level's own.
    assert_eq!(down, 2, "{down} plunges for {side} side feeds");
    assert!(air_entries * 10 >= down * 9, "{air_entries} of {down} passes enter through the air");
    // Both depth passes reach the floor and it is cleared wherever the tool can reach.
    assert_eq!(path.stats().lowest_z, Some(-3.0));
    let cuts = cutting_points(&path, -3.0 + 1e-6);
    assert!(cuts.iter().any(|p| (p.z + 3.0).abs() < 1e-9));
    for i in 0..=40 {
        for j in 0..=20 {
            let q = v(i as f64 * 0.5, j as f64 * 0.5);
            if shape.clear(q, 0.5 + 1e-3) {
                let dd = dist_to_cuts(&path, -3.0 + 1e-6, q);
                assert!(dd <= 0.5 + 0.01, "{q:?} is {dd} from the nearest pass");
            }
        }
    }
}

#[test]
fn open_pocket_leaves_the_stock_on_the_closed_walls() {
    // The same pocket with 0.25 mm left on the walls: the opening is still cut flush.
    let regions = [rect(0.0, 0.0, 20.0, 10.0)];
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let entry = v(-12.0, -2.0);
    let params =
        PocketParams { stepover: 0.4, climb: true, stock_to_leave: 0.25, tolerance: 0.01, ramp_angle: 0.0, entry: Some(entry), adaptive: false };
    let c = cut(0.0, -2.0, 2.0);
    let path = pocket(&regions, &tool(1.0), &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let walls = [
        Piece::Line { a: v(20.0, 0.0), b: v(20.0, 10.0) },
        Piece::Line { a: v(0.0, 10.0), b: v(20.0, 10.0) },
        Piece::Line { a: v(0.0, 0.0), b: v(20.0, 0.0) },
    ];
    for p in cutting_points(&path, 0.0) {
        for w in &walls {
            assert!(w.dist(v(p.x, p.y)) >= 0.75 - 1e-6, "the stock on a closed wall is broken at ({}, {})", p.x, p.y);
        }
    }
    // Every pass begins on a lead-in outside the opening's plane, and the wall's own pass
    // ends with its disc crossing that plane: the opening is cut flush, with no ledge
    // standing at its face.
    assert!(cutting_points(&path, 0.0).iter().any(|p| p.x < -1e-6), "no pass crosses the opening's plane");
    // Everything deeper than the stock is cleared.
    for i in 0..=40 {
        for j in 0..=20 {
            let q = v(i as f64 * 0.5, j as f64 * 0.5);
            if shape.clear(q, 0.75 + 0.5 + 1e-3) {
                let dd = dist_to_cuts(&path, -2.0 + 1e-6, q);
                assert!(dd <= 0.5 + 0.01, "{q:?} is {dd} from the nearest pass");
            }
        }
    }
}

#[test]
fn open_pocket_refuses_an_entry_that_is_not_air_or_not_facing() {
    let regions = [rect(0.0, 0.0, 20.0, 10.0)];
    let ok = PocketParams {
        stepover: 0.4,
        climb: true,
        stock_to_leave: 0.0,
        tolerance: 0.01,
        ramp_angle: 0.0,
        entry: Some(v(-12.0, -2.0)),
        adaptive: false,
    };
    // In air, but not clear of the tool's radius.
    let near = PocketParams { entry: Some(v(-0.3, 5.0)), ..ok };
    let err = pocket(&regions, &tool(1.0), &near, &cut(0.0, -2.0, 2.0)).unwrap_err().to_string();
    assert!(err.contains("in air outside"), "{err}");
    // Inside the material.
    let inside = PocketParams { entry: Some(v(10.0, 5.0)), ..ok };
    assert!(pocket(&regions, &tool(1.0), &inside, &cut(0.0, -2.0, 2.0)).unwrap_err().to_string().contains("in air outside"));
    // Conventional milling has no meaning once the open side fixes which way each pass runs.
    let down = PocketParams { climb: false, ..ok };
    let err = pocket(&regions, &tool(1.0), &down, &cut(0.0, -2.0, 2.0)).unwrap_err().to_string();
    assert!(err.contains("climb"), "{err}");
    // A tool wider than the pocket, entered far enough out to stand clear of the walls.
    let wide = PocketParams { entry: Some(v(-30.0, -2.0)), ..ok };
    let err = pocket(&regions, &tool(30.0), &wide, &cut(0.0, -2.0, 2.0)).unwrap_err().to_string();
    assert!(err.contains("does not fit"), "{err}");
}

#[test]
fn open_pocket_goes_round_an_island_and_still_feeds_one_way() {
    // A 40 × 10 pocket open on its left with a hole standing in the middle. The passes
    // nest against the closed wall and, as rings, against the hole too; every one is fed
    // the one way round the metal it hugs, and the floor behind the island is cleared as
    // well as the floor in front of it.
    let regions = [Region2 { outer: rect(0.0, 0.0, 40.0, 10.0).outer, holes: vec![Loop2::circle(v(20.0, 5.0), 2.0)] }];
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let entry = v(-12.0, 5.0);
    let params =
        PocketParams { stepover: 0.4, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: Some(entry), adaptive: false };
    let c = cut(0.0, -2.0, 1.0);
    let path = pocket(&regions, &tool(1.0), &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let d = v(0.0, 1.0);
    let mut pos = v(0.0, 0.0);
    let mut side = 0usize;
    // The disc may stand past the opening's plane, where a pass cuts the pocket flush;
    // it may never come nearer to a closed wall or the island than its own radius.
    let walls = [
        Piece::Line { a: v(40.0, 0.0), b: v(40.0, 10.0) },
        Piece::Line { a: v(0.0, 10.0), b: v(40.0, 10.0) },
        Piece::Line { a: v(0.0, 0.0), b: v(40.0, 0.0) },
    ];
    let island = v(20.0, 5.0);
    // The feeds made so far and the height each ran at: a feed that runs the other way
    // is a turn or a crossing, allowed only over the ground those feeds already cut.
    let mut feeds: Vec<(Vec2, Vec2, f64)> = Vec::new();
    let mut down = 0usize;
    for m in &path.moves {
        if let Move::Rapid { to } = *m {
            pos = v(to.x, to.y);
        }
        if let Move::Feed { to, .. } = *m {
            let q = v(to.x, to.y);
            let closed = |p: Vec2| walls.iter().all(|w| w.dist(p) >= 0.5 - 1e-6) && p.dist(island) - 2.0 >= 0.5 - 1e-6;
            assert!(closed(q) || q.dist(pos) < 1e-9, "the disc gouges at ({}, {})", to.x, to.y);
            if q.dist(pos) < 1e-9 {
                down += 1;
            } else {
                side += 1;
                if (q - pos).normalized().unwrap().dot(d) <= 1e-6 {
                    // Whatever this feed is, the disc must bite nothing on the way: the
                    // ground under it stands within the tool's radius of a feed at its
                    // own height.
                    for i in 0..=4 {
                        let p = pos.lerp(q, i as f64 / 4.0);
                        assert!(
                            feeds
                                .iter()
                                .any(|(a, b, fz)| (*fz - to.z).abs() <= 1e-9
                                    && p.extend(0.0).dist_to_segment(a.extend(0.0), b.extend(0.0)) <= 0.5 - 1e-6),
                            "a feed runs the other way, off the cut floor: ({}, {}) to ({}, {})",
                            pos.x,
                            pos.y,
                            to.x,
                            to.y
                        );
                    }
                }
            }
            feeds.push((pos, q, to.z));
            pos = q;
        }
    }
    assert!(side > 50, "{side} side feeds");
    // The wall's passes chain at the mouth; only the island's rings break the chain,
    // each entered by its own descent, so plunges stay a small share of the path.
    assert!(down * 8 <= side, "{down} plunges for {side} side feeds");
    // The floor behind the island is cut too.
    for i in 0..=80 {
        for j in 0..=20 {
            let q = v(i as f64 * 0.5, j as f64 * 0.5);
            if shape.clear(q, 0.5 + 1e-3) {
                let dd = dist_to_cuts(&path, -2.0 + 1e-6, q);
                assert!(dd <= 0.5 + 0.01, "{q:?} is {dd} from the nearest pass");
            }
        }
    }
}

#[test]
fn open_pocket_finishes_along_its_curved_wall() {
    // A half-disc pocket, open across its flat side and entered square onto it. Every
    // pass is an offset of the curved wall itself, a stepover nearer the centre than the
    // one before it, and they run deepest first: the cut that finishes the pocket is the
    // pass tangent to the wall, one long arc rather than a dozen short chords.
    let regions = [Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: v(0.0, 10.0), b: v(0.0, -10.0) },
                Seg2::Arc { center: v(0.0, 0.0), radius: 10.0, start: -PI / 2.0, sweep: PI },
            ],
        },
        holes: vec![],
    }];
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let params = PocketParams {
        stepover: 0.8,
        climb: true,
        stock_to_leave: 0.0,
        tolerance: 0.01,
        ramp_angle: 0.0,
        entry: Some(v(-12.0, 0.0)),
        adaptive: false,
    };
    let c = cut(0.0, -2.0, 2.0);
    let path = pocket(&regions, &tool(2.0), &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let wall = Piece::Arc { c: v(0.0, 0.0), r: 10.0, a0: -PI / 2.0, sweep: PI };
    let mut pos = v(0.0, 0.0);
    let (mut side, mut down, mut along) = (0usize, 0usize, 0usize);
    let mut feeds: Vec<(Vec2, Vec2)> = Vec::new();
    for m in &path.moves {
        if let Move::Rapid { to } = *m {
            pos = v(to.x, to.y);
        }
        if let Move::Feed { to, .. } = *m {
            let q = v(to.x, to.y);
            assert!(wall.dist(q) >= 1.0 - 1e-6, "the disc gouges the wall at ({}, {})", to.x, to.y);
            if q.dist(pos) < 1e-9 {
                down += 1;
                assert!(pos.x <= 1e-6, "the chain comes down at ({}, {}), inside the pocket", pos.x, pos.y);
            } else {
                side += 1;
                // An arc rounds the far side and comes back, and the links cross the
                // mouth: wherever a feed faces into the pocket, the disc must stand over
                // ground its own path already cut.
                if (q - pos).normalized().unwrap().dot(v(0.0, 1.0)) <= 1e-6 {
                    for i in 0..=4 {
                        let p = pos.lerp(q, i as f64 / 4.0);
                        assert!(
                            feeds.iter().any(|(a, b)| p.extend(0.0).dist_to_segment(a.extend(0.0), b.extend(0.0)) <= 1.0 - 1e-6),
                            "a feed runs the other way, off the cut floor: ({}, {}) to ({}, {})",
                            pos.x,
                            pos.y,
                            to.x,
                            to.y
                        );
                    }
                }
                feeds.push((pos, q));
                if (q.len() - 9.0).abs() <= 0.02 {
                    along += 1;
                }
            }
            pos = q;
        }
    }
    assert!(side > 200, "{side} side feeds");
    // One depth, one plunge: the offsets feed one into the next across the mouth without
    // the tool ever coming up.
    assert_eq!(down, 1, "{down} plunges for a chain of wall offsets");
    // The pocket ends along its wall: many a pass's centre runs the offset the disc cuts
    // flush to, along the arc itself, one long cut rather than a sequence of short chords.
    assert!(along > 40, "{along} feeds run along the curved wall");
    // And the last cut of all is that arc: the path ends on the wall, not on a chord.
    let last = path.moves.iter().rev().find_map(|m| match *m {
        Move::Feed { to, .. } => Some(v(to.x, to.y)),
        _ => None,
    });
    assert!(last.is_some_and(|p| (p.len() - 9.0).abs() <= 0.02), "the path ends at {last:?}");
    // The floor is cleared wherever the disc can reach, the lens beside the wall included.
    for i in 0..=40 {
        for j in 0..=40 {
            let q = v(0.25 + i as f64 * 0.24, -4.9 + j as f64 * 0.25);
            if shape.clear(q, 1.0 + 1e-3) {
                let dd = dist_to_cuts(&path, -2.0 + 1e-6, q);
                assert!(dd <= 1.0 + 0.01, "{q:?} is {dd} from the nearest pass");
            }
        }
    }
}

#[test]
fn open_pocket_roughs_in_nested_offsets_and_turns_at_the_mouth() {
    // A half-disc pocket roughed adaptively: every in-pocket pass runs along an offset of
    // the wall, one stepover inside the last, so the disc bites no more than a stepover's
    // width anywhere; the level is one continuous feed, with every reversal of direction
    // taken in the air where the links cross the mouth; and the wall's own pass finishes
    // the pocket last.
    let regions = [Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: v(0.0, 12.0), b: v(0.0, -12.0) },
                Seg2::Arc { center: v(0.0, 0.0), radius: 12.0, start: -PI / 2.0, sweep: PI },
            ],
        },
        holes: vec![],
    }];
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let entry = v(-20.0, 0.0);
    let params =
        PocketParams { stepover: 1.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: Some(entry), adaptive: false };
    let c = cut(0.0, -2.0, 1.0);
    let r = 0.76;
    let path = pocket(&regions, &tool(2.0 * r), &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    // The pass radii, wall first and deepest last: the wall's own offset, then a stepover
    // in, until the disc would show past the centre.
    let rings: Vec<f64> = (0..8).map(|m| 12.0 - r - 1.5 * m as f64).collect();
    let on_a_ring = |p: Vec2| rings.iter().any(|&q| (p.len() - q).abs() <= 0.01);
    let mut pos = v(0.0, 0.0);
    let mut dir: Option<Vec2> = None;
    let (mut down, mut level_max, mut cur_z) = (0usize, 0.0f64, f64::NAN);
    let wall = Piece::Arc { c: v(0.0, 0.0), r: 12.0, a0: -PI / 2.0, sweep: PI };
    for m in &path.moves {
        if let Move::Rapid { to } = *m {
            // The tool comes up clear of the work between levels and never rapid below
            // the retract height: all depth is taken feeding.
            assert!(to.z >= 2.0 - 1e-9, "a rapid runs down to {} inside the pocket", to.z);
            pos = v(to.x, to.y);
            dir = None;
        }
        if let Move::Feed { to, .. } = *m {
            let q = v(to.x, to.y);
            assert!(wall.dist(q) >= r - 1e-6, "the disc breaks the wall at ({}, {})", to.x, to.y);
            if q.dist(pos) < 1e-9 {
                down += 1;
                assert!(q.x <= 1e-6, "the level begins at ({}, {}), not in the air off the mouth", q.x, q.y);
            } else {
                // Wherever the pass runs inside the pocket it runs a ring; where it turns
                // it turns in the air.
                if q.x > 0.0 && pos.x > 0.0 {
                    assert!(on_a_ring(q) && on_a_ring(pos), "a feed crosses the pocket off any ring: ({}, {}) to ({}, {})", pos.x, pos.y, q.x, q.y);
                }
                if let Some(t) = (q - pos).normalized() {
                    // A turn at depth is the arc's own gentle bend; the sharpest corners
                    // in this path are the plunges' corners, which no feed sees.
                    if let Some(p) = dir {
                        let mut a = p.angle() - t.angle();
                        while a > PI {
                            a -= 2.0 * PI;
                        }
                        while a < -PI {
                            a += 2.0 * PI;
                        }
                        assert!(a.abs() <= 80.0f64.to_radians(), "a feed turns {} degrees at ({}, {})", a.to_degrees(), q.x, q.y);
                    }
                    dir = Some(t);
                }
                if (to.z - cur_z).abs() > 1e-9 {
                    if !cur_z.is_nan() {
                        assert!(level_max >= 12.0 - r - 0.01, "a level ended {level_max} shy of its wall");
                    }
                    cur_z = to.z;
                    level_max = 0.0;
                }
                if q.x > 0.0 {
                    level_max = level_max.max(q.len());
                }
            }
            pos = q;
        }
    }
    // Two depths, two plunges: each level feeds all the way through in one descent.
    assert_eq!(down, 2, "{down} plunges for two levels");
    // Each level ends on the wall's own ring: the finishing pass is tangent to the wall.
    assert!(level_max >= 12.0 - r - 0.01, "the last level ended {level_max} shy of its wall");
    // The floor is cleared wherever the disc can reach: rings a stepover apart leave the
    // standing metal no wider than the tool's radius beyond the nearest pass.
    for i in 0..=40 {
        for j in 0..=80 {
            let q = v(0.05 + i as f64 * 0.3, -12.0 + j as f64 * 0.3);
            if shape.clear(q, r + 1e-3) {
                let dd = dist_to_cuts(&path, -2.0 + 1e-6, q);
                assert!(dd <= r + 0.01, "{q:?} is {dd} from the nearest pass");
            }
        }
    }
}

/// Two holes plunged at 200 mm/min with a quarter-second dwell, written out move by move.
fn drill_program() -> Program {
    let t = Tool::new(3, ToolKind::Drill, 5.0, 40.0);
    let at = |x: f64, z: f64| Vec3::new(x, 10.0, z);
    let mut moves = Vec::new();
    for (i, x) in [10.0, 30.5].into_iter().enumerate() {
        if i > 0 {
            moves.push(Move::Rapid { to: at(10.0, 10.0) });
        }
        moves.extend([
            Move::Rapid { to: at(x, 10.0) },
            Move::Rapid { to: at(x, 2.0) },
            Move::Feed { to: at(x, -10.0), feed: 200.0 },
            Move::Dwell { seconds: 0.25 },
        ]);
    }
    moves.push(Move::Rapid { to: at(30.5, 10.0) });
    let path = Toolpath::new(moves);
    verify(&path, &cut(0.0, -10.0, 1.0).heights).unwrap();
    Program {
        work_offset: None,
        name: "Bracket (rev 2)".into(),
        operations: vec![Operation { name: "Drill".into(), tool: t, spindle_rpm: 3000.0, coolant: true, path }],
    }
}

/// A post for a controller that wants a decimal point on every number, two-digit motion codes,
/// dwells as `G04 X` in seconds and helices as lines.
fn padded() -> Post {
    Post {
        name: "padded".into(),
        trailing_decimal: true,
        two_digit_codes: true,
        helical_arcs: false,
        dwell: "G04 X{seconds}".into(),
        ..Post::rs274()
    }
}

#[test]
fn post_switches_change_how_numbers_and_codes_are_written() {
    let text = emit(&drill_program(), &padded()).unwrap();
    for line in ["G00 X10. Y10.", "G00 Z2.", "G01 Z-10. F200.", "G04 X0.25", "G00 X30.5"] {
        assert!(text.lines().any(|l| l == line), "no `{line}` in\n{text}");
    }
    let back = Toolpath::new(parse(&text).unwrap());
    assert_eq!(back.stats(), drill_program().operations[0].path.stats());
}

#[test]
fn rs274_program_text() {
    let text = emit(&drill_program(), &Post::rs274()).unwrap();
    let expected = "\
(Bracket rev 2)
G17 G21 G40 G49 G54 G80 G90 G94
(Drill: T3 D5 drill)
M5
T3 M6
G43 H3
S3000 M3
M8
G0 Z10
G0 X10 Y10
G0 Z2
G1 Z-10 F200
G4 P0.25
G0 Z10
G0 X30.5
G0 Z2
G1 Z-10
G4 P0.25
G0 Z10
M9
M5
M30
";
    assert_eq!(text, expected);
    // German names are spelled out in comments, not cut down.
    let program = Program { name: "Außenkontur Größe Ü1".into(), ..drill_program() };
    let text = emit(&program, &Post::rs274()).unwrap();
    assert!(text.starts_with("(Aussenkontur Groesse Ue1)\n"), "{text}");
}

#[test]
fn written_program_reads_back_as_the_same_path() {
    let island = Loop2::circle(v(30.0, 20.0), 5.0);
    let regions = [Region2 { outer: rect(0.0, 0.0, 60.0, 40.0).outer, holes: vec![island] }];
    let params = PocketParams { stepover: 3.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -4.0, 2.0)).unwrap();
    let program = Program {
        work_offset: None,
        name: "p".into(),
        operations: vec![Operation { name: "Pocket".into(), tool: tool(6.0), spindle_rpm: 12000.0, coolant: false, path: path.clone() }],
    };
    for arcs in [true, false] {
        let mut post = Post::rs274();
        post.arcs = arcs;
        let text = emit(&program, &post).unwrap();
        assert_eq!(text.contains("G2 ") || text.contains("G3 "), arcs);
        let back = Toolpath::new(parse(&text).unwrap());
        verify(&back, &cut(0.0, -4.0, 2.0).heights).unwrap();
        let (a, b) = (path.stats(), back.stats());
        // Coordinates are written to 0.0001 mm.
        close(a.cut_length, b.cut_length, 0.2);
        close(a.rapid_length, b.rapid_length, 0.2);
        assert_eq!(a.lowest_z, b.lowest_z);
        // Every point of the path as read is on the path as planned.
        let planned: Vec<Vec3> = path.points(0.002).into_iter().map(|(p, _)| p).collect();
        for (p, _) in back.points(0.002) {
            let d = planned.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
            assert!(d < 0.02, "{p:?} is {d} off the planned path");
        }
    }
}

#[test]
fn every_post_writes_within_its_own_precision() {
    let island = Loop2::circle(v(30.0, 20.0), 5.0);
    let regions = [Region2 { outer: rect(0.0, 0.0, 60.0, 40.0).outer, holes: vec![island] }];
    let params = PocketParams {
        stepover: 3.0,
        climb: true,
        stock_to_leave: 0.0,
        tolerance: 0.01,
        ramp_angle: 3.0f64.to_radians(),
        entry: None,
        adaptive: false,
    };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -4.0, 2.0)).unwrap();
    let mut program = Program {
        work_offset: None,
        name: "p".into(),
        operations: vec![Operation { name: "Pocket".into(), tool: tool(6.0), spindle_rpm: 12000.0, coolant: false, path: path.clone() }],
    };
    let mut inch = Post::rs274();
    inch.units = Units::Inch;
    let mut chords = Post::rs274();
    chords.arcs = false;
    for post in [Post::rs274(), inch, chords, padded()] {
        let text = emit(&program, &post).unwrap();
        let r = readback(&text, &program, &post).unwrap();
        assert!(r.within(), "{}: {r:?}", post.name);
        close(r.cut_length, r.planned_cut_length, 0.05 * r.planned_cut_length / 100.0 + 0.5);
    }
    // The precision a post allows: four decimals in millimetres, 0.005 mm chords.
    close(allowed_deviation(&Post::rs274()), 3.0 * 0.00005 * 3f64.sqrt() + 0.01 + 1e-6, 1e-12);

    // A post that rounds to whole millimetres keeps to its own precision, but that precision
    // is coarse: the program strays tenths of a millimetre, which the read-back measures.
    let mut coarse = Post::rs274();
    coarse.decimals = 0;
    let r = readback(&emit(&program, &coarse).unwrap(), &program, &coarse).unwrap();
    assert!(r.within() && r.max_deviation > 0.3 && r.allowed > 2.0, "{r:?}");

    // A program that is not what was written: every move 0.5 mm higher.
    let text = emit(&program, &Post::rs274()).unwrap();
    for m in &mut program.operations[0].path.moves {
        if let Move::Rapid { to } | Move::Feed { to, .. } | Move::Arc { to, .. } = m {
            to.z += 0.5;
        }
    }
    let r = readback(&text, &program, &Post::rs274()).unwrap();
    assert!(!r.within(), "{r:?}");
    close(r.max_deviation, 0.5, 0.01);
    // A program with no cut against one with cuts: as far as the comparison looks.
    let empty = Program {
        work_offset: None,
        name: "e".into(),
        operations: vec![Operation { path: Toolpath::new(vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) }]), ..program.operations[0].clone() }],
    };
    let r = readback(&text, &empty, &Post::rs274()).unwrap();
    assert_eq!(r.max_deviation, r.reach);
}

#[test]
fn arc_too_short_to_write_is_never_a_full_circle() {
    // 0.00002 mm of arc: its end is written the same as its start.
    let moves = vec![
        Move::Rapid { to: Vec3::new(10.0, 0.0, 5.0) },
        Move::Feed { to: Vec3::new(10.0, 0.0, -1.0), feed: 100.0 },
        Move::Arc { to: Vec3::new(10.0, 0.00002, -1.0), center: v(0.0, 0.0), ccw: true, feed: 100.0 },
    ];
    let program = Program {
        work_offset: None,
        name: "p".into(),
        operations: vec![Operation { name: "o".into(), tool: tool(6.0), spindle_rpm: 1000.0, coolant: false, path: Toolpath::new(moves) }],
    };
    let text = emit(&program, &Post::rs274()).unwrap();
    assert!(!text.contains("G2 ") && !text.contains("G3 "), "{text}");
}

#[test]
fn verify_refuses_unsafe_paths() {
    let h = cut(0.0, -5.0, 1.0).heights;
    let at = |x: f64, z: f64| Vec3::new(x, 0.0, z);
    let unsafe_paths = [
        // Starts below the clearance height.
        vec![Move::Rapid { to: at(0.0, 1.0) }],
        // Starts with a cut.
        vec![Move::Feed { to: at(0.0, 10.0), feed: 100.0 }],
        // Rapid down into uncut material.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Rapid { to: at(0.0, -1.0) }],
        // Sideways rapid below the retract height.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Feed { to: at(0.0, -1.0), feed: 100.0 }, Move::Rapid { to: at(5.0, -1.0) }],
        // Below the bottom.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Feed { to: at(0.0, -5.1), feed: 100.0 }],
        // No feed.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Feed { to: at(0.0, -1.0), feed: 0.0 }],
        // Not a number.
        vec![Move::Rapid { to: at(f64::NAN, 10.0) }],
        // An arc whose ends are at different radii.
        vec![Move::Rapid { to: at(5.0, 10.0) }, Move::Arc { to: at(-7.0, 10.0), center: v(0.0, 0.0), ccw: true, feed: 100.0 }],
    ];
    for moves in unsafe_paths {
        assert!(matches!(verify(&Toolpath::new(moves.clone()), &h), Err(CamError::Unsafe(_))), "{moves:?}");
    }
    // A drill returning into its own hole is fine.
    let ok = vec![
        Move::Rapid { to: at(0.0, 10.0) },
        Move::Feed { to: at(0.0, -3.0), feed: 100.0 },
        Move::Rapid { to: at(0.0, 2.0) },
        Move::Rapid { to: at(0.0, -2.5) },
        Move::Feed { to: at(0.0, -5.0), feed: 100.0 },
    ];
    verify(&Toolpath::new(ok), &h).unwrap();
}

#[test]
fn hostile_values_are_errors_never_panics() {
    let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e300, -1e300, 1e-300, f64::MIN_POSITIVE];
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let good_cut = cut(0.0, -5.0, 2.0);
    for &b in &bad {
        let _ = offset_regions(&regions, b, 0.01);
        let _ = offset_regions(&regions, 1.0, b);
        let _ = offset_regions(&[rect(0.0, 0.0, b, b)], 1.0, 0.01);
        let _ = offset_regions(&[Region2 { outer: Loop2::circle(v(b, b), b), holes: vec![Loop2::circle(v(0.0, b), b)] }], b, 0.01);
        let t = Tool::new(1, ToolKind::FlatEndmill, b, b);
        let pocket_params = PocketParams { stepover: b, climb: true, stock_to_leave: b, tolerance: b, ramp_angle: b, entry: None, adaptive: false };
        let contour_params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: b, tolerance: b, ramp_angle: b };
        let bad_cut = Cut { heights: Heights { clearance: b, retract: b, top: b, bottom: -b }, feeds: Feeds { cut: b, plunge: b }, stepdown: b };
        let slot_params_bad = SlotParams { width: Some(b), stepover: b, stock_to_leave: b, tolerance: b, ramp_angle: b, trochoidal: false };
        let slot_path = open_line((0.0, 0.0), (40.0, 0.0));
        for c in [good_cut, bad_cut, Cut { stepdown: b, ..good_cut }] {
            for tl in [&t, &tool(6.0)] {
                let _ = pocket(&regions, tl, &pocket_params, &c);
                let _ = pocket(
                    &regions,
                    tl,
                    &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false },
                    &c,
                );
                let _ = contour(&regions, tl, &contour_params, &c);
                let _ = slot(std::slice::from_ref(&slot_path), tl, &slot_params_bad, &c);
            }
        }
        let _ = ToolLibraryFile::from_json(&format!("{{\"pad\":{b}}}"));
        let _ = ToolLibraryFile::from_json(&b.to_string());
        let _ = ToolLibraryFile::from_json(&format!(
            "{{\"format\":\"solidcraft-tools\",\"version\":1,\"tools\":[{{\"number\":1,\"kind\":\"flat_endmill\",\"diameter\":{b},\"flute_length\":{b}}}]}}"
        ));
        let moves = vec![
            Move::Rapid { to: Vec3::new(b, b, b) },
            Move::Arc { to: Vec3::new(b, 0.0, b), center: v(b, b), ccw: true, feed: b },
            Move::Dwell { seconds: b },
        ];
        let path = Toolpath::new(moves);
        let _ = path.stats();
        let _ = path.points(b);
        let _ = verify(&path, &good_cut.heights);
        let program = Program {
            work_offset: None,
            name: "x".into(),
            operations: vec![Operation { name: "o".into(), tool: tool(6.0), spindle_rpm: b, coolant: false, path }],
        };
        let _ = emit(&program, &Post::rs274());
    }
    // Things that must be refused outright.
    assert!(
        pocket(
            &regions,
            &tool(6.0),
            &PocketParams { stepover: 1e-9, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false },
            &good_cut
        )
        .is_err()
    );
    assert!(
        pocket(
            &regions,
            &tool(60.0),
            &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false },
            &good_cut
        )
        .is_err()
    );
    assert!(
        pocket(
            &[],
            &tool(6.0),
            &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false },
            &good_cut
        )
        .is_err()
    );
    // A drill cuts only at its point: no contour or pocket with one.
    let drill = Tool::new(4, ToolKind::Drill, 6.0, 30.0);
    let square = [rect(0.0, 0.0, 20.0, 20.0)];
    let side = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(contour(&square, &drill, &side, &good_cut).unwrap_err().to_string().contains("a drill cannot mill"));
    let lanes = PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
    assert!(pocket(&square, &drill, &lanes, &good_cut).is_err());
    let open = Region2 { outer: Loop2 { segs: rect(0.0, 0.0, 10.0, 10.0).outer.segs.into_iter().take(3).collect() }, holes: vec![] };
    assert!(offset_regions(&[open], 1.0, 0.01).is_err());
    assert!(emit(&Program::default(), &Post::rs274()).is_err());
}

/// An open path of one straight segment.
fn open_line(a: (f64, f64), b: (f64, f64)) -> OpenPath {
    OpenPath { segs: vec![Seg2::Line { a: v(a.0, a.1), b: v(b.0, b.1) }] }
}

/// Length of the moves that stay at `z` (a lift or a rapid is not part of it).
fn length_at(path: &Toolpath, z: f64) -> f64 {
    path.points(0.001)
        .windows(2)
        .filter(|w| (w[0].0.z - z).abs() < 1e-9 && (w[1].0.z - z).abs() < 1e-9)
        .map(|w| (w[1].0.x - w[0].0.x).hypot(w[1].0.y - w[0].0.y))
        .sum()
}

fn slot_params() -> SlotParams {
    SlotParams { width: None, stepover: 3.0, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, trochoidal: false }
}

#[test]
fn a_straight_slot_as_wide_as_the_tool_cuts_each_depth_once() {
    let c = cut(0.0, -6.0, 3.0);
    let path = slot(&[open_line((0.0, 0.0), (40.0, 0.0))], &tool(3.0), &slot_params(), &c).unwrap();
    verify(&path, &c.heights).unwrap();
    close(length_at(&path, -3.0), 40.0, 1e-6);
    close(length_at(&path, -6.0), 40.0, 1e-6);
    // Between the passes the tool goes up to the retract height to get back to the start.
    assert!(path.moves.iter().any(|m| matches!(m, Move::Rapid { to } if (to.z - c.heights.retract).abs() < 1e-9)));
    // A slot as wide as the tool is one pass, so the stock to leave changes nothing.
    let left = SlotParams { stock_to_leave: 2.0, ..slot_params() };
    assert_eq!(slot(&[open_line((0.0, 0.0), (40.0, 0.0))], &tool(3.0), &left, &c).unwrap().stats(), path.stats());
}

#[test]
fn an_arc_slot_keeps_its_arc() {
    let c = cut(0.0, -3.0, 3.0);
    let arc = OpenPath { segs: vec![Seg2::Arc { center: v(0.0, 0.0), radius: 10.0, start: 0.0, sweep: FRAC_PI_2 }] };
    let path = slot(&[arc], &tool(4.0), &slot_params(), &c).unwrap();
    verify(&path, &c.heights).unwrap();
    assert!(path.moves.iter().any(|m| matches!(m, Move::Arc { .. })), "the arc stays an arc");
    close(length_at(&path, -3.0), 10.0 * FRAC_PI_2, 0.01);
    // And the program keeps it as G2/G3 and reads back.
    let program = Program {
        name: "Slot".into(),
        operations: vec![Operation { name: "Slot".into(), tool: tool(4.0), spindle_rpm: 4000.0, coolant: false, path }],
        work_offset: None,
    };
    let post = Post::builtin("linuxcnc").unwrap();
    let text = emit(&program, &post).unwrap();
    assert!(text.contains("G2") || text.contains("G3"));
    assert!(readback(&text, &program, &post).unwrap().within());
}

#[test]
fn a_wide_slot_reaches_both_walls_with_side_passes() {
    let c = cut(0.0, -3.0, 3.0);
    let t = tool(4.0);
    let line = open_line((0.0, 0.0), (40.0, 0.0));
    // 10 mm wide with a 4 mm tool: the centre pass and one pass each side at 3 mm, where the
    // tool's edge is exactly at the wall (width/2 = 5).
    let wide = SlotParams { width: Some(10.0), stepover: 3.0, stock_to_leave: 0.0, ..slot_params() };
    let path = slot(std::slice::from_ref(&line), &t, &wide, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let ys: Vec<f64> = cutting_points(&path, 0.0).iter().map(|p| p.y).collect();
    close(ys.iter().copied().fold(f64::NEG_INFINITY, f64::max), 3.0, 1e-6);
    close(ys.iter().copied().fold(f64::INFINITY, f64::min), -3.0, 1e-6);
    close(length_at(&path, -3.0), 3.0 * 40.0, 1e-6);
    // A smaller stepover adds passes in between, and stock to leave pulls them off the wall.
    let closer = SlotParams { width: Some(10.0), stepover: 1.0, stock_to_leave: 1.0, ..slot_params() };
    let path = slot(&[line], &t, &closer, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let ys: Vec<f64> = cutting_points(&path, 0.0).iter().map(|p| p.y).collect();
    close(ys.iter().copied().fold(f64::NEG_INFINITY, f64::max), 2.0, 1e-6);
    close(ys.iter().copied().fold(f64::INFINITY, f64::min), -2.0, 1e-6);
    close(length_at(&path, -3.0), 5.0 * 40.0, 1e-6);
}

#[test]
fn a_slot_ramps_into_an_open_path_and_round_a_closed_one() {
    let ramp = 5.0f64.to_radians();
    let params = SlotParams { ramp_angle: ramp, stepover: 2.0, ..slot_params() };
    let c = cut(0.0, -6.0, 2.0);
    // Open: back and forth along the first part, never straight down.
    let path = slot(&[open_line((0.0, 0.0), (40.0, 0.0))], &tool(3.0), &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let (angle, plunges) = steepest(&path, 0.0);
    assert!(!plunges, "an open slot ramps instead of plunging");
    assert!(angle <= ramp + 1e-6, "{angle} is steeper than {ramp}");
    close(length_at(&path, -2.0), 40.0, 1e-6);
    // Closed: round the loop itself.
    let square = OpenPath {
        segs: vec![
            Seg2::Line { a: v(0.0, 0.0), b: v(40.0, 0.0) },
            Seg2::Line { a: v(40.0, 0.0), b: v(40.0, 30.0) },
            Seg2::Line { a: v(40.0, 30.0), b: v(0.0, 30.0) },
            Seg2::Line { a: v(0.0, 30.0), b: v(0.0, 0.0) },
        ],
    };
    let path = slot(&[square], &tool(3.0), &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let (angle, plunges) = steepest(&path, 0.0);
    assert!(!plunges, "a closed slot ramps round itself");
    assert!(angle <= ramp + 1e-6, "{angle} is steeper than {ramp}");
    close(length_at(&path, -2.0), 140.0, 1e-6);
}

#[test]
fn slots_that_do_not_fit_are_refused() {
    let c = cut(0.0, -3.0, 3.0);
    let t = tool(6.0);
    let line = open_line((0.0, 0.0), (40.0, 0.0));
    let err = slot(std::slice::from_ref(&line), &t, &SlotParams { width: Some(4.0), ..slot_params() }, &c).unwrap_err();
    assert!(format!("{err}").contains("the slot is narrower than the tool"), "{err}");
    assert!(slot(std::slice::from_ref(&line), &t, &SlotParams { stepover: 7.0, ..slot_params() }, &c).is_err());
    assert!(slot(std::slice::from_ref(&line), &t, &SlotParams { stepover: 0.0, ..slot_params() }, &c).is_err());
    assert!(slot(std::slice::from_ref(&line), &t, &SlotParams { ramp_angle: 2.0, ..slot_params() }, &c).is_err());
    assert!(slot(std::slice::from_ref(&line), &t, &SlotParams { width: Some(f64::NAN), ..slot_params() }, &c).is_err());
    assert!(slot(std::slice::from_ref(&line), &t, &SlotParams { stock_to_leave: -1.0, ..slot_params() }, &c).is_err());
    // A side pass that would turn the arc inside out.
    let tight = OpenPath { segs: vec![Seg2::Arc { center: v(0.0, 0.0), radius: 4.0, start: 0.0, sweep: FRAC_PI_2 }] };
    let err = slot(&[tight], &t, &SlotParams { width: Some(14.0), ..slot_params() }, &c).unwrap_err();
    assert!(format!("{err}").contains("the tool does not fit the open contour"), "{err}");
    // A drill cuts only at its point; no paths at all is no slot.
    assert!(slot(std::slice::from_ref(&line), &Tool::new(4, ToolKind::Drill, 6.0, 30.0), &slot_params(), &c).is_err());
    assert!(slot(&[], &t, &slot_params(), &c).is_err());
}

#[test]
fn a_tool_library_round_trips_with_cutting_data_per_material() {
    let mut flat = Tool::new(3, ToolKind::FlatEndmill, 6.0, 20.0);
    flat.description = "6 mm flat".into();
    flat.cutting.insert("aluminium".into(), CuttingData { vc: 250.0, fz: 0.06, ap: 0.5, ae: 0.4 });
    flat.cutting.insert("oak".into(), CuttingData { vc: 120.0, fz: 0.05, ap: 1.0, ae: 0.5 });
    let drill = Tool::new(1, ToolKind::Drill, 5.0, 40.0);
    let library = ToolLibraryFile { tools: vec![flat.clone(), drill.clone()] };
    let text = library.to_json().unwrap();
    assert!(text.contains("\"format\": \"solidcraft-tools\""), "{text}");
    assert!(text.contains("\"version\": 1"));
    // Sorted by number, so a second export of the same library is the same file.
    let back = ToolLibraryFile::from_json(&text).unwrap();
    assert_eq!(back.tools, vec![drill, flat]);
    assert_eq!(ToolLibraryFile { tools: back.tools.clone() }.to_json().unwrap(), text);
}

#[test]
fn a_tool_library_refuses_what_it_cannot_read() {
    let refused = |result: Result<ToolLibraryFile>, what: &str| {
        let message = match result {
            Ok(_) => String::new(),
            Err(error) => format!("{error}"),
        };
        assert!(message.contains(what), "expected `{what}` in `{message}`");
    };
    let tool = |number: u32| serde_json::to_value(Tool::new(number, ToolKind::FlatEndmill, 6.0, 20.0)).unwrap();
    let good = serde_json::json!({"format": "solidcraft-tools", "version": 1, "tools": [tool(1)]}).to_string();
    assert_eq!(ToolLibraryFile::from_json(&good).unwrap().tools.len(), 1);
    refused(ToolLibraryFile::from_json(r#"{"format":"other","version":1,"tools":[]}"#), "not a SolidCraft tool library");
    refused(ToolLibraryFile::from_json(r#"{"format":"solidcraft-tools","version":2,"tools":[]}"#), "version 2");
    refused(ToolLibraryFile::from_json(r#"{"format":"solidcraft-tools","version":1}"#), "no `tools` list");
    refused(ToolLibraryFile::from_json("[]"), "is a JSON object");
    refused(ToolLibraryFile::from_json("{"), "not valid JSON");
    refused(ToolLibraryFile::from_json(""), "not valid JSON");
    // The same number twice.
    let twice = serde_json::json!({"format": "solidcraft-tools", "version": 1, "tools": [tool(3), tool(3)]}).to_string();
    refused(ToolLibraryFile::from_json(&twice), "tool 3 is in the file twice");
    // A tool that is not valid, named by its number.
    let mut bad = Tool::new(7, ToolKind::FlatEndmill, 6.0, 20.0);
    bad.diameter = 0.0;
    let text = serde_json::json!({"format": "solidcraft-tools", "version": 1, "tools": [serde_json::to_value(bad).unwrap()]}).to_string();
    refused(ToolLibraryFile::from_json(&text), "tool 7:");
    // A tool that is not a tool at all.
    let text = serde_json::json!({"format": "solidcraft-tools", "version": 1, "tools": [{"number": "x"}]}).to_string();
    assert!(ToolLibraryFile::from_json(&text).is_err());
    // More tools than are kept, and more text than is read.
    let many: Vec<_> = (0..=MAX_LIBRARY_TOOLS).map(|i| tool((i % 9_999) as u32 + 1)).collect();
    let text = serde_json::json!({"format": "solidcraft-tools", "version": 1, "tools": many}).to_string();
    refused(ToolLibraryFile::from_json(&text), "more than 10000 tools");
    let big = format!(r#"{{"format":"solidcraft-tools","version":1,"tools":[],"pad":"{}"}}"#, "x".repeat(MAX_LIBRARY_TEXT));
    assert!(ToolLibraryFile::from_json(&big).is_err());
}

#[test]
fn posts_are_data_and_bad_ones_are_refused() {
    let mut own = Post::rs274();
    (own.name, own.units, own.line_numbers) = ("own".into(), Units::Inch, Some(LineNumbers { start: 10, step: 5 }));
    let json = serde_json::to_string(&own).unwrap();
    let post: Post = serde_json::from_str(&json).unwrap();
    assert_eq!(post, own);
    assert_eq!(Post::builtin("LinuxCNC"), Some(Post::linuxcnc()));
    assert!(Post::BUILTIN.iter().all(|name| Post::builtin(name).is_some_and(|p| p.validate().is_ok())));
    let broken = |change: fn(&mut Post)| {
        let mut p = Post::rs274();
        change(&mut p);
        p.validate().is_err() && emit(&drill_program(), &p).is_err()
    };
    assert!(broken(|p| p.header = vec!["G21 {nonsense}".into()]));
    assert!(broken(|p| p.footer = vec!["M30 {tool".into()]));
    assert!(broken(|p| p.tool_change = vec!["M6\nG0 Z-100".into()]));
    assert!(broken(|p| p.decimals = 9));
    assert!(broken(|p| p.arc_tolerance = f64::NAN));
    assert!(broken(|p| p.line_numbers = Some(LineNumbers { start: 1, step: 0 })));
    assert!(broken(|p| p.extension = "../x".into()));
    assert!(broken(|p| p.spindle_on = vec!["M3".into(); 1000]));
    assert!(broken(|p| p.dwell = "G4 P{minutes}".into()));
    // A post saved before these switches existed reads with what it meant.
    let mut old = serde_json::to_value(Post::rs274()).unwrap();
    for k in ["helical_arcs", "trailing_decimal", "two_digit_codes", "dwell"] {
        old.as_object_mut().unwrap().remove(k);
    }
    assert_eq!(serde_json::from_value::<Post>(old).unwrap(), Post::rs274());
}

#[test]
fn reader_reports_what_it_cannot_read() {
    for text in ["G1 X1 Y1 Z1", "X1 Y1 Z1", "G0 X1 Y1 Z1\nG2 X2 Y2", "G91", "G0 X1..2", "G0 X", "G0 @", "G2 X1 Y1 Z1 I1 F10"] {
        assert!(matches!(parse(text), Err(CamError::Parse(..))), "{text}");
    }
    assert_eq!(parse("%\n(a comment) ; another\nN10 g0 x1 y2 (inline) z3\n").unwrap(), vec![Move::Rapid { to: Vec3::new(1.0, 2.0, 3.0) }]);
    assert_eq!(parse("G20 G0 X1 Y0 Z0\nG1 X2 F10").unwrap().last(), Some(&Move::Feed { to: Vec3::new(50.8, 0.0, 0.0), feed: 254.0 }));
}

#[test]
fn work_offsets_go_where_the_post_writes_them() {
    let mut program = drill_program();
    program.work_offset = Some(3);
    let text = emit(&program, &Post::rs274()).unwrap();
    assert!(text.contains("\nG17 G21 G40 G49 G56 G80 G90 G94\n"), "{text}");
    // A post that leaves the offset to the operator can only stand for the first one.
    let mut manual = Post::rs274();
    manual.header = vec!["G17 {units} G40 G49 G80 G90 G94".into()];
    let refused = emit(&program, &manual).unwrap_err().to_string();
    assert!(refused.contains("writes no work offset") && refused.contains("G56"), "{refused}");
    program.work_offset = Some(1);
    assert!(emit(&program, &manual).is_ok());
    for n in [0, 7, 255] {
        program.work_offset = Some(n);
        assert!(emit(&program, &Post::rs274()).is_err());
    }
    assert_eq!(work_offset_code(6).unwrap(), "G59");
}

#[test]
fn cutting_data_give_spindle_speed_and_feeds_per_material() {
    // n = vc · 1000 / (π · D), vf = n · z · fz.
    let mut mill = tool(10.0);
    mill.flutes = 3;
    let data = CuttingData { vc: 100.0 * PI / 1000.0 * 1000.0, fz: 0.05, ap: 0.5, ae: 0.4 };
    let s = data.speeds(&mill, None).unwrap();
    assert_eq!(s.spindle_rpm, 10_000.0);
    assert_eq!(s.feed, 1500.0);
    assert_eq!(s.plunge_feed, 750.0);
    close(s.stepdown, 5.0, 1e-12);
    close(s.stepover, 4.0, 1e-12);
    // A slower spindle keeps the chip: the feed follows the speed reached.
    let held = data.speeds(&mill, Some(5000.0)).unwrap();
    assert_eq!((held.spindle_rpm, held.feed), (5000.0, 750.0));
    // A drill feeds per revolution.
    let drill_tool = Tool::new(2, ToolKind::Drill, 5.0, 30.0);
    let d = CuttingData { vc: 5.0 * PI, fz: 0.1, ap: 1.0, ae: 1.0 }.speeds(&drill_tool, None).unwrap();
    assert_eq!((d.spindle_rpm, d.feed, d.plunge_feed), (1000.0, 100.0, 100.0));
    // Every starting group has data, harder materials cut slower; own data win over them.
    for kind in [ToolKind::FlatEndmill, ToolKind::Drill] {
        for m in MATERIALS {
            CuttingData::starting(kind, m, 6.0).unwrap().validate().unwrap();
        }
    }
    let vc = |m: &str| CuttingData::starting(ToolKind::FlatEndmill, m, 6.0).unwrap().vc;
    assert!(vc("aluminium") > vc("steel") && vc("steel") > vc("stainless"));
    assert_eq!(CuttingData::starting(ToolKind::FlatEndmill, "unobtainium", 6.0), None);
    mill.cutting.insert("aluminium".into(), data);
    assert_eq!(mill.cutting_for("aluminium"), Some(data));
    assert!(mill.cutting_for("steel").is_some());
    // Bad data and names are refused, not used.
    for bad in [CuttingData { vc: 0.0, ..data }, CuttingData { fz: f64::NAN, ..data }, CuttingData { ae: 2.0, ..data }] {
        assert!(bad.validate().is_err() && bad.speeds(&mill, None).is_err());
    }
    mill.cutting.insert(" padded".into(), data);
    assert!(mill.validate().is_err());
    assert!(valid_material("Holz (Buche)") && !valid_material("") && !valid_material("a\nb"));
}

#[test]
fn the_load_of_a_cut_gives_chip_power_and_torque_and_thin_chips_raise_the_feed() {
    // From half the diameter on the chip is fz; below it thinner, by 1 / (2 √(r (1 − r))).
    close(chip_thinning(3.0, 6.0), 1.0, 1e-12);
    close(chip_thinning(6.0, 6.0), 1.0, 1e-12);
    close(chip_thinning(0.6, 6.0), 1.0 / 0.6, 1e-9);
    assert_eq!(chip_thinning(1e-9, 6.0), cutting::MAX_CHIP_THINNING);
    for bad in [(0.0, 6.0), (-1.0, 6.0), (f64::NAN, 6.0), (1.0, 0.0)] {
        assert_eq!(chip_thinning(bad.0, bad.1), 1.0);
    }
    // A 6 mm three-flute end mill in aluminium: 10000 rev/min, fz 0.03 mm, 6 mm deep, 3 mm wide.
    let mut mill = tool(6.0);
    mill.flutes = 3;
    let l = load(&mill, "aluminium", 10_000.0, 900.0, 6.0, 3.0).unwrap();
    close(l.removal, 16.2, 1e-9);
    close(l.chip, 0.03, 1e-12);
    assert_eq!(l.thinned_feed, 900.0);
    // kc from the mean chip fz · 2 / π after Kienzle, P = Q · kc / 60e6 / η.
    let kc = 700.0 * (0.03 * 2.0 / PI).powf(-0.25);
    close(l.power, 16_200.0 * kc / 60.0e6 / SPINDLE_EFFICIENCY, 1e-9);
    assert!(l.power > 0.5 && l.power < 0.8, "{l:?}");
    close(l.torque, l.power * 1000.0 * 60.0 / (2.0 * PI * 10_000.0), 1e-9);
    // Twice as deep, twice the power; steel takes more than aluminium.
    close(load(&mill, "aluminium", 10_000.0, 900.0, 12.0, 3.0).unwrap().power, 2.0 * l.power, 1e-9);
    assert!(load(&mill, "steel", 10_000.0, 900.0, 6.0, 3.0).unwrap().power > 2.0 * l.power);
    // A light stepover thins the chip: the feed may rise until it is fz again.
    let light = load(&mill, "aluminium", 10_000.0, 900.0, 6.0, 0.6).unwrap();
    close(light.chip, 0.03 * 0.6, 1e-9);
    assert_eq!(light.thinned_feed, 1500.0);
    assert!(light.power < l.power);
    // A drill cuts its whole diameter; a tap, unknown materials and hostile values give nothing.
    let drill_tool = Tool::new(2, ToolKind::Drill, 10.0, 50.0);
    let d = load(&drill_tool, "steel", 800.0, 120.0, 0.0, 0.0).unwrap();
    close(d.removal, PI * 25.0 * 120.0 / 1000.0, 1e-9);
    assert!(d.power > 0.0 && d.torque > 0.0);
    let tap = Tool { pitch: Some(1.5), ..Tool::new(3, ToolKind::Tap, 10.0, 20.0) };
    assert_eq!(load(&tap, "steel", 300.0, 450.0, 1.0, 1.0), None);
    assert_eq!(load(&mill, "unobtainium", 10_000.0, 900.0, 6.0, 3.0), None);
    for (rpm, feed, ap, ae) in [(0.0, 900.0, 6.0, 3.0), (f64::NAN, 900.0, 6.0, 3.0), (1e4, f64::INFINITY, 6.0, 3.0), (1e4, 900.0, 0.0, 3.0), (1e4, 900.0, 6.0, -1.0)] {
        assert_eq!(load(&mill, "aluminium", rpm, feed, ap, ae), None);
    }
    // A machine names what its spindle has: more is flagged, so are speeds beyond it.
    let mut m = Machine::new("Fräse");
    assert!(m.overload(10_000.0, 900.0, Some(&l)).is_empty());
    m.spindle_power = Some(0.5);
    m.validate().unwrap();
    assert_eq!(m.overload(10_000.0, 900.0, Some(&l)), vec![Overload::Power { needed: l.power, have: 0.5 }]);
    assert!(m.overload(10_000.0, 900.0, Some(&light)).is_empty());
    assert_eq!(
        m.overload(30_000.0, 6000.0, None),
        vec![Overload::SpindleFast { wanted: 30_000.0, max: 24_000.0 }, Overload::Feed { wanted: 6000.0, max: 5000.0 }]
    );
    assert_eq!(m.overload(500.0, 100.0, None), vec![Overload::SpindleSlow { wanted: 500.0, min: 1000.0 }]);
    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(json["spindle_power"], serde_json::json!(0.5));
    assert!(serde_json::to_value(Machine::new("a")).unwrap().get("spindle_power").is_none());
    for bad in [0.0, -1.0, f64::NAN, 2000.0] {
        assert!(Machine { spindle_power: Some(bad), ..Machine::new("a") }.validate().is_err());
    }
}

#[test]
fn tool_kinds_keep_their_sizes_and_refuse_sizes_they_do_not_have() {
    // Every kind reads back by its key, and a file keeps the sizes it was given.
    for kind in ToolKind::ALL {
        assert_eq!(ToolKind::from_key(kind.key()), Some(*kind));
    }
    assert_eq!(ToolKind::from_key("spoon"), None);
    let mut bull = Tool::new(1, ToolKind::BullNose, 10.0, 25.0);
    assert!(bull.validate().is_err(), "a corner-radius end mill needs its radius");
    bull.corner_radius = Some(1.0);
    (bull.overall_length, bull.stick_out, bull.shank_diameter) = (Some(75.0), Some(30.0), Some(10.0));
    bull.holder = Some(ToolHolder { description: "ER25".into(), diameter: 42.0, length: 60.0 });
    bull.validate().unwrap();
    let json = serde_json::to_string(&bull).unwrap();
    assert_eq!(serde_json::from_str::<Tool>(&json).unwrap(), bull);
    assert_eq!(bull.reach(), 30.0);
    // Older tools have none of the new sizes and read as before.
    let old: Tool = serde_json::from_str(r#"{"number":2,"kind":"flat_endmill","diameter":6,"flute_length":20}"#).unwrap();
    assert_eq!((old.holder.clone(), old.reach()), (None, 20.0));
    let mut tap = Tool::new(6, ToolKind::Tap, 10.0, 20.0);
    assert!(tap.validate().is_err(), "a tap needs its pitch");
    tap.pitch = Some(1.5);
    tap.validate().unwrap();
    // A tap feeds one pitch a turn; a reamer and a centre drill feed per revolution like a drill.
    let tapped = CuttingData { vc: 10.0 * PI, fz: 0.1, ap: 1.0, ae: 1.0 }.speeds(&tap, None).unwrap();
    assert_eq!((tapped.spindle_rpm, tapped.feed, tapped.plunge_feed), (1000.0, 1500.0, 1500.0));
    for kind in [ToolKind::CentreDrill, ToolKind::Reamer] {
        let t = Tool::new(3, kind, 10.0, 20.0);
        let d = CuttingData { vc: 10.0 * PI, fz: 0.1, ap: 1.0, ae: 1.0 }.speeds(&t, None).unwrap();
        assert_eq!(d.feed, 100.0, "{kind:?}");
    }
    // Starting values exist for every kind; taps and reamers start slower than a drill.
    for kind in ToolKind::ALL {
        for m in MATERIALS {
            CuttingData::starting(*kind, m, 6.0).unwrap().validate().unwrap();
        }
    }
    let vc = |k: ToolKind| CuttingData::starting(k, "steel", 6.0).unwrap().vc;
    assert!(vc(ToolKind::Tap) < vc(ToolKind::Reamer) && vc(ToolKind::Reamer) < vc(ToolKind::Drill));
    // Sizes a kind does not have, or that do not fit it, are refused.
    let with = |kind: ToolKind, f: &dyn Fn(&mut Tool)| {
        let mut t = Tool::new(4, kind, 10.0, 20.0);
        f(&mut t);
        t.validate()
    };
    assert!(with(ToolKind::FlatEndmill, &|t| t.corner_radius = Some(1.0)).is_err());
    assert!(with(ToolKind::BullNose, &|t| t.corner_radius = Some(6.0)).is_err());
    assert!(with(ToolKind::FlatEndmill, &|t| t.pitch = Some(1.0)).is_err());
    assert!(with(ToolKind::ThreadMill, &|t| t.pitch = Some(0.0)).is_err());
    assert!(with(ToolKind::FlatEndmill, &|t| t.tip_angle = Some(90.0)).is_err());
    assert!(with(ToolKind::Chamfer, &|t| t.tip_angle = Some(180.0)).is_err());
    assert!(with(ToolKind::Chamfer, &|t| t.tip_diameter = Some(10.0)).is_err());
    assert!(with(ToolKind::TSlot, &|t| t.shank_diameter = Some(12.0)).is_err());
    assert!(with(ToolKind::FlatEndmill, &|t| (t.overall_length, t.stick_out) = (Some(40.0), Some(50.0))).is_err());
    assert!(with(ToolKind::FlatEndmill, &|t| t.stick_out = Some(f64::NAN)).is_err());
    let holder = |diameter: f64, length: f64| Some(ToolHolder { description: String::new(), diameter, length });
    assert!(with(ToolKind::FlatEndmill, &|t| t.holder = holder(0.0, 50.0)).is_err());
    assert!(with(ToolKind::FlatEndmill, &|t| t.holder = holder(40.0, f64::INFINITY)).is_err());
    with(ToolKind::Chamfer, &|t| (t.tip_angle, t.tip_diameter) = (Some(90.0), Some(0.5))).unwrap();
}

#[test]
fn machine_limits_hold_speeds_and_refuse_programs_beyond_them() {
    let mut m = Machine::new("Fräse");
    m.validate().unwrap();
    // Speeds are held to the spindle range, the feeds follow and stop at the fastest feed.
    let s = Speeds { spindle_rpm: 30000.0, feed: 3000.0, plunge_feed: 1500.0, stepdown: 1.0, stepover: 2.0 };
    let held = m.hold(s);
    assert_eq!((held.spindle_rpm, held.feed, held.plunge_feed), (24000.0, 2400.0, 1200.0));
    let slow = m.hold(Speeds { spindle_rpm: 500.0, feed: 4000.0, plunge_feed: 100.0, ..s });
    assert_eq!((slow.spindle_rpm, slow.feed, slow.plunge_feed), (1000.0, 5000.0, 200.0));
    // A program is checked by its speed, its feeds and how far it reaches.
    let path = Toolpath::new(vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 5.0) }, Move::Feed { to: Vec3::new(250.0, 10.0, -2.0), feed: 800.0 }]);
    m.check_operation(12000.0, &path).unwrap();
    assert!(m.check_operation(800.0, &path).is_err() && m.check_operation(30000.0, &path).is_err());
    m.max_feed = 600.0;
    assert!(m.check_operation(12000.0, &path).is_err());
    let (lo, hi) = reach(&path).unwrap();
    assert_eq!((lo, hi), ([0.0, 0.0, -2.0], [250.0, 10.0, 5.0]));
    m.check_reach(lo, hi).unwrap();
    m.x = [-100.0, 100.0];
    assert!(m.check_reach(lo, hi).unwrap_err().to_string().contains("along X"));
    // Bad limits are refused.
    for bad in [
        Machine { name: " padded".into(), ..Machine::new("a") },
        Machine { y: [10.0, 10.0], ..Machine::new("a") },
        Machine { z: [f64::NAN, 0.0], ..Machine::new("a") },
        Machine { a: Some(RotaryAxis::Limited { min: 90.0, max: -90.0 }), ..Machine::new("a") },
        Machine { spindle_min: 5000.0, spindle_max: 4000.0, ..Machine::new("a") },
        Machine { max_feed: 0.0, ..Machine::new("a") },
    ] {
        assert!(bad.validate().is_err(), "{bad:?}");
    }
    Machine { a: Some(RotaryAxis::Endless), ..Machine::new("a") }.validate().unwrap();
    let json = serde_json::to_value(Machine { a: Some(RotaryAxis::Endless), ..Machine::new("a") }).unwrap();
    assert_eq!(json["a"], serde_json::json!({"kind": "endless"}));
}

#[test]
fn facing_clears_the_whole_stock_in_passes_and_goes_down_in_the_air() {
    let c = cut(2.0, 0.0, 1.0);
    let params = FaceParams { stepover: 4.0, overrun: 2.0 };
    let path = face(v(0.0, 0.0), v(40.0, 30.0), &tool(6.0), &params, &c).unwrap();
    let cuts = cutting_points(&path, 1.5);
    // Two passes, at 1 and at 0.
    assert!(cuts.iter().any(|p| (p.z - 1.0).abs() < 1e-9) && cuts.iter().any(|p| p.z.abs() < 1e-9));
    assert!(cuts.iter().all(|p| p.z >= -1e-9));
    // Every lane runs from clear of the stock to clear of it: 3 + 2 mm past each end.
    let floor: Vec<&Vec3> = cuts.iter().filter(|p| p.z.abs() < 1e-9).collect();
    assert!(floor.iter().any(|p| (p.x - -5.0).abs() < 1e-9) && floor.iter().any(|p| (p.x - 45.0).abs() < 1e-9));
    // The lanes cover the stock with the tool's edge 2 mm past it, no more than 4 mm apart.
    let mut ys: Vec<f64> = floor.iter().map(|p| p.y).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert!((ys[0] - 1.0).abs() < 1e-9 && (ys[ys.len() - 1] - 29.0).abs() < 1e-9, "{ys:?}");
    assert!(ys.windows(2).all(|w| w[1] - w[0] <= 4.0 + 1e-9));
    // The tool goes down only outside the stock.
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        if let (Some(p), Some(to)) = (pos, m.target())
            && to.z < p.z - 1e-9
            && to.z < 2.0
        {
            assert!(to.x < 0.0 - 3.0 || to.x > 40.0 + 3.0, "goes down over the stock at {to:?}");
        }
        pos = m.target().or(pos);
    }
    // Bad input is refused.
    assert!(face(v(0.0, 0.0), v(0.0, 30.0), &tool(6.0), &params, &c).is_err());
    assert!(face(v(0.0, 0.0), v(40.0, 30.0), &tool(6.0), &FaceParams { stepover: 7.0, ..params }, &c).is_err());
    assert!(face(v(0.0, 0.0), v(40.0, 30.0), &tool(6.0), &FaceParams { overrun: -1.0, ..params }, &c).is_err());
    assert!(face(v(0.0, 0.0), v(40.0, 30.0), &tool(6.0), &params, &cut(0.0, 0.0, 1.0)).is_err());
}

#[test]
fn facing_exact_regions_covers_the_outline_and_goes_down_in_the_air() {
    let c = cut(2.0, 0.0, 1.0);
    let params = FaceParams { stepover: 4.0, overrun: 2.0 };
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let path = face_regions(&regions, &tool(6.0), &params, &c, None).unwrap();
    // Without an entry the first pass starts as `face` does: 5 mm clear of the outline at
    // the near end of the lowest lane.
    let to = path.moves[0].target().unwrap();
    close(to.x, -5.0, 1e-9);
    close(to.y, 1.0, 1e-9);
    close(to.z, 10.0, 1e-9);
    // The disc sweeps every point of the outline: a 1 mm grid lies within the radius of a cut.
    let swept = feed_xy(&path, 0.25);
    let grid: Vec<(f64, f64)> = (1..=39).flat_map(|x| (1..=29).map(move |y| (x as f64, y as f64))).collect();
    assert_eq!(all_covered(&swept, &grid, 3.3), Vec::new(), "points left standing");
    // The lanes end with the tool's edge 2 mm past the outline.
    assert!(swept.iter().any(|p| (p.x + 5.0).abs() < 1e-6) && swept.iter().any(|p| (p.x - 45.0).abs() < 1e-6));
    // Bad input is refused, including entries that are not in air.
    assert!(face_regions(&[], &tool(6.0), &params, &c, None).is_err());
    assert!(face_regions(&regions, &tool(6.0), &FaceParams { stepover: 7.0, ..params }, &c, None).is_err());
    assert!(face_regions(&regions, &tool(6.0), &params, &cut(0.0, 0.0, 1.0), None).is_err());
    assert!(face_regions(&regions, &tool(6.0), &params, &c, Some(v(20.0, 15.0))).is_err());
    assert!(face_regions(&regions, &tool(6.0), &params, &c, Some(v(41.5, 15.0))).is_err());
}

#[test]
fn facing_exact_entry_plunges_in_air_and_feeds_onto_the_first_lane() {
    let c = cut(2.0, 0.0, 1.0);
    let params = FaceParams { stepover: 4.0, overrun: 2.0 };
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let path = face_regions(&regions, &tool(6.0), &params, &c, Some(v(50.0, 40.0))).unwrap();
    // Rapid to the entry, down in the air, then the first motion of the cut is horizontal.
    let first = path.moves[0].target().unwrap();
    close(first.x, 50.0, 1e-9);
    close(first.y, 40.0, 1e-9);
    let plunge = path.moves[2].target().unwrap();
    close(plunge.x, 50.0, 1e-9);
    close(plunge.y, 40.0, 1e-9);
    assert!(plunge.z < 2.0, "the plunge stays above the cut: {plunge:?}");
    let onto = path.moves[3].target().unwrap();
    close(onto.x, 45.0, 1e-9);
    close(onto.y, 29.0, 1e-9);
    close(onto.z, plunge.z, 1e-9);
    // An entry above the outline starts the sweep at the far end of the nearest lane (the
    // stepover here leaves no room for a grazing lane outside it); an entry right of the
    // outline at lane height starts at the far end of the lowest lane, a left one at the near end.
    let path = face_regions(&regions, &tool(6.0), &params, &c, Some(v(50.0, 15.0))).unwrap();
    let onto = path.moves[3].target().unwrap();
    close(onto.x, 45.0, 1e-9);
    close(onto.y, 1.0, 1e-9);
    let path = face_regions(&regions, &tool(6.0), &params, &c, Some(v(-10.0, 15.0))).unwrap();
    let onto = path.moves[3].target().unwrap();
    close(onto.x, -5.0, 1e-9);
    close(onto.y, 1.0, 1e-9);
    // An entry facing no material is refused: on the outline, and inside its tool radius.
    assert!(face_regions(&regions, &tool(6.0), &params, &c, Some(v(40.0, 15.0))).is_err());
}

#[test]
fn facing_exact_entry_beyond_an_edge_ramps_in_at_the_stepover() {
    let c = cut(2.0, 0.0, 1.0);
    let params = FaceParams { stepover: 2.0, overrun: 2.0 };
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let cut_ys = |path: &Toolpath| -> Vec<f64> {
        path.moves
            .iter()
            .filter_map(|m| match m {
                Move::Feed { feed, .. } if (*feed - 200.0).abs() > 1e-9 => Some(m.target().unwrap().y),
                _ => None,
            })
            .collect()
    };
    // Below: one grazing lane at the lowest lane's y minus the stepover (-1; a tool of radius
    // 3 centred below -3 meets no material, so it is the only grazing lane here).
    let path = face_regions(&regions, &tool(6.0), &params, &c, Some(v(25.0, -15.0))).unwrap();
    let onto = path.moves[3].target().unwrap();
    close(onto.x, 45.0, 1e-9);
    close(onto.y, -1.0, 1e-9);
    // Nothing feeds closer to the outline than that grazing lane, and the pattern's own
    // lowest lane follows it.
    let ys = cut_ys(&path);
    assert!(ys.iter().all(|y| *y > -1.0 - 1e-9), "something grazes closer than the outer lane");
    assert!(ys.iter().any(|y| (y + 1.0).abs() < 1e-9) && ys.iter().any(|y| (y - 1.0).abs() < 1e-9));
    // Grazing costs coverage nothing: the disc still sweeps the whole outline.
    let swept = feed_xy(&path, 0.25);
    let grid: Vec<(f64, f64)> = (1..=39).flat_map(|x| (1..=29).map(move |y| (x as f64, y as f64))).collect();
    assert_eq!(all_covered(&swept, &grid, 3.3), Vec::new(), "points left standing");
    // Above: the sweep starts at the entry's end, on the grazing lane over the high edge.
    let path = face_regions(&regions, &tool(6.0), &params, &c, Some(v(25.0, 45.0))).unwrap();
    let onto = path.moves[3].target().unwrap();
    close(onto.x, 45.0, 1e-9);
    close(onto.y, 31.0, 1e-9);
    // Beside the area the feeds stay on the pattern's own lanes: no grazing.
    let path = face_regions(&regions, &tool(6.0), &params, &c, Some(v(50.0, 15.0))).unwrap();
    assert_eq!(cut_ys(&path).into_iter().fold(f64::INFINITY, f64::min), 1.0);
}

#[test]
fn facing_exact_islands_split_lanes_and_the_tool_never_touches_one() {
    let c = cut(0.0, -2.0, 2.0);
    let params = FaceParams { stepover: 3.0, overrun: 1.0 };
    let mut region = rect(0.0, 0.0, 40.0, 30.0);
    region.holes = vec![Loop2::polygon(&[v(15.0, 10.0), v(25.0, 10.0), v(25.0, 20.0), v(15.0, 20.0)])];
    let regions = [region];
    let path = face_regions(&regions, &tool(6.0), &params, &c, None).unwrap();
    let wall = |p: Vec2| {
        let dx = (15.0 - p.x).max(p.x - 25.0).max(0.0);
        let dy = (10.0 - p.y).max(p.y - 20.0).max(0.0);
        (dx * dx + dy * dy).sqrt()
    };
    // No side cut puts the tool's disc on the island, and no link feeds through it either.
    let swept = feed_xy(&path, 0.25);
    assert!(swept.iter().all(|p| wall(*p) >= 3.0 - 0.3), "a lane touched the island");
    // The disc still sweeps the material away from the island's own tool radius.
    let grid: Vec<(f64, f64)> = (1..=39)
        .flat_map(|x| (1..=29).map(move |y| (x as f64, y as f64)))
        .filter(|(x, y)| !(*x >= 15.0 && *x <= 25.0 && *y >= 10.0 && *y <= 20.0) && wall(Vec2::new(*x, *y)) > 1.2)
        .collect();
    assert_eq!(all_covered(&swept, &grid, 3.4), Vec::new(), "points left standing");
    // Lanes broken by the island re-plunge: more than one plunge for one depth pass.
    assert!(path.moves.iter().filter(|m| matches!(m, Move::Feed { feed, .. } if (*feed - 200.0).abs() < 1e-9)).count() > 2);
    // An entry inside an island is refused.
    assert!(face_regions(&regions, &tool(6.0), &params, &c, Some(v(20.0, 15.0))).is_err());
}

#[test]
fn facing_exact_l_shape_enters_at_the_chosen_point_and_covers_both_legs() {
    let c = cut(0.0, -2.0, 2.0);
    let params = FaceParams { stepover: 3.0, overrun: 1.0 };
    let l = polygon(&[(0.0, 0.0), (40.0, 0.0), (40.0, 12.0), (12.0, 12.0), (12.0, 30.0), (0.0, 30.0)]);
    let path = face_regions(std::slice::from_ref(&l), &tool(6.0), &params, &c, Some(v(50.0, 6.0))).unwrap();
    // Down in the air beside the low leg, then a horizontal feed onto the first lane's far end.
    let first = path.moves[0].target().unwrap();
    close(first.x, 50.0, 1e-9);
    close(first.y, 6.0, 1e-9);
    let onto = path.moves[3].target().unwrap();
    close(onto.x, 44.0, 1e-9);
    close(onto.y, 2.0, 1e-9);
    // The disc sweeps both legs up to the outline; the bay's grid points are not required.
    let shape = Shape::from_regions(std::slice::from_ref(&l), 0.005).unwrap();
    let swept = feed_xy(&path, 0.25);
    let grid: Vec<(f64, f64)> =
        (1..=39).flat_map(|x| (1..=29).map(move |y| (x as f64, y as f64))).filter(|(x, y)| shape.inside(Vec2::new(*x, *y), 0.05)).collect();
    assert_eq!(all_covered(&swept, &grid, 3.4), Vec::new(), "points left standing");
    // An entry in the bay's air works too.
    assert!(face_regions(&[l], &tool(6.0), &params, &c, Some(v(30.0, 20.0))).is_ok());
}

/// An open path of straight segments through `pts`.
fn open(pts: &[(f64, f64)]) -> OpenPath {
    OpenPath { segs: pts.windows(2).map(|w| Seg2::Line { a: v(w[0].0, w[0].1), b: v(w[1].0, w[1].1) }).collect() }
}

fn open_params(side: OpenSide, reverse: bool, extend_start: f64, extend_end: f64) -> OpenContourParams {
    OpenContourParams { side, reverse, stock_to_leave: 0.0, tolerance: 0.01, extend_start, extend_end }
}

fn length(pieces: &[Piece]) -> f64 {
    pieces.iter().map(Piece::length).sum()
}

#[test]
fn a_quarter_turn_lead_comes_from_the_free_side_and_joins_the_contour_tangentially() {
    let (s, t, f) = (v(10.0, 5.0), v(1.0, 0.0), v(0.0, 1.0));
    let lead = Lead::Arc { radius: 3.0, sweep: FRAC_PI_2 };
    let piece = lead_in(&lead, s, t, f).unwrap();
    // It ends exactly on the contour, going the way the contour runs.
    close(piece.end().dist(s), 0.0, 1e-9);
    close(piece.tangent_at(1.0).dist(t), 0.0, 1e-9);
    // It starts on the free side, a radius from the centre it turns about.
    assert!((piece.start() - s).dot(f) > 0.0);
    close(piece.start().dist(s), 2.0f64.sqrt() * 3.0, 1e-9);
    // The lead-out is the same arc, leaving along the tangent.
    let out = lead_out(&lead, s, t, f).unwrap();
    close(out.start().dist(s), 0.0, 1e-9);
    close(out.tangent_at(0.0).dist(t), 0.0, 1e-9);
    assert!((out.end() - s).dot(f) > 0.0);
    // A sweep other than a quarter turn still lands on the contour tangentially.
    let half = lead_in(&Lead::Arc { radius: 2.0, sweep: PI }, s, t, f).unwrap();
    close(half.end().dist(s), 0.0, 1e-9);
    close(half.tangent_at(1.0).dist(t), 0.0, 1e-9);
    // The free side decides which way the arc curls.
    let below = lead_in(&lead, s, t, v(0.0, -1.0)).unwrap();
    assert!((below.start() - s).dot(v(0.0, -1.0)) > 0.0, "{:?}", below.start());
}

#[test]
fn tangent_and_perpendicular_leads_run_in_line_and_square_to_the_contour() {
    let (s, t, f) = (v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0));
    let tangent = lead_in(&Lead::Tangent { length: 4.0 }, s, t, f).unwrap();
    close(tangent.length(), 4.0, 1e-12);
    close(tangent.start().dist(v(-4.0, 0.0)), 0.0, 1e-9);
    close(tangent.tangent_at(1.0).dist(t), 0.0, 1e-9);
    let square = lead_in(&Lead::Perpendicular { length: 4.0 }, s, t, f).unwrap();
    close(square.start().dist(v(0.0, 4.0)), 0.0, 1e-9);
    assert!((square.start() - s).dot(f) > 0.0);
    let out = lead_out(&Lead::Perpendicular { length: 4.0 }, s, t, f).unwrap();
    close(out.end().dist(v(0.0, 4.0)), 0.0, 1e-9);
    // Bad leads are refused.
    let bad = [
        Lead::Arc { radius: 0.0, sweep: FRAC_PI_2 },
        Lead::Arc { radius: 1.0, sweep: 0.0 },
        Lead::Arc { radius: 1.0, sweep: PI + 0.1 },
        Lead::Arc { radius: f64::NAN, sweep: FRAC_PI_2 },
        Lead::Tangent { length: -1.0 },
        Lead::Perpendicular { length: f64::INFINITY },
    ];
    for lead in bad {
        assert!(lead.validate().is_err(), "{lead:?}");
    }
    assert!(Lead::Tangent { length: 2.0 }.validate().is_ok());
    assert!(Leads { overlap: -1.0, ..Leads::default() }.validate().is_err());
    assert!(Leads::default().is_direct());
}

#[test]
fn contour_leads_cut_from_and_to_the_free_side_without_touching_the_part() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -3.0, 3.0);
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let lead = Lead::Arc { radius: 3.0, sweep: FRAC_PI_2 };
    let leads = Leads { lead_in: lead, lead_out: lead, overlap: 1.0 };
    let path = contour_with(&regions, &tool(6.0), &params, &leads, &c, None).unwrap();
    verify(&path, &c.heights).unwrap();
    assert!(path.moves.iter().any(|m| matches!(m, Move::Arc { .. })), "the arcs stay arcs");
    // Everything cut at depth keeps the tool's edge off the part: the leads come from outside.
    for p in cutting_points(&path, 0.0) {
        let outside = (p.x.clamp(0.0, 40.0) - p.x).hypot(p.y.clamp(0.0, 30.0) - p.y);
        assert!(outside >= 3.0 - 1e-6, "{p:?} is {outside} mm from the part");
    }
    // The last move of the pass is the lead-out's end, farther out than the contour it left.
    let last_cut = path.moves.iter().rev().find_map(|m| match m {
        Move::Arc { to, .. } | Move::Feed { to, .. } => Some(*to),
        _ => None,
    });
    assert!(last_cut.is_some_and(|p| (p.x.clamp(0.0, 40.0) - p.x).hypot(p.y.clamp(0.0, 30.0) - p.y) > 3.0 + 1e-6));
    // The overlap runs on past the start, so the pass is longer than the loop alone.
    let plain = Leads { lead_in: lead, lead_out: lead, overlap: 0.0 };
    let shorter = contour_with(&regions, &tool(6.0), &params, &plain, &c, None).unwrap();
    close(path.stats().cut_length - shorter.stats().cut_length, 1.0, 1e-6);
}

#[test]
fn a_lead_that_would_cut_into_the_part_is_refused() {
    let c = cut(0.0, -3.0, 3.0);
    let inside = ContourParams { side: Side::Inside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    // A 10 mm opening with the loop 3 mm in from its walls leaves 2 mm to the centre: a 6 mm
    // lead towards the middle runs the tool's edge into the far wall.
    let small = [rect(0.0, 0.0, 10.0, 10.0)];
    let leads = Leads { lead_in: Lead::Perpendicular { length: 6.0 }, ..Leads::default() };
    let err = contour_with(&small, &tool(6.0), &inside, &leads, &c, None).unwrap_err();
    assert!(format!("{err}").contains("the lead-in would cut into the part"), "{err}");
    // The lead-out is checked the same way.
    let leads = Leads { lead_out: Lead::Perpendicular { length: 6.0 }, ..Leads::default() };
    let err = contour_with(&small, &tool(6.0), &inside, &leads, &c, None).unwrap_err();
    assert!(format!("{err}").contains("the lead-out would cut into the part"), "{err}");
    // A roomy opening takes the same lead.
    let roomy = [rect(0.0, 0.0, 60.0, 60.0)];
    let leads = Leads { lead_in: Lead::Perpendicular { length: 6.0 }, ..Leads::default() };
    assert!(contour_with(&roomy, &tool(6.0), &inside, &leads, &c, None).is_ok());
}

#[test]
fn an_open_l_path_offsets_to_each_side_round_its_corners() {
    let l = open(&[(0.0, 0.0), (30.0, 0.0), (30.0, 20.0)]);
    // Left: the corner closes towards the tool, so the offset lines are trimmed to their crossing.
    let left = crate::open::offset_path(&l, &open_params(OpenSide::Left, false, 0.0, 0.0), 3.0).unwrap();
    close(length(&left), 27.0 + 17.0, 1e-9);
    close(left.first().map_or(0.0, |p| p.start().dist(v(0.0, 3.0))), 0.0, 1e-9);
    close(left.last().map_or(0.0, |p| p.end().dist(v(27.0, 20.0))), 0.0, 1e-9);
    // Right: the corner opens away from the tool, so an arc of the tool's radius goes round it.
    let right = crate::open::offset_path(&l, &open_params(OpenSide::Right, false, 0.0, 0.0), 3.0).unwrap();
    close(length(&right), 30.0 + 3.0 * FRAC_PI_2 + 20.0, 1e-9);
    assert!(right.iter().any(|p| matches!(p, Piece::Arc { .. })), "the corner is an arc");
    // On the path itself; the extensions run on along the first and last tangents.
    let on = crate::open::offset_path(&l, &open_params(OpenSide::On, false, 5.0, 2.0), 3.0).unwrap();
    close(length(&on), 30.0 + 20.0 + 7.0, 1e-9);
    close(on.first().map_or(f64::NAN, |p| p.start().dist(v(-5.0, 0.0))), 0.0, 1e-9);
    close(on.last().map_or(f64::NAN, |p| p.end().dist(v(30.0, 22.0))), 0.0, 1e-9);
    // Reversed, the tool cuts the other way, so the same side sees the other corners.
    let reversed_left = crate::open::offset_path(&l, &open_params(OpenSide::Left, true, 0.0, 0.0), 3.0).unwrap();
    close(length(&reversed_left), 20.0 + 3.0 * FRAC_PI_2 + 30.0, 1e-9);
    assert!(reversed_left.iter().any(|p| matches!(p, Piece::Arc { .. })));
    // Stock to leave widens the offset.
    let stock =
        crate::open::offset_path(&l, &OpenContourParams { stock_to_leave: 2.0, ..open_params(OpenSide::Left, false, 0.0, 0.0) }, 3.0).unwrap();
    close(stock.first().map_or(0.0, |p| p.start().dist(v(0.0, 5.0))), 0.0, 1e-9);
}

#[test]
fn an_open_path_that_the_tool_cannot_follow_is_refused() {
    let c = cut(0.0, -3.0, 3.0);
    let params = open_params(OpenSide::Left, false, 0.0, 0.0);
    // An arc tighter than the tool's radius.
    let tight = OpenPath { segs: vec![Seg2::Arc { center: v(0.0, 0.0), radius: 2.0, start: 0.0, sweep: FRAC_PI_2 }] };
    let err = contour_open(&[tight], &tool(6.0), &params, &Leads::default(), &c).unwrap_err();
    assert!(format!("{err}").contains("the tool does not fit the open contour"), "{err}");
    // Segments that do not join end to end.
    let broken = OpenPath { segs: vec![Seg2::Line { a: v(0.0, 0.0), b: v(10.0, 0.0) }, Seg2::Line { a: v(12.0, 0.0), b: v(12.0, 10.0) }] };
    assert!(contour_open(&[broken], &tool(3.0), &params, &Leads::default(), &c).is_err());
    // A chain that closes belongs to the regions.
    let closed = open(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)]);
    assert!(contour_open(&[closed], &tool(3.0), &params, &Leads::default(), &c).is_err());
    // Bad parameters are refused, not panicked on.
    assert!(
        contour_open(&[open(&[(0.0, 0.0), (10.0, 0.0)])], &tool(3.0), &open_params(OpenSide::On, false, -1.0, 0.0), &Leads::default(), &c).is_err()
    );
    assert!(contour_open(&[], &tool(3.0), &params, &Leads::default(), &c).is_err());
}

#[test]
fn an_open_contour_cuts_every_depth_pass_the_same_way() {
    let l = open(&[(0.0, 0.0), (30.0, 0.0), (30.0, 20.0)]);
    let c = cut(0.0, -6.0, 3.0);
    let params = open_params(OpenSide::Left, false, 1.0, 1.0);
    let lead = Lead::Arc { radius: 3.0, sweep: FRAC_PI_2 };
    let leads = Leads { lead_in: lead, lead_out: lead, overlap: 0.0 };
    let path = contour_open(&[l], &tool(6.0), &params, &leads, &c).unwrap();
    verify(&path, &c.heights).unwrap();
    let at = |z: f64| {
        path.points(0.005).into_iter().filter(move |(p, cutting)| *cutting && (p.z - z).abs() < 1e-9).map(|(p, _)| (p.x, p.y)).collect::<Vec<_>>()
    };
    let (first, second) = (at(-3.0), at(-6.0));
    assert!(first.len() > 5, "{first:?}");
    assert_eq!(first.len(), second.len(), "both passes run the same path");
    for (a, b) in first.iter().zip(&second) {
        close(a.0, b.0, 1e-6);
        close(a.1, b.1, 1e-6);
    }
    // Between the passes the tool goes up to the retract height and back to where it plunges.
    let retracts = path.moves.iter().filter(|m| matches!(m, Move::Rapid { to } if (to.z - c.heights.retract).abs() < 1e-9)).count();
    assert!(retracts >= 2, "{retracts} moves at the retract height");
    // The pass is longer than the path alone: four quarter-turn leads (in and out, two passes)
    // are cut too.
    let bare = contour_open(&[open(&[(0.0, 0.0), (30.0, 0.0), (30.0, 20.0)])], &tool(6.0), &params, &Leads::default(), &c).unwrap();
    close(path.stats().cut_length - bare.stats().cut_length, 4.0 * FRAC_PI_2 * 3.0, 1e-6);
}

#[test]
fn an_open_contour_keeps_arcs_as_arcs_and_reads_back() {
    let path = OpenPath {
        segs: vec![
            Seg2::Line { a: v(0.0, 0.0), b: v(20.0, 0.0) },
            Seg2::Arc { center: v(20.0, 10.0), radius: 10.0, start: -FRAC_PI_2, sweep: FRAC_PI_2 },
        ],
    };
    let c = cut(0.0, -3.0, 3.0);
    let params = open_params(OpenSide::Right, false, 0.0, 0.0);
    let toolpath = contour_open(&[path], &tool(6.0), &params, &Leads::default(), &c).unwrap();
    verify(&toolpath, &c.heights).unwrap();
    assert!(toolpath.moves.iter().any(|m| matches!(m, Move::Arc { .. })), "the arc came out as an arc");
    let program = Program {
        name: "Open".into(),
        operations: vec![Operation { name: "Open".into(), tool: tool(6.0), spindle_rpm: 2000.0, coolant: false, path: toolpath }],
        work_offset: None,
    };
    let post = Post::builtin("linuxcnc").unwrap();
    let text = emit(&program, &post).unwrap();
    assert!(text.contains("G3") || text.contains("G2"), "an arc move in the program");
    assert!(readback(&text, &program, &post).unwrap().within());
}

#[test]
fn every_builtin_post_writes_and_reads_back_a_contour_and_a_drill_job() {
    assert!(Post::BUILTIN.windows(2).all(|w| w[0] < w[1]), "the built-in posts stay sorted");
    let contour_path = contour(
        &[rect(0.0, 0.0, 40.0, 30.0)],
        &tool(6.0),
        &ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 },
        &cut(0.0, -4.0, 2.0),
    )
    .unwrap();
    let drill_tool = Tool::new(3, ToolKind::Drill, 5.0, 40.0);
    let drill_path = drill(&[v(10.0, 15.0), v(25.0, 15.0)], &drill_tool, &cut(0.0, -5.0, 2.0), 0.25).unwrap();
    let program = Program {
        name: "Post test".into(),
        operations: vec![
            Operation { name: "Contour".into(), tool: tool(6.0), spindle_rpm: 6000.0, coolant: true, path: contour_path },
            Operation { name: "Drill".into(), tool: drill_tool, spindle_rpm: 3000.0, coolant: false, path: drill_path },
        ],
        work_offset: Some(2),
    };
    // Klartext has no work offset to write: its own test below.
    for name in Post::BUILTIN.iter().filter(|n| **n != "heidenhain") {
        let post = Post::builtin(name).unwrap();
        assert_eq!(post.name, *name);
        post.validate().unwrap();
        let text = emit(&program, &post).unwrap();
        let back = readback(&text, &program, &post).unwrap();
        assert!(back.within(), "{name}: {back:?}");
        // Every preset writes the offset it was given and the dwell in seconds, in its own words.
        assert!(text.contains("G55"), "{name} writes no work offset");
        let dwell = match *name {
            "fanuc" => "G04 X0.25",
            "sinumerik" => "G4 F0.25",
            _ => "G4 P0.25",
        };
        assert!(text.lines().filter(|l| l.contains(dwell)).count() == 2, "{name}: dwells as {dwell}:\n{text}");
        match *name {
            "grbl" => {
                // No tool changer and no tool length offset; the operator is told which tool.
                assert_eq!(post.extension, "nc");
                assert!(!text.contains("M6") && !text.contains("G43"), "GRBL has no M6 or G43:\n{text}");
                assert!(text.contains("(change tool T3)") && text.contains("(change tool T1)"));
            }
            "mach3" => {
                assert_eq!(post.extension, "tap");
                assert!(text.contains("M6 T3") && text.contains("G43 H3"));
            }
            "centroid" => {
                assert!(text.contains("T3 M6") && text.contains("G43 H3"));
            }
            "linuxcnc" => assert!(text.contains("G91.1 G61 G97") && text.contains("T3 M6")),
            "fanuc" => {
                // Between % lines, two-digit codes and a decimal point on every whole number.
                assert!(text.starts_with("(Post test)\n%\nO0001 (Post test)") && text.trim_end().ends_with('%'), "{text}");
                assert!(text.contains("T3 M06") && text.contains("G43 H3") && text.contains("G00 ") && !text.contains("G0 "));
                assert!(text.contains("X40.") || text.contains("Y30."), "whole numbers carry a point:\n{text}");
            }
            "sinumerik" => {
                assert_eq!(post.extension, "mpf");
                assert!(text.contains("G71") && text.contains("T3 D1\nM6") && text.contains("; Contour"), "{text}");
            }
            _ => {}
        }
    }
}

#[test]
fn tapped_holes_are_written_as_rigid_tapping_or_refused() {
    let mut m6 = Tool::new(5, ToolKind::Tap, 6.0, 15.0);
    m6.pitch = Some(1.0);
    let heights = cut(0.0, -10.0, 10.0);
    let path = tap(&[v(10.0, 15.0), v(25.0, 15.0)], &m6, &heights, 500.0).unwrap();
    // In and out at one pitch a turn, from and to the retract height, as one cycle per hole.
    assert_eq!(path.cycles.len(), 2);
    assert!(path.cycles.iter().all(|c| c.tap == Some(1.0) && c.feed == 500.0 && c.peck.is_none()));
    let feeds: Vec<f64> = path.moves.iter().filter_map(Move::feed).collect();
    assert_eq!(feeds, [500.0; 4]);
    let program = Program {
        name: "Tap".into(),
        operations: vec![Operation { name: "M6".into(), tool: m6.clone(), spindle_rpm: 500.0, coolant: false, path: path.clone() }],
        work_offset: None,
    };
    for name in Post::BUILTIN {
        let post = Post::builtin(name).unwrap();
        let written = emit(&program, &post);
        let Ok(text) = written else {
            // GRBL and plasma tables have no tapping cycle, Heidenhain's 207 is not written yet:
            // the program is refused, never written as plain moves.
            assert!(matches!(*name, "grbl" | "heidenhain" | "plasma") && post.tapping == Tapping::None, "{name}: {written:?}");
            continue;
        };
        let back = readback(&text, &program, &post).unwrap();
        assert!(back.within(), "{name}: {back:?}\n{text}");
        assert!((back.cut_length - back.planned_cut_length).abs() < 1e-6, "{name}: {back:?}");
        let block = match post.tapping {
            Tapping::G33_1 => "G33.1 Z-10 K1",
            Tapping::G84 => "G98 G84 X10. Y15. Z-10. R",
            Tapping::G84Rigid => "M29 S500\nG98 G84 X10. Y15. Z-10. R",
            Tapping::G331 => "SPOS=0\nG331 Z-10 K1 S500\nG332 Z",
            Tapping::None => continue,
        };
        assert!(text.contains(block), "{name} writes {block}:\n{text}");
    }
    // A tap needs its pitch and a spindle speed; the moves of a tapped hole are never written alone.
    assert!(tap(&[v(0.0, 0.0)], &Tool::new(5, ToolKind::Drill, 6.0, 15.0), &heights, 500.0).is_err());
    assert!(tap(&[v(0.0, 0.0)], &m6, &heights, 0.0).is_err());
    assert!(tap(&[v(0.0, 0.0)], &m6, &cut(0.0, -20.0, 20.0), 500.0).is_err(), "deeper than the thread length");
    let mut broken = path;
    broken.cycles[0].at = v(11.0, 15.0);
    let program = Program {
        name: "Tap".into(),
        operations: vec![Operation { name: "M6".into(), tool: m6, spindle_rpm: 500.0, coolant: false, path: broken }],
        work_offset: None,
    };
    assert!(emit(&program, &Post::linuxcnc()).is_err());
}

#[test]
fn tabs_lift_the_tool_over_them_on_the_passes_below_them() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 3.0);
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    // Four tabs 2 mm long, keeping 2 mm of the wall above the -6 mm bottom: their top is at -4.
    let tabs = Tabs { count: 4, spacing: 0.0, width: 2.0, height: 2.0 };
    let path = contour_with_tabs(&regions, &tool(6.0), &params, &Leads::default(), &tabs, &c, None).unwrap();
    verify(&path, &c.heights).unwrap();
    let lifts = path.moves.iter().filter(|m| matches!(m, Move::Rapid { to } if (to.z + 4.0).abs() < 1e-9)).count();
    assert_eq!(lifts, 4, "one lift over each tab on the lower pass");
    // The upper pass is above the tabs' top, so it cuts the whole loop at -3.
    let loop_len = 140.0 + 6.0 * PI;
    // Length cut at `z`, counting only moves that stay at that height (the lift over a tab is not
    // part of it).
    let cut_at = |z: f64| {
        let pts = path.points(0.001);
        pts.windows(2)
            .filter(|w| (w[0].0.z - z).abs() < 1e-9 && (w[1].0.z - z).abs() < 1e-9)
            .map(|w| (w[1].0.x - w[0].0.x).hypot(w[1].0.y - w[0].0.y))
            .sum::<f64>()
    };
    close(cut_at(-3.0), loop_len, 0.01);
    // The lower pass leaves the four 2 mm tabs uncut below their top.
    close(cut_at(-6.0), loop_len - 4.0 * 2.0, 0.01);
    // Each tab is crossed at its top and the tool goes back down behind it.
    close(cut_at(-4.0), 4.0 * 2.0, 0.01);
    // Spacing instead of a count: a 158.8 mm loop takes 7 tabs 20 mm apart.
    let spaced = Tabs { count: 0, spacing: 20.0, width: 2.0, height: 2.0 };
    let path = contour_with_tabs(&regions, &tool(6.0), &params, &Leads::default(), &spaced, &c, None).unwrap();
    verify(&path, &c.heights).unwrap();
    let lifts = path.moves.iter().filter(|m| matches!(m, Move::Rapid { to } if (to.z + 4.0).abs() < 1e-9)).count();
    assert_eq!(lifts, 7);
}

#[test]
fn tabs_that_do_not_fit_are_refused() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 3.0);
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let run = |tabs: Tabs| contour_with_tabs(&regions, &tool(6.0), &params, &Leads::default(), &tabs, &c, None);
    // Wider than the gap between them.
    let err = run(Tabs { count: 200, spacing: 0.0, width: 2.0, height: 2.0 }).unwrap_err();
    assert!(format!("{err}").contains("the tab width must be at most"), "{err}");
    // Taller than the cut.
    let err = run(Tabs { count: 4, spacing: 0.0, width: 2.0, height: 6.0 }).unwrap_err();
    assert!(format!("{err}").contains("the tab height must be less than the depth"), "{err}");
    // Nothing to leave behind.
    assert!(run(Tabs { count: 4, spacing: 0.0, width: 0.0, height: 2.0 }).is_err());
    assert!(run(Tabs { count: 4, spacing: 0.0, width: 2.0, height: 0.0 }).is_err());
    assert!(run(Tabs { count: 4, spacing: 0.0, width: f64::NAN, height: 2.0 }).is_err());
    // No tabs at all is fine, by count or by spacing.
    assert!(run(Tabs::default()).is_ok());
    assert!(run(Tabs { count: 0, spacing: 0.0, width: 2.0, height: 2.0 }).is_ok());
    assert!(Tabs { count: 0, spacing: 0.0, width: 2.0, height: 2.0 }.is_empty());
    assert!(!Tabs { count: 0, spacing: 20.0, width: 2.0, height: 2.0 }.is_empty());
}

mod sim_tests {
    use super::*;

    /// The stock top of the cell nearest to (x, y).
    fn at(stock: &StockMap, x: f64, y: f64) -> f64 {
        let (nx, ny) = stock.size();
        let i = (((x - stock.origin().x) / stock.cell()).floor() as isize).clamp(0, nx as isize - 1) as usize;
        let j = (((y - stock.origin().y) / stock.cell()).floor() as isize).clamp(0, ny as isize - 1) as usize;
        stock.height(i, j).unwrap_or(f64::NAN)
    }

    /// The centre of the cell nearest to (x, y).
    fn centre(stock: &StockMap, x: f64, y: f64) -> Vec2 {
        let (nx, ny) = stock.size();
        let i = (((x - stock.origin().x) / stock.cell()).floor() as isize).clamp(0, nx as isize - 1) as usize;
        let j = (((y - stock.origin().y) / stock.cell()).floor() as isize).clamp(0, ny as isize - 1) as usize;
        Vec2::new(stock.origin().x + (i as f64 + 0.5) * stock.cell(), stock.origin().y + (j as f64 + 0.5) * stock.cell())
    }

    #[test]
    fn a_pocket_leaves_its_floor_and_the_stock_outside() {
        let cutter = tool(6.0);
        let regions = [rect(0.0, 0.0, 40.0, 30.0)];
        let params = PocketParams { stepover: 3.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
        let c = cut(0.0, -5.0, 2.5);
        let path = pocket(&regions, &cutter, &params, &c).unwrap();
        let mut stock = StockMap::new_box(Vec3::new(-5.0, -5.0, -5.0), Vec3::new(45.0, 35.0, 0.0), 0.5).unwrap();
        let report = stock.cut(&cutter, &path, Vec3::new(20.0, 15.0, 10.0)).unwrap();
        assert!(report.rapid_hits.is_empty(), "{:?}", report.rapid_hits);
        assert!(report.removed > 0.0);
        close(at(&stock, 20.0, 15.0), -5.0, 1e-5);
        // Well outside the pocket the stock keeps its top.
        close(at(&stock, 44.0, 34.0), 0.0, 1e-6);
    }

    #[test]
    fn a_contour_around_a_box_leaves_the_walls_standing() {
        let cutter = tool(6.0);
        let regions = [rect(5.0, 5.0, 35.0, 25.0)];
        let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
        let c = cut(0.0, -5.0, 2.5);
        let path = contour(&regions, &cutter, &params, &c).unwrap();
        let mut stock = StockMap::new_box(Vec3::new(-5.0, -5.0, -5.0), Vec3::new(45.0, 35.0, 0.0), 0.5).unwrap();
        stock.cut(&cutter, &path, Vec3::new(2.0, 15.0, 10.0)).unwrap();
        // Under the pass outside the part the stock is cut down.
        close(at(&stock, 2.0, 15.0), -5.0, 1e-5);
        // The part itself and the stock far outside keep their top.
        close(at(&stock, 20.0, 15.0), 0.0, 1e-6);
        close(at(&stock, 44.0, 15.0), 0.0, 1e-6);
    }

    #[test]
    fn a_drill_leaves_a_cone_tip() {
        let bit = Tool::new(2, ToolKind::Drill, 6.0, 20.0);
        let c = cut(0.0, -6.0, 3.0);
        let path = drill(&[v(20.0, 20.0)], &bit, &c, 0.0).unwrap();
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -20.0), Vec3::new(40.0, 40.0, 0.0), 0.25).unwrap();
        stock.cut(&bit, &path, Vec3::new(20.0, 20.0, 10.0)).unwrap();
        let cot = 1.664_279_482_350_9;
        let near = centre(&stock, 20.0, 20.0);
        let d = near.dist(v(20.0, 20.0));
        assert!(d < 0.25, "{d}");
        close(at(&stock, 20.0, 20.0), -6.0 + d / cot, 1e-4);
        let side = centre(&stock, 21.5, 20.0);
        let d = side.dist(v(20.0, 20.0));
        assert!(d > 1.0 && d < 3.0, "{d}");
        close(at(&stock, 21.5, 20.0), -6.0 + d / cot, 1e-4);
        // Beyond the drill's radius the stock is untouched.
        close(at(&stock, 24.0, 20.0), 0.0, 1e-6);
    }

    #[test]
    fn a_ball_end_mill_leaves_a_round_groove() {
        let ball = Tool::new(3, ToolKind::BallEndmill, 6.0, 20.0);
        let path = Toolpath::new(vec![
            Move::Rapid { to: Vec3::new(5.0, 20.0, 10.0) },
            Move::Feed { to: Vec3::new(5.0, 20.0, -1.0), feed: 600.0 },
            Move::Feed { to: Vec3::new(35.0, 20.0, -1.0), feed: 600.0 },
            Move::Rapid { to: Vec3::new(35.0, 20.0, 10.0) },
        ]);
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, 0.0), 0.25).unwrap();
        let report = stock.cut(&ball, &path, Vec3::new(5.0, 20.0, 10.0)).unwrap();
        assert!(report.rapid_hits.is_empty(), "{:?}", report.rapid_hits);
        let r = 3.0;
        for (x, y) in [(20.0, 20.0), (20.0, 21.5), (20.0, 21.75)] {
            let p = centre(&stock, x, y);
            let d = (p.y - 20.0).abs();
            let want = -1.0 + r - (r * r - d * d).max(0.0).sqrt();
            close(at(&stock, x, y), want, 1e-4);
        }
        // Where the ball's surface sits above the stock's top, nothing is cut: the groove is
        // 2.236 mm wide at the top and the stock beyond it keeps its height.
        close(at(&stock, 20.0, 22.5), 0.0, 1e-6);
        close(at(&stock, 20.0, 24.0), 0.0, 1e-6);
    }

    #[test]
    fn a_chamfer_cutter_and_a_corner_radius_end_mill_leave_their_shapes() {
        // 90 degree chamfer cutter with a 1 mm flat, plunged 2 mm: flat in the middle, then a 45 degree wall.
        let mut chamfer = Tool::new(5, ToolKind::Chamfer, 12.0, 6.0);
        (chamfer.tip_angle, chamfer.tip_diameter) = (Some(90.0), Some(1.0));
        let plunge = |at: Vec3| {
            Toolpath::new(vec![
                Move::Rapid { to: at + Vec3::new(0.0, 0.0, 10.0) },
                Move::Feed { to: at, feed: 300.0 },
                Move::Rapid { to: at + Vec3::new(0.0, 0.0, 10.0) },
            ])
        };
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, 0.0), 0.25).unwrap();
        stock.cut(&chamfer, &plunge(Vec3::new(20.0, 20.0, -2.0)), Vec3::new(20.0, 20.0, 10.0)).unwrap();
        for (x, y) in [(20.0, 20.0), (21.5, 20.0), (20.0, 22.0)] {
            let d = centre(&stock, x, y).dist(v(20.0, 20.0));
            close(at(&stock, x, y), -2.0 + (d - 0.5).max(0.0), 1e-4);
        }
        close(at(&stock, 23.0, 20.0), 0.0, 1e-6);
        // Corner radius 2 on a 10 mm end mill: flat to 3 mm out, then a quarter round.
        let mut bull = Tool::new(6, ToolKind::BullNose, 10.0, 20.0);
        bull.corner_radius = Some(2.0);
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, 0.0), 0.25).unwrap();
        stock.cut(&bull, &plunge(Vec3::new(20.0, 20.0, -3.0)), Vec3::new(20.0, 20.0, 10.0)).unwrap();
        for (x, y) in [(20.0, 20.0), (22.5, 20.0), (24.0, 20.0), (20.0, 24.5)] {
            let e = centre(&stock, x, y).dist(v(20.0, 20.0)) - 3.0;
            let want = if e <= 0.0 { -3.0 } else { -3.0 + 2.0 - (4.0 - e * e).max(0.0).sqrt() };
            close(at(&stock, x, y), want, 1e-4);
        }
        close(at(&stock, 25.5, 20.0), 0.0, 1e-6);
    }

    #[test]
    fn a_rapid_through_stock_is_reported_and_removes_nothing() {
        let cutter = tool(4.0);
        let path = Toolpath::new(vec![Move::Rapid { to: Vec3::new(15.0, 10.0, 2.0) }]);
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 5.0), 0.5).unwrap();
        let report = stock.cut(&cutter, &path, Vec3::new(5.0, 10.0, 8.0)).unwrap();
        assert_eq!(report.rapid_hits, vec![0]);
        assert_eq!(report.removed, 0.0);
        close(at(&stock, 10.0, 10.0), 5.0, 1e-6);
    }

    #[test]
    fn a_cut_deeper_than_the_flutes_is_reported() {
        let short = Tool::new(1, ToolKind::FlatEndmill, 6.0, 5.0);
        let path = Toolpath::new(vec![Move::Rapid { to: Vec3::new(10.0, 10.0, 2.0) }, Move::Feed { to: Vec3::new(10.0, 10.0, -8.0), feed: 200.0 }]);
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -20.0), Vec3::new(20.0, 20.0, 0.0), 0.25).unwrap();
        let report = stock.cut(&short, &path, Vec3::new(10.0, 10.0, 2.0)).unwrap();
        assert_eq!(report.too_deep, vec![1]);
        close(at(&stock, 10.0, 10.0), -8.0, 1e-5);
    }

    #[test]
    fn the_removed_volume_of_a_faced_stock_is_within_two_percent() {
        let cutter = tool(6.0);
        let params = FaceParams { stepover: 6.0, overrun: 0.0 };
        let c = cut(0.0, -4.0, 2.0);
        let path = face(v(0.0, 0.0), v(40.0, 30.0), &cutter, &params, &c).unwrap();
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -4.0), Vec3::new(40.0, 30.0, 0.0), 0.5).unwrap();
        let report = stock.cut(&cutter, &path, Vec3::new(-3.0, 3.0, 10.0)).unwrap();
        assert!(report.rapid_hits.is_empty(), "{:?}", report.rapid_hits);
        let expected = 40.0 * 30.0 * 4.0;
        assert!((report.removed - expected).abs() <= expected * 0.02, "{} is not within 2 % of {expected}", report.removed);
        close(at(&stock, 20.0, 15.0), -4.0, 1e-5);
    }

    #[test]
    fn the_mesh_never_exceeds_its_triangle_budget() {
        let stock = StockMap::new_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 5.0), 0.5).unwrap();
        for budget in [0usize, 8, 10, 50, 1000, 100_000] {
            let (positions, normals, indices) = stock.mesh(budget);
            assert!(indices.len() / 3 <= budget, "budget {budget}: {} triangles", indices.len() / 3);
            if budget < 10 {
                assert!(indices.is_empty(), "budget {budget}");
            } else {
                assert!(!indices.is_empty(), "budget {budget}");
            }
            assert_eq!(positions.len(), normals.len());
            for p in &positions {
                assert!(p.iter().all(|v| v.is_finite()));
                assert!((-1e-3..=20.001).contains(&p[0]) && (-1e-3..=20.001).contains(&p[1]), "{p:?}");
            }
            for n in &normals {
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                close(len as f64, 1.0, 1e-5);
            }
            for i in &indices {
                assert!((*i as usize) < positions.len());
            }
        }
    }

    #[test]
    fn rest_material_is_what_stands_above_the_target() {
        let cutter = tool(4.0);
        let path = Toolpath::new(vec![
            Move::Rapid { to: Vec3::new(5.0, 5.0, 5.0) },
            Move::Feed { to: Vec3::new(5.0, 5.0, 3.0), feed: 100.0 },
            Move::Rapid { to: Vec3::new(5.0, 5.0, 8.0) },
        ]);
        let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 5.0), 1.0).unwrap();
        stock.cut(&cutter, &path, Vec3::new(5.0, 5.0, 5.0)).unwrap();
        let rest = stock.rest(&|_| Some(4.0));
        assert_eq!(rest.rest.len(), 100);
        assert!(rest.max_at.is_some());
        close(rest.max, 1.0, 1e-6);
        let none = stock.rest(&|_| None);
        assert_eq!(none.max, 0.0);
        assert!(none.max_at.is_none());
    }

    #[test]
    fn a_round_bar_has_no_stock_outside_its_circle() {
        let stock = StockMap::new_round(v(0.0, 0.0), 10.0, 0.0, 5.0, 1.0).unwrap();
        assert_eq!(stock.size(), (20, 20));
        close(at(&stock, 0.0, 0.0), 5.0, 1e-6);
        close(at(&stock, 9.0, 9.0), 0.0, 1e-6);
    }

    #[test]
    fn hostile_stock_and_moves_are_refused_never_panic() {
        let good = Vec3::new(0.0, 0.0, 0.0);
        let top = Vec3::new(10.0, 10.0, 5.0);
        for &b in &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300] {
            assert!(StockMap::new_box(Vec3::new(b, 0.0, 0.0), top, 1.0).is_err());
            assert!(StockMap::new_box(good, Vec3::new(10.0, 10.0, b), 1.0).is_err());
            assert!(StockMap::new_round(v(0.0, 0.0), b, 0.0, 5.0, 1.0).is_err());
            assert!(StockMap::new_round(v(0.0, 0.0), 5.0, b, 5.0, 1.0).is_err());
            assert!(StockMap::new_round(v(b, 0.0), 5.0, 0.0, 5.0, 1.0).is_err());
        }
        for &b in &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e-300] {
            assert!(StockMap::new_box(good, top, b).is_err());
        }
        // Empty boxes and a zero radius.
        assert!(StockMap::new_box(good, Vec3::new(0.0, 10.0, 5.0), 1.0).is_err());
        assert!(StockMap::new_box(good, Vec3::new(10.0, 10.0, 0.0), 1.0).is_err());
        assert!(StockMap::new_round(v(0.0, 0.0), 0.0, 0.0, 5.0, 1.0).is_err());
        assert!(StockMap::new_round(v(0.0, 0.0), 5.0, 5.0, 5.0, 1.0).is_err());
        // Too fine a grid.
        let err = StockMap::new_box(good, Vec3::new(1e6, 1e6, 5.0), 0.01).unwrap_err();
        assert!(format!("{err}").contains("too fine"), "{err}");

        let cutter = tool(2.0);
        let mut stock = StockMap::new_box(good, top, 1.0).unwrap();
        for &b in &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
            let path = Toolpath::new(vec![
                Move::Feed { to: Vec3::new(b, 0.0, -1.0), feed: 100.0 },
                Move::Arc { to: Vec3::new(1.0, 1.0, 0.0), center: v(b, b), ccw: true, feed: 100.0 },
            ]);
            let _ = stock.cut(&cutter, &path, Vec3::new(0.0, 0.0, 1.0));
            let _ = stock.cut(&cutter, &Toolpath::new(vec![Move::Rapid { to: Vec3::new(1.0, 1.0, 1.0) }]), Vec3::new(b, 0.0, 0.0));
        }
        // A zero-radius arc is refused, not divided by zero.
        let err = stock
            .cut(
                &cutter,
                &Toolpath::new(vec![Move::Arc { to: Vec3::new(1.0, 1.0, 0.0), center: v(0.0, 0.0), ccw: true, feed: 100.0 }]),
                Vec3::new(0.0, 0.0, 0.0),
            )
            .unwrap_err();
        assert!(format!("{err}").contains("no radius"), "{err}");
    }
}

mod time_tests {
    use super::*;

    fn model() -> TimeModel {
        TimeModel { rapid_feed: 3000.0, max_feed: [10_000.0; 3], accel: [500.0; 3], tool_change: 8.0 }
    }

    fn feed(x: f64, y: f64, f: f64) -> Move {
        Move::Feed { to: Vec3::new(x, y, 0.0), feed: f }
    }

    fn op(number: u32, moves: Vec<Move>) -> Operation {
        Operation {
            name: format!("T{number}"),
            tool: Tool::new(number, ToolKind::FlatEndmill, 6.0, 20.0),
            spindle_rpm: 10_000.0,
            coolant: false,
            path: Toolpath::new(moves),
        }
    }

    fn program(ops: Vec<Operation>) -> Program {
        Program { name: "t".into(), operations: ops, work_offset: None }
    }

    #[test]
    fn a_long_line_takes_its_length_over_the_feed_plus_one_ramp() {
        // 1000 mm at 10 mm/s with 500 mm/s²: t = d/v + v/a.
        let est = estimate(&program(vec![op(1, vec![feed(0.0, 0.0, 600.0), feed(1000.0, 0.0, 600.0)])]), &model()).unwrap();
        close(est.cutting, 100.0 + 10.0 / 500.0, 1e-9);
        close(est.total, est.cutting, 1e-12);
        assert_eq!(est.tool_changes, 0.0);
    }

    #[test]
    fn rapids_run_at_the_rapid_feed_and_short_moves_never_reach_it() {
        // Trapezoid: 100 mm at 50 mm/s.
        let rapid = |x: f64| Move::Rapid { to: Vec3::new(x, 0.0, 0.0) };
        let est = estimate(&program(vec![op(1, vec![rapid(0.0), rapid(100.0)])]), &model()).unwrap();
        close(est.rapid, 100.0 / 50.0 + 50.0 / 500.0, 1e-9);
        assert_eq!(est.cutting, 0.0);
        // Triangle: 1 mm wanting 100 mm/s peaks at sqrt(a·d) after 2·sqrt(d/a).
        let times = move_times(&[feed(1.0, 0.0, 6000.0)], Vec3::default(), &model()).unwrap();
        close(times[0].seconds, 2.0 * (1.0f64 / 500.0).sqrt(), 1e-9);
        close(times[0].peak, 500.0f64.sqrt() * 60.0, 1e-6);
        close(times[0].length, 1.0, 1e-12);
        // Nothing to go: no time.
        let none = move_times(&[feed(0.0, 0.0, 600.0)], Vec3::default(), &model()).unwrap();
        assert_eq!(none[0], MoveTime::default());
    }

    #[test]
    fn corners_stop_the_tool_but_straight_pieces_run_on() {
        let f = 3000.0;
        // 100 short pieces of 1 mm, all along X: one run, close to one long line.
        let straight: Vec<Move> = (0..=100).map(|i| feed(f64::from(i), 0.0, f)).collect();
        let s: f64 = move_times(&straight, Vec3::default(), &model()).unwrap().iter().map(|t| t.seconds).sum();
        close(s, 100.0 / 50.0 + 50.0 / 500.0, 1e-9);
        // The same 100 mm as a staircase of 90° corners: the tool stops at every one.
        let stairs: Vec<Move> = (0..=100).map(|i| feed(f64::from((i + 1) / 2), f64::from(i / 2), f)).collect();
        let t: f64 = move_times(&stairs, Vec3::default(), &model()).unwrap().iter().map(|t| t.seconds).sum();
        assert!(t > 2.5 * (100.0 / 50.0), "{t}");
        close(t, 100.0 * 2.0 * (1.0f64 / 500.0).sqrt(), 1e-9);
        // A feed change stops the tool too.
        let changing = [feed(50.0, 0.0, f), feed(100.0, 0.0, f / 2.0)];
        let c: f64 = move_times(&changing, Vec3::default(), &model()).unwrap().iter().map(|t| t.seconds).sum();
        close(c, (50.0 / 50.0 + 50.0 / 500.0) + (50.0 / 25.0 + 25.0 / 500.0), 1e-9);
    }

    #[test]
    fn axis_limits_clamp_the_feed_and_the_acceleration() {
        let slow = TimeModel { max_feed: [1200.0, 10_000.0, 10_000.0], accel: [500.0, 100.0, 500.0], ..model() };
        // Along X the feed is clamped to 20 mm/s.
        let along = move_times(&[feed(100.0, 0.0, 6000.0)], Vec3::default(), &slow).unwrap();
        close(along[0].seconds, 100.0 / 20.0 + 20.0 / 500.0, 1e-9);
        close(along[0].peak, 1200.0, 1e-6);
        // At 45° Y's 100 mm/s² holds the path to 100 / cos 45°.
        let a = 100.0 / std::f64::consts::FRAC_1_SQRT_2;
        let d = 2.0f64.sqrt();
        let diagonal = move_times(&[feed(1.0, 1.0, 1000.0)], Vec3::default(), &slow).unwrap();
        close(diagonal[0].seconds, 2.0 * (d / a).sqrt(), 1e-9);
    }

    #[test]
    fn arcs_are_timed_by_their_length_and_held_to_their_bend() {
        // A quarter circle of radius 10 at 10 mm/s.
        let arc = Move::Arc { to: Vec3::new(0.0, 10.0, 0.0), center: Vec2::new(0.0, 0.0), ccw: true, feed: 600.0 };
        let t = move_times(&[arc], Vec3::new(10.0, 0.0, 0.0), &model()).unwrap();
        let length = 10.0 * std::f64::consts::FRAC_PI_2;
        close(t[0].length, length, 1e-9);
        close(t[0].seconds, length / 10.0 + 10.0 / 500.0, 1e-9);
        // A line running on tangent to the arc's end does not stop the tool.
        let on = move_times(&[arc, feed(-20.0, 10.0, 600.0)], Vec3::new(10.0, 0.0, 0.0), &model()).unwrap();
        close(on[0].seconds + on[1].seconds, (length + 20.0) / 10.0 + 10.0 / 500.0, 1e-9);
        // A tight arc at a high feed is slowed to sqrt(a·r).
        let tight = Move::Arc { to: Vec3::new(-0.5, 0.0, 0.0), center: Vec2::new(0.0, 0.0), ccw: true, feed: 6000.0 };
        let t = move_times(&[tight], Vec3::new(0.5, 0.0, 0.0), &model()).unwrap();
        assert!(t[0].peak <= (500.0f64 * 0.5).sqrt() * 60.0 + 1e-6, "{}", t[0].peak);
    }

    #[test]
    fn tool_changes_and_dwells_add_up_per_operation() {
        let first = op(1, vec![feed(0.0, 0.0, 600.0), feed(100.0, 0.0, 600.0), Move::Dwell { seconds: 1.5 }]);
        let same = op(1, vec![feed(0.0, 0.0, 600.0), feed(0.0, 100.0, 600.0)]);
        let other = op(2, vec![feed(0.0, 0.0, 600.0), feed(100.0, 0.0, 600.0)]);
        let est = estimate(&program(vec![first, same, other]), &model()).unwrap();
        assert_eq!(est.tool_changes, 8.0);
        assert_eq!(est.dwell, 1.5);
        let line = 100.0 / 10.0 + 10.0 / 500.0;
        assert_eq!(est.per_operation.len(), 3);
        close(est.per_operation[0], line + 1.5, 1e-9);
        close(est.per_operation[1], line, 1e-9);
        close(est.per_operation[2], 8.0 + line, 1e-9);
        close(est.per_operation.iter().sum::<f64>(), est.total, 1e-9);
        close(est.total, est.cutting + est.rapid + est.dwell + est.tool_changes, 1e-12);
    }

    #[test]
    fn hostile_time_values_are_refused() {
        let good = program(vec![op(1, vec![feed(0.0, 0.0, 600.0), feed(10.0, 0.0, 600.0)])]);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
            assert!(estimate(&good, &TimeModel { rapid_feed: bad, ..model() }).is_err(), "rapid {bad}");
            assert!(estimate(&good, &TimeModel { max_feed: [600.0, bad, 600.0], ..model() }).is_err(), "feed {bad}");
            assert!(estimate(&good, &TimeModel { accel: [500.0, 500.0, bad], ..model() }).is_err(), "accel {bad}");
            if bad != 0.0 {
                assert!(estimate(&good, &TimeModel { tool_change: bad, ..model() }).is_err(), "change {bad}");
            }
            let path = [feed(10.0, 0.0, bad)];
            assert!(move_times(&path, Vec3::default(), &model()).is_err(), "move feed {bad}");
            assert!(move_times(&[Move::Dwell { seconds: -bad.abs() - 1.0 }], Vec3::default(), &model()).is_err());
            assert!(move_times(&[feed(f64::NAN, 0.0, 600.0)], Vec3::default(), &model()).is_err());
            assert!(move_times(&[feed(1.0, 0.0, 600.0)], Vec3::new(f64::INFINITY, 0.0, 0.0), &model()).is_err());
        }
        assert!(estimate(&good, &TimeModel { tool_change: 0.0, ..model() }).is_ok());
    }
}

#[test]
fn a_helically_bored_hole_is_round_and_flat_and_reads_back_for_every_post() {
    let mill = tool(10.0);
    let heights = cut(0.0, -12.0, 2.0);
    let path = helical_bore(&[v(20.0, 20.0)], &mill, &HelixParams { diameter: 16.0, climb: true, ..Default::default() }, &heights).unwrap();
    // Down the helix 2 mm a turn, one turn at the bottom, out through the middle.
    let arcs: Vec<(Vec3, bool)> =
        path.moves.iter().filter_map(|m| if let Move::Arc { to, ccw, .. } = m { Some((*to, *ccw)) } else { None }).collect();
    assert_eq!(arcs.len(), 12 + 2);
    assert!(arcs.iter().all(|(to, ccw)| *ccw && (Vec2::new(to.x, to.y).dist(v(20.0, 20.0)) - 3.0).abs() < 1e-9));
    assert!(arcs.windows(2).all(|w| w[1].0.z <= w[0].0.z + 1e-9), "the helix only goes down");
    // The stock keeps a 16 mm hole, 12 deep with a flat floor, and nothing outside it.
    let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -20.0), Vec3::new(40.0, 40.0, 0.0), 0.25).unwrap();
    let report = stock.cut(&mill, &path, Vec3::new(20.0, 20.0, 10.0)).unwrap();
    assert!(report.rapid_hits.is_empty(), "{report:?}");
    for (x, y) in [(20.0, 20.0), (27.5, 20.0), (20.0, 12.5)] {
        close(sim_height(&stock, x, y), -12.0, 1e-6);
    }
    close(sim_height(&stock, 28.5, 20.0), 0.0, 1e-6);
    let program = Program {
        name: "Bore".into(),
        operations: vec![Operation { name: "Bore".into(), tool: mill.clone(), spindle_rpm: 8000.0, coolant: false, path }],
        work_offset: None,
    };
    for name in Post::BUILTIN {
        let post = Post::builtin(name).unwrap();
        let text = emit(&program, &post).unwrap();
        assert!(readback(&text, &program, &post).unwrap().within(), "{name}");
    }
    // Too narrow, or so wide a core would stand; a drill cannot mill.
    for d in [10.0, 21.0, f64::NAN] {
        assert!(helical_bore(&[v(0.0, 0.0)], &mill, &HelixParams { diameter: d, climb: true, ..Default::default() }, &heights).is_err(), "{d}");
    }
    let drill = Tool::new(2, ToolKind::Drill, 6.0, 30.0);
    assert!(helical_bore(&[v(0.0, 0.0)], &drill, &HelixParams { diameter: 10.0, climb: true, ..Default::default() }, &heights).is_err());
    // A pitch of its own replaces the stepdown: 4 mm a turn is 3 turns down 12 mm.
    let steep = helical_bore(&[v(20.0, 20.0)], &mill, &HelixParams { diameter: 16.0, climb: true, pitch: Some(4.0), outside: false }, &heights).unwrap();
    assert_eq!(steep.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count(), 6 + 2);
    for pitch in [0.0, -1.0, f64::NAN] {
        let params = HelixParams { diameter: 16.0, climb: true, pitch: Some(pitch), outside: false };
        assert!(helical_bore(&[v(0.0, 0.0)], &mill, &params, &heights).is_err(), "{pitch}");
    }
    // Round a 20 mm boss: the tool's centre 15 mm out, climb milling clockwise, out up its own
    // groove. The boss stands, the groove is 12 deep, and a boss wider than twice the tool is fine.
    let boss = HelixParams { diameter: 20.0, climb: true, pitch: None, outside: true };
    let path = helical_bore(&[v(20.0, 20.0)], &mill, &boss, &heights).unwrap();
    let arcs: Vec<(Vec3, bool)> = path.moves.iter().filter_map(|m| if let Move::Arc { to, ccw, .. } = m { Some((*to, *ccw)) } else { None }).collect();
    assert!(arcs.iter().all(|(to, ccw)| !*ccw && (Vec2::new(to.x, to.y).dist(v(20.0, 20.0)) - 15.0).abs() < 1e-9));
    assert!(path.moves.iter().all(|m| !matches!(m, Move::Feed { to, .. } if Vec2::new(to.x, to.y).dist(v(20.0, 20.0)) < 14.0)), "never into the boss");
    let mut stock = StockMap::new_box(Vec3::new(0.0, 0.0, -20.0), Vec3::new(40.0, 40.0, 0.0), 0.25).unwrap();
    let report = stock.cut(&mill, &path, Vec3::new(20.0, 20.0, 10.0)).unwrap();
    assert!(report.rapid_hits.is_empty(), "{report:?}");
    close(sim_height(&stock, 20.0, 20.0), 0.0, 1e-6);
    close(sim_height(&stock, 25.0, 20.0), 0.0, 1e-6);
    close(sim_height(&stock, 32.0, 20.0), -12.0, 1e-6);
    close(sim_height(&stock, 20.0, 8.0), -12.0, 1e-6);
    // Climb milling round a boss turns the other way from climb milling in a hole.
    let conventional = helical_bore(&[v(20.0, 20.0)], &mill, &HelixParams { climb: false, ..boss }, &heights).unwrap();
    assert!(conventional.moves.iter().all(|m| !matches!(m, Move::Arc { ccw: false, .. })));
}

#[test]
fn a_milled_thread_rises_one_pitch_a_turn_in_its_hand() {
    let mut mill = Tool::new(7, ToolKind::ThreadMill, 6.0, 12.0);
    mill.pitch = Some(1.5);
    let heights = cut(0.0, -10.0, 10.0);
    let m10 = ThreadParams { diameter: 10.0, pitch: 1.5, internal: true, left_hand: false };
    let path = thread_mill(&[v(0.0, 0.0)], &mill, &m10, &heights).unwrap();
    let arcs: Vec<(Vec3, bool)> =
        path.moves.iter().filter_map(|m| if let Move::Arc { to, ccw, .. } = m { Some((*to, *ccw)) } else { None }).collect();
    // Up from the bottom, counter-clockwise for a right-hand thread, 0.75 mm a half turn, on a 2 mm radius.
    assert_eq!(arcs.len(), 14);
    for (i, (to, ccw)) in arcs.iter().enumerate() {
        assert!(*ccw);
        close(to.z, -10.0 + 0.75 * (i + 1) as f64, 1e-9);
        close(Vec2::new(to.x, to.y).len(), 2.0, 1e-9);
    }
    assert!(arcs.last().is_some_and(|(to, _)| to.z >= 0.0), "the thread runs out above the top");
    // It starts in the middle of the core hole and leaves there.
    let first_feed = path.moves.iter().find_map(|m| if let Move::Feed { to, .. } = m { Some(*to) } else { None }).unwrap();
    assert_eq!((first_feed.x, first_feed.y), (0.0, 0.0));
    let left = thread_mill(&[v(0.0, 0.0)], &mill, &ThreadParams { left_hand: true, ..m10 }, &heights).unwrap();
    assert!(left.moves.iter().all(|m| !matches!(m, Move::Arc { ccw: true, .. })));
    // A stud: the tool runs outside the thread, on the major radius plus its own.
    let stud = thread_mill(&[v(0.0, 0.0)], &mill, &ThreadParams { internal: false, ..m10 }, &heights).unwrap();
    assert!(stud.moves.iter().all(|m| !matches!(m, Move::Arc { to, .. } if (Vec2::new(to.x, to.y).len() - 8.0).abs() > 1e-9)));
    let program = Program {
        name: "Thread".into(),
        operations: vec![Operation { name: "M10".into(), tool: mill.clone(), spindle_rpm: 6000.0, coolant: false, path }],
        work_offset: None,
    };
    for name in Post::BUILTIN {
        let post = Post::builtin(name).unwrap();
        let text = emit(&program, &post).unwrap();
        assert!(readback(&text, &program, &post).unwrap().within(), "{name}");
    }
    // The tool must fit the core hole; the tool must be a thread mill; the pitch must be sane.
    let mut fat = mill.clone();
    fat.diameter = 9.0;
    assert!(thread_mill(&[v(0.0, 0.0)], &fat, &m10, &heights).is_err());
    assert!(thread_mill(&[v(0.0, 0.0)], &tool(6.0), &m10, &heights).is_err());
    assert!(thread_mill(&[v(0.0, 0.0)], &mill, &ThreadParams { pitch: 0.0, ..m10 }, &heights).is_err());
    assert!(thread_mill(&[], &mill, &m10, &heights).is_err());
}

/// The stock top of the cell nearest to (x, y).
fn sim_height(stock: &StockMap, x: f64, y: f64) -> f64 {
    let (nx, ny) = stock.size();
    let i = (((x - stock.origin().x) / stock.cell()).floor() as isize).clamp(0, nx as isize - 1) as usize;
    let j = (((y - stock.origin().y) / stock.cell()).floor() as isize).clamp(0, ny as isize - 1) as usize;
    stock.height(i, j).unwrap_or(f64::NAN)
}

#[test]
fn heidenhain_klartext_writes_numbered_blocks_and_reads_back() {
    let contour_path = contour(
        &[rect(0.0, 0.0, 40.0, 30.0)],
        &tool(6.0),
        &ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 },
        &cut(0.0, -4.0, 2.0),
    )
    .unwrap();
    let drill_tool = Tool::new(3, ToolKind::Drill, 5.0, 40.0);
    let drill_path = drill(&[v(10.0, 15.0), v(25.0, 15.0)], &drill_tool, &cut(0.0, -5.0, 2.0), 0.25).unwrap();
    let bore = helical_bore(&[v(20.0, 15.0)], &tool(6.0), &HelixParams { diameter: 10.0, climb: true, ..Default::default() }, &cut(0.0, -4.0, 2.0)).unwrap();
    let mut program = Program {
        name: "Saugbacke S1".into(),
        operations: vec![
            Operation { name: "Außenkontur".into(), tool: tool(6.0), spindle_rpm: 6000.0, coolant: true, path: contour_path },
            Operation { name: "Bohrung".into(), tool: tool(6.0), spindle_rpm: 6000.0, coolant: true, path: bore },
            Operation { name: "Drill".into(), tool: drill_tool, spindle_rpm: 3000.0, coolant: false, path: drill_path },
        ],
        work_offset: None,
    };
    let post = Post::heidenhain();
    post.validate().unwrap();
    let text = emit(&program, &post).unwrap();
    let back = readback(&text, &program, &post).unwrap();
    assert!(back.within(), "{back:?}\n{text}");
    assert!((back.cut_length - back.planned_cut_length).abs() < 0.05, "{back:?}");
    let lines: Vec<&str> = text.lines().collect();
    // Every block numbered from 0, between BEGIN and END PGM with the same name.
    assert!(lines.iter().enumerate().all(|(i, l)| l.starts_with(&format!("{i} "))), "{text}");
    assert_eq!(lines.first().copied(), Some("0 BEGIN PGM Saugbacke_S1 MM"));
    assert!(lines.last().is_some_and(|l| l.ends_with("END PGM Saugbacke_S1 MM")));
    // A tool call per tool, the spindle and coolant on the next move, FMAX rapids, signed
    // coordinates, arcs as CC and C with a direction, dwells as cycle 9.
    assert_eq!(lines.iter().filter(|l| l.contains("TOOL CALL ")).count(), 2, "{text}");
    assert!(text.contains("TOOL CALL 1 Z S6000") && text.contains("TOOL CALL 3 Z S3000"), "{text}");
    assert!(lines.iter().any(|l| l.contains(" FMAX M3 M8")), "{text}");
    assert!(text.contains(" R0 FMAX") && text.contains("Z-4") && text.contains("X+43") || text.contains("X-3"), "{text}");
    assert!(text.contains("CC X") && (text.contains(" DR+") || text.contains(" DR-")), "{text}");
    assert!(text.contains("CYCL DEF 9.1 DWELL 0.250"), "{text}");
    assert!(lines.iter().rev().nth(1).is_some_and(|l| l.contains("M5") && l.contains("M30")), "{text}");
    assert!(text.contains("; Aussenkontur"), "German letters are spelled out: {text}");
    assert!(!text.contains("G0") && !text.contains("G1 "), "no G-code: {text}");
    // Another work offset than the control's preset is refused, as is a tapped hole.
    program.work_offset = Some(2);
    assert!(emit(&program, &post).is_err());
}

#[test]
fn a_plasma_torch_cuts_holes_first_and_is_lit_only_from_pierce_to_rapid() {
    let mut torch = Tool::new(1, ToolKind::Plasma, 1.5, 10.0);
    (torch.pierce_height, torch.cut_height, torch.pierce_delay) = (Some(3.8), Some(1.5), Some(0.5));
    let c = Cut {
        heights: Heights { clearance: 10.0, retract: 5.0, top: 0.0, bottom: -3.0 },
        feeds: Feeds { cut: 2000.0, plunge: 500.0 },
        stepdown: 3.0,
    };
    let outside = ContourParams { side: Side::Outside, climb: false, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let inside = ContourParams { side: Side::Inside, ..outside };
    // The outline first, then a 4 mm hole in it: the torch cuts the hole first, at half the feed.
    let mut moves = contour(&[rect(0.0, 0.0, 40.0, 30.0)], &torch, &outside, &c).unwrap().moves;
    moves.extend(contour(&[rect(10.0, 10.0, 14.0, 14.0)], &torch, &inside, &c).unwrap().moves);
    let p = PlasmaParams { top: 0.0, travel: 5.0, clearance: 10.0, plunge_feed: 500.0, slow_holes: Some((5.0, 0.5)) };
    let path = plasma(&Toolpath::new(moves), &torch, &p).unwrap();
    let dwells: Vec<f64> = path.moves.iter().filter_map(|m| if let Move::Dwell { seconds } = m { Some(*seconds) } else { None }).collect();
    assert_eq!(dwells, [0.5, 0.5], "one pierce a cut");
    // Never into the sheet: every cut at the cut height, every rapid at the pierce height or above.
    for m in &path.moves {
        match m {
            Move::Feed { to, .. } | Move::Arc { to, .. } => close(to.z, 1.5, 1e-9),
            Move::Rapid { to } => assert!(to.z >= 3.8 - 1e-9, "{m:?}"),
            Move::Dwell { .. } => {}
        }
    }
    let cuts: Vec<(Vec3, f64)> = path.moves.iter().filter_map(|m| if let Move::Feed { to, feed } = m { Some((*to, *feed)) } else { None }).collect();
    let first = cuts.iter().find(|(_, f)| *f != 500.0).unwrap();
    assert!((9.0..15.0).contains(&first.0.x) && (9.0..15.0).contains(&first.0.y), "the hole comes first: {first:?}");
    assert!(cuts.iter().filter(|(to, _)| to.x < 9.0 || to.x > 15.0).all(|(_, f)| *f == 2000.0 || *f == 500.0), "the outline at the full feed");
    assert!(cuts.iter().any(|(_, f)| *f == 1000.0), "the small hole at half the feed");
    // The post lights the torch before each pierce and puts it out before each rapid.
    let program = Program {
        name: "Sheet".into(),
        operations: vec![Operation { name: "Cut".into(), tool: torch.clone(), spindle_rpm: 1.0, coolant: false, path: path.clone() }],
        work_offset: None,
    };
    let post = Post::builtin("plasma").unwrap();
    let text = emit(&program, &post).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.iter().filter(|l| l.trim() == "M3 S1").count(), 2, "{text}");
    for (i, l) in lines.iter().enumerate() {
        if l.starts_with("G4 ") {
            assert_eq!(lines.get(i.wrapping_sub(1)).map(|p| p.trim()), Some("M3 S1"), "lit right before the pierce:\n{text}");
        }
        if l.starts_with("G0") && lines[..i].iter().rev().find(|p| p.trim() == "M3 S1" || p.trim() == "M5").is_some_and(|p| p.trim() == "M3 S1") {
            panic!("a rapid with the torch lit at line {}:\n{text}", i + 1);
        }
    }
    assert!(!text.contains("M6") && !text.contains("G43"), "a torch is not changed:\n{text}");
    let back = readback(&text, &program, &post).unwrap();
    assert!(back.within(), "{back:?}\n{text}");
    // Heidenhain writes no plasma cuts; a torch needs its heights and delay; a mill is no torch.
    assert!(emit(&program, &Post::builtin("heidenhain").unwrap()).is_err());
    let mut bare = torch.clone();
    bare.pierce_delay = None;
    assert!(bare.validate().is_err());
    let mut low = torch.clone();
    low.pierce_height = Some(1.0);
    assert!(low.validate().is_err(), "a torch pierces no lower than it cuts");
    let mut mill = tool(6.0);
    assert!(plasma(&path, &mill, &p).is_err());
    mill.cut_height = Some(1.5);
    assert!(mill.validate().is_err(), "a cut height belongs to a torch");
}

#[test]
fn trochoidal_arc_geometry_is_exact() {
    let pos = v(10.0, 20.0);
    let dir = v(1.0, 0.0);
    let norm = v(0.0, 1.0);
    let pitch = 2.0;
    let r_lat = 3.0;

    // Climb: clockwise forward arc.
    let arc_climb = trochoid_arc_calc(pos, dir, norm, pitch, r_lat, true).unwrap();
    assert!(!arc_climb.ccw, "climb is clockwise");
    assert!((arc_climb.start.dist(arc_climb.center) - arc_climb.radius).abs() < 1e-9);
    assert!((arc_climb.end.dist(arc_climb.center) - arc_climb.radius).abs() < 1e-9);
    let apex = pos + dir * (pitch * 0.5);
    assert!((apex.dist(arc_climb.center) - arc_climb.radius).abs() < 1e-9);
    assert_eq!(arc_climb.start, v(10.0, 23.0));
    assert_eq!(arc_climb.end, v(10.0, 17.0));

    // Conventional: counter-clockwise forward arc.
    let arc_conv = trochoid_arc_calc(pos, dir, norm, pitch, r_lat, false).unwrap();
    assert!(arc_conv.ccw, "conventional is counter-clockwise");
    assert_eq!(arc_conv.start, v(10.0, 17.0));
    assert_eq!(arc_conv.end, v(10.0, 23.0));

    // Piece arc sweeps match.
    let piece_climb = crate::trochoid::trochoid_arc_piece(pos, dir, norm, pitch, r_lat, true).unwrap();
    assert_eq!(piece_climb.start(), arc_climb.start);
    assert!((piece_climb.end().dist(arc_climb.end)) < 1e-6);
}

#[test]
fn trochoidal_channel_cuts_within_bounds_and_reads_back() {
    let path = OpenPath { segs: vec![Seg2::Line { a: v(0.0, 10.0), b: v(50.0, 10.0) }] };
    let t = tool(6.0);
    let params = TrochoidParams { width: 14.0, stepover: 2.0, climb: true, stock_to_leave: 0.5, tolerance: 0.01 };
    let c = cut(0.0, -3.0, 3.0);
    let tp = trochoid_channel(&[path], &t, &params, &c).unwrap();
    verify(&tp, &c.heights).unwrap();

    // Max lateral extension from centerline y=10 must not exceed width/2 - stock = 7 - 0.5 = 6.5.
    // Tool radius is 3, so tool center y must stay within 10 ± (6.5 - 3) = [6.5, 13.5].
    for p in cutting_points(&tp, 0.0) {
        assert!(p.y >= 6.5 - 1e-6 && p.y <= 13.5 + 1e-6, "lateral gouge at {p:?}");
        assert!(p.x >= -1.0 && p.x <= 51.0, "longitudinal overrun at {p:?}");
    }

    // Emits G-code and reads back.
    let prog = Program {
        name: "TrochoidSlot".into(),
        operations: vec![Operation { name: "Slot".into(), tool: t, spindle_rpm: 3000.0, coolant: true, path: tp }],
        work_offset: None,
    };
    let post = Post::builtin("linuxcnc").unwrap();
    let text = emit(&prog, &post).unwrap();
    let back = readback(&text, &prog, &post).unwrap();
    assert!(back.within(), "readback failed:\n{text}");
}

#[test]
fn pocket_corner_peeling_clears_sharp_corners_in_progressive_steps() {
    // An L-shaped pocket with inside corners. Passes 5 apart with a 6mm tool.
    let l_shape =
        Region2 { outer: polygon(&[(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (20.0, 20.0), (20.0, 40.0), (0.0, 40.0)]).outer, holes: Vec::new() };
    let t = tool(6.0);
    let params = PocketParams { stepover: 4.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: false };
    let c = cut(0.0, -2.0, 2.0);
    let path = pocket(std::slice::from_ref(&l_shape), &t, &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();

    let shape = Shape::from_regions(std::slice::from_ref(&l_shape), 0.01).unwrap();
    for p in cutting_points(&path, 0.0) {
        assert!(shape.clear(v(p.x, p.y), 3.0 - 1e-4), "gouge at {p:?}");
    }

    // Ensure all points in the pocket reachable by tool are cleared.
    for i in 0..=80 {
        for j in 0..=80 {
            let q = v(i as f64 * 0.5, j as f64 * 0.5);
            if shape.clear(q, 3.0 + 1e-3) {
                let d = dist_to_cuts(&path, -2.0 + 1e-6, q);
                assert!(d <= 3.0 + 0.01, "{q:?} is {d} from nearest cut");
            }
        }
    }
}

#[test]
fn hostile_trochoid_parameters_are_refused() {
    let t = tool(6.0);
    let p_valid = TrochoidParams { width: 10.0, stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01 };
    assert!(p_valid.validate(&t).is_ok());

    let mut p_narrow = p_valid;
    p_narrow.width = 5.0; // narrower than tool
    assert!(p_narrow.validate(&t).is_err());

    let mut p_zero_step = p_valid;
    p_zero_step.stepover = 0.0;
    assert!(p_zero_step.validate(&t).is_err());

    let mut p_too_wide_step = p_valid;
    p_too_wide_step.stepover = 8.0; // larger than tool diameter
    assert!(p_too_wide_step.validate(&t).is_err());

    let mut p_excess_stock = p_valid;
    p_excess_stock.stock_to_leave = 3.0; // wall = 5 - 3 - 3 < 0
    assert!(p_excess_stock.validate(&t).is_err());

    let c = cut(0.0, -2.0, 2.0);
    assert!(trochoid_channel(&[], &t, &p_valid, &c).is_err(), "empty path is refused");
}

#[test]
fn adaptive_pocket_generates_central_helical_entry_and_peeled_corners() {
    let regions = [rect(0.0, 0.0, 50.0, 50.0)];
    let t = tool(6.0);
    let c = cut(0.0, -4.0, 4.0);
    let params = PocketParams {
        stepover: 2.0,
        climb: true,
        stock_to_leave: 0.0,
        tolerance: 0.01,
        ramp_angle: 5.0f64.to_radians(),
        entry: None,
        adaptive: true,
    };
    let path = pocket(&regions, &t, &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();

    // Check that cuts stay within safe pocket walls
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let pts = cutting_points(&path, 0.0);
    assert!(!pts.is_empty());
    for p in &pts {
        assert!(shape.clear(v(p.x, p.y), 3.0 - 1e-4), "gouge at {p:?}");
    }

    // Has helical entry moves and peeled corner moves (which produce arc moves)
    let arc_count = path.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count();
    assert!(arc_count > 0, "adaptive pocket must contain circular arc moves from helical entry and peeled corners");
}

#[test]
fn slot_trochoidal_flag_generates_trochoids() {
    let t = tool(6.0);
    let c = cut(0.0, -2.0, 2.0);
    let line = open_line((0.0, 0.0), (50.0, 0.0));
    let params = SlotParams { width: Some(10.0), stepover: 2.0, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, trochoidal: true };
    let path = slot(&[line], &t, &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();

    let arc_count = path.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count();
    assert!(arc_count >= 10, "trochoidal slotting along 50mm line must generate forward trochoid arcs");
}

#[test]
fn adaptive_pocket_clears_channel_pocket_with_radial_wavefront_and_return_links() {
    // L-shaped pocket similar to Fusion 360 adaptive clearing channel image
    let regions = [polygon(&[(0.0, 0.0), (50.0, 0.0), (50.0, 20.0), (20.0, 20.0), (20.0, 50.0), (0.0, 50.0)])];
    let t = tool(6.0);
    let c = cut(0.0, -3.0, 3.0);
    let params = PocketParams {
        stepover: 2.0,
        climb: true,
        stock_to_leave: 0.0,
        tolerance: 0.01,
        ramp_angle: 5.0f64.to_radians(),
        entry: None,
        adaptive: true,
    };
    let path = pocket(&regions, &t, &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();

    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let pts = cutting_points(&path, 0.0);
    assert!(!pts.is_empty());
    for p in &pts {
        assert!(shape.clear(v(p.x, p.y), 3.0 - 1e-4), "gouge at {p:?}");
    }

    let arc_count = path.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count();
    assert!(arc_count >= 10, "adaptive channel clearing must produce radial wavefront arcs, found {arc_count}");
}

#[test]
fn adaptive_pocket_defaults_to_helical_entry_and_spiral_with_zero_ramp_angle() {
    let regions = [rect(0.0, 0.0, 40.0, 40.0)];
    let t = tool(6.0);
    let c = cut(0.0, -3.0, 3.0);
    // Explicitly 0.0 ramp_angle
    let params = PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: None, adaptive: true };
    let path = pocket(&regions, &t, &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();

    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let pts = cutting_points(&path, 0.0);
    assert!(!pts.is_empty());
    for p in &pts {
        assert!(shape.clear(v(p.x, p.y), 3.0 - 1e-4), "gouge at {p:?}");
    }

    let helical_arcs = path.moves.iter().filter(|m| if let Move::Arc { to, .. } = m { to.z > -3.0 + 1e-4 } else { false }).count();
    assert!(
        helical_arcs > 0,
        "adaptive clearing must spiral down via helical entry even with ramp_angle 0, found {helical_arcs} helical descent arcs"
    );
}

#[test]
fn open_pocket_adaptive_clearing_enters_in_air_and_expands_wavefront() {
    // A 20 × 10 pocket open on its left side (mouth at x = 0), entered from x = -5 in air.
    let regions = [rect(0.0, 0.0, 20.0, 10.0)];
    let entry = v(-5.0, 5.0);
    let params =
        PocketParams { stepover: 1.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: Some(entry), adaptive: true };
    let c = cut(0.0, -3.0, 3.0);
    let t = tool(2.0);
    let path = pocket(&regions, &t, &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();

    // Verify no gouging into closed walls: x=20, y=0, y=10
    let walls = [
        Piece::Line { a: v(20.0, 0.0), b: v(20.0, 10.0) },
        Piece::Line { a: v(0.0, 10.0), b: v(20.0, 10.0) },
        Piece::Line { a: v(0.0, 0.0), b: v(20.0, 0.0) },
    ];
    let pts = cutting_points(&path, 0.0);
    assert!(!pts.is_empty());
    for p in &pts {
        for w in &walls {
            assert!(w.dist(v(p.x, p.y)) >= 1.0 - 1e-4, "the disc touches a closed wall at ({}, {})", p.x, p.y);
        }
    }

    // Verify arc moves exist (radial wavefront arcs)
    let arc_count = path.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count();
    assert!(arc_count >= 5, "open pocket adaptive clearing must produce expanding radial arcs, found {arc_count}");

    // The plunge must be in the air at entry, no helical ramp down into material
    let helical_arcs = path.moves.iter().filter(|m| if let Move::Arc { to, .. } = m { to.z > -3.0 + 1e-4 } else { false }).count();
    assert_eq!(helical_arcs, 0, "open pocket entry in air must plunge directly without helical descent in stock");
}

#[test]
fn closed_pocket_adaptive_clearing_with_entry_spirals_down_inside() {
    // A closed 40 × 40 pocket with an entry point inside it: spirals down inside the pocket
    let regions = [rect(0.0, 0.0, 40.0, 40.0)];
    let t = tool(6.0);
    let c = cut(0.0, -3.0, 3.0);
    let entry = v(20.0, 20.0); // Inside the closed pocket
    let params =
        PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0, entry: Some(entry), adaptive: true };
    let path = pocket(&regions, &t, &params, &c).unwrap();
    verify(&path, &c.heights).unwrap();

    let helical_arcs = path.moves.iter().filter(|m| if let Move::Arc { to, .. } = m { to.z > -3.0 + 1e-4 } else { false }).count();
    assert!(
        helical_arcs > 0,
        "closed pocket adaptive clearing must spiral down via helical entry when entry is inside, found {helical_arcs} helical descent arcs"
    );
}
