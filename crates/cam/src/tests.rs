use std::f64::consts::{FRAC_PI_2, PI};

use solidcraft_geom::{Loop2, Region2, Seg2, Vec2, Vec3};

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
    let path = Toolpath {
        moves: vec![
            Move::Rapid { to: Vec3::new(100.0, 0.0, 0.0) },
            Move::Arc { to: Vec3::new(-100.0, 0.0, 0.0), center: v(0.0, 0.0), ccw: true, feed: 100.0 },
        ],
    };
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

/// Distance in XY from `q` to the nearest cut made entirely below `below`.
fn dist_to_cuts(path: &Toolpath, below: f64, q: Vec2) -> f64 {
    let flat = |p: Vec3| Vec3::new(p.x, p.y, 0.0);
    let pts = path.points(0.005);
    pts.windows(2)
        .filter(|w| w[1].1 && w[0].0.z < below && w[1].0.z < below)
        .map(|w| q.extend(0.0).dist_to_segment(flat(w[0].0), flat(w[1].0)))
        .fold(f64::INFINITY, f64::min)
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
    let params = PocketParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
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
    let params = PocketParams { stepover: 3.0, climb: false, stock_to_leave: 0.5, tolerance: 0.01, ramp_angle: 0.0 };
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
    let params = PocketParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: ramp };
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
    assert!(pocket(&regions, &tool(6.0), &PocketParams { ramp_angle: 2.0, ..params }, &c).is_err());
}

#[test]
fn pocket_with_passes_up_to_a_diameter_apart_still_clears_the_middle() {
    // A 40 × 30 pocket, a 6 mm tool, passes 5 apart: rings 3, 8 and 13 from the walls. The last
    // leaves a strip down the middle (15 from the long walls) more than a radius from it.
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    for (stepover, name) in [(5.0, "wide"), (6.0, "a diameter"), (4.0, "moderate")] {
        let params = PocketParams { stepover, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
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
    let params = PocketParams { stepover: 5.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
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
    let too_wide = PocketParams { stepover: 6.5, ..params };
    assert!(pocket(&regions, &tool(6.0), &too_wide, &cut(0.0, -2.0, 2.0)).is_err(), "passes more than a diameter apart");
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
    let path = Toolpath { moves };
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
    let back = Toolpath { moves: parse(&text).unwrap() };
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
    let params = PocketParams { stepover: 3.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
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
        let back = Toolpath { moves: parse(&text).unwrap() };
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
    let params = PocketParams { stepover: 3.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 3.0f64.to_radians() };
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
        operations: vec![Operation {
            path: Toolpath { moves: vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) }] },
            ..program.operations[0].clone()
        }],
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
        operations: vec![Operation { name: "o".into(), tool: tool(6.0), spindle_rpm: 1000.0, coolant: false, path: Toolpath { moves } }],
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
        assert!(matches!(verify(&Toolpath { moves: moves.clone() }, &h), Err(CamError::Unsafe(_))), "{moves:?}");
    }
    // A drill returning into its own hole is fine.
    let ok = vec![
        Move::Rapid { to: at(0.0, 10.0) },
        Move::Feed { to: at(0.0, -3.0), feed: 100.0 },
        Move::Rapid { to: at(0.0, 2.0) },
        Move::Rapid { to: at(0.0, -2.5) },
        Move::Feed { to: at(0.0, -5.0), feed: 100.0 },
    ];
    verify(&Toolpath { moves: ok }, &h).unwrap();
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
        let pocket_params = PocketParams { stepover: b, climb: true, stock_to_leave: b, tolerance: b, ramp_angle: b };
        let contour_params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: b, tolerance: b, ramp_angle: b };
        let bad_cut = Cut { heights: Heights { clearance: b, retract: b, top: b, bottom: -b }, feeds: Feeds { cut: b, plunge: b }, stepdown: b };
        for c in [good_cut, bad_cut, Cut { stepdown: b, ..good_cut }] {
            for tl in [&t, &tool(6.0)] {
                let _ = pocket(&regions, tl, &pocket_params, &c);
                let _ = pocket(&regions, tl, &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &c);
                let _ = contour(&regions, tl, &contour_params, &c);
            }
        }
        let moves = vec![
            Move::Rapid { to: Vec3::new(b, b, b) },
            Move::Arc { to: Vec3::new(b, 0.0, b), center: v(b, b), ccw: true, feed: b },
            Move::Dwell { seconds: b },
        ];
        let path = Toolpath { moves };
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
        pocket(&regions, &tool(6.0), &PocketParams { stepover: 1e-9, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &good_cut)
            .is_err()
    );
    assert!(
        pocket(&regions, &tool(60.0), &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &good_cut)
            .is_err()
    );
    assert!(
        pocket(&[], &tool(6.0), &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &good_cut)
            .is_err()
    );
    // A drill cuts only at its point: no contour or pocket with one.
    let drill = Tool::new(4, ToolKind::Drill, 6.0, 30.0);
    let square = [rect(0.0, 0.0, 20.0, 20.0)];
    let side = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(contour(&square, &drill, &side, &good_cut).unwrap_err().to_string().contains("a drill cannot mill"));
    let lanes = PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(pocket(&square, &drill, &lanes, &good_cut).is_err());
    let open = Region2 { outer: Loop2 { segs: rect(0.0, 0.0, 10.0, 10.0).outer.segs.into_iter().take(3).collect() }, holes: vec![] };
    assert!(offset_regions(&[open], 1.0, 0.01).is_err());
    assert!(emit(&Program::default(), &Post::rs274()).is_err());
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
    let path = Toolpath { moves: vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 5.0) }, Move::Feed { to: Vec3::new(250.0, 10.0, -2.0), feed: 800.0 }] };
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
