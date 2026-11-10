//! 2D Adaptive Clearing pocket milling strategy.
//!
//! Generates a radial wavefront toolpath from the pocket's Chebyshev center (pole of
//! inaccessibility) outward. In enclosed areas, passes expand in concentric circular loops.
//! As the wavefront meets pocket walls, passes break into circular sweeping arcs across
//! each channel with constant radial engagement, accompanied by low-load return links across
//! previously cleared interior space. A final perimeter contour pass finishes the walls
//! to exact tolerance.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use crate::offset::Shape;
use crate::ops::{Builder, Cut, PocketParams};
use crate::path::Toolpath;
use crate::piece::Piece;
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Finds the pocket's central entry point (approximation of the Chebyshev center).
pub(crate) fn find_chebyshev_center(shape: &Shape, rings: &[Vec<Vec<Piece>>], wall: f64) -> Vec2 {
    let candidate = if let Some(innermost) = rings.last().and_then(|r_vec| r_vec.first()) {
        let mut sum = Vec2::ZERO;
        let mut count = 0.0;
        for p in innermost {
            sum = sum + p.start();
            count += 1.0;
        }
        if count > 0.0 { sum / count } else { Vec2::ZERO }
    } else {
        Vec2::ZERO
    };

    let mut best_pt = candidate;
    let mut best_d = if shape.clear(candidate, wall) { shape.dist(candidate) } else { -1.0 };

    let mut candidates = Vec::new();
    candidates.push(candidate);
    if let Some(innermost) = rings.last().and_then(|r_vec| r_vec.first()) {
        for p in innermost {
            candidates.push(p.start());
            candidates.push(p.point_at(0.5));
        }
    }

    for pt in candidates {
        if shape.clear(pt, wall) {
            let d = shape.dist(pt);
            if d > best_d {
                best_d = d;
                best_pt = pt;
            }
        }
    }

    // Local gradient search around best_pt
    let mut step = 1.0;
    while step > 0.05 {
        let mut improved = false;
        for dx in [-1.0, 0.0, 1.0] {
            for dy in [-1.0, 0.0, 1.0] {
                if dx == 0.0 && dy == 0.0 {
                    continue;
                }
                let np = best_pt + Vec2::new(dx * step, dy * step);
                if shape.clear(np, wall) {
                    let d = shape.dist(np);
                    if d > best_d + 1e-4 {
                        best_d = d;
                        best_pt = np;
                        improved = true;
                    }
                }
            }
        }
        if !improved {
            step *= 0.5;
        }
    }

    best_pt
}

/// Discretizes circle `K(center, radius)` and extracts continuous arc segments that remain inside `shape.clear(., wall)`.
pub(crate) fn extract_arc_intervals(shape: &Shape, center: Vec2, radius: f64, wall: f64, climb: bool) -> Vec<Piece> {
    if radius <= 1e-4 {
        return Vec::new();
    }

    let circ = TAU * radius;
    let n = ((circ / 0.5).ceil()).clamp(120.0, 1440.0) as usize;
    let d_theta = TAU / n as f64;

    // Add a small safety margin for arc chord between discrete angular samples
    let safety_margin = (radius * (1.0 - (d_theta * 0.5).cos()) + 0.01).max(0.01);
    let wall_req = wall + safety_margin;

    let mut is_clear = vec![false; n];
    let mut any_clear = false;
    let mut all_clear = true;

    for i in 0..n {
        let theta = i as f64 * d_theta;
        let pt = center + Vec2::new(theta.cos(), theta.sin()) * radius;
        let c = shape.clear(pt, wall_req);
        is_clear[i] = c;
        if c {
            any_clear = true;
        } else {
            all_clear = false;
        }
    }

    if !any_clear {
        return Vec::new();
    }

    // If completely clear 360 degrees: single full circular arc
    if all_clear {
        let (a0, sweep) = if climb { (0.0, TAU) } else { (0.0, -TAU) };
        return vec![Piece::Arc { c: center, r: radius, a0, sweep }];
    }

    // Identify contiguous angular intervals of clear samples
    let mut intervals: Vec<(usize, usize)> = Vec::new();
    let mut in_interval = false;
    let mut start_idx = 0;

    for i in 0..n {
        if is_clear[i] && !in_interval {
            in_interval = true;
            start_idx = i;
        } else if !is_clear[i] && in_interval {
            in_interval = false;
            if i > 0 && i - 1 > start_idx {
                intervals.push((start_idx, i - 1));
            }
        }
    }
    if in_interval && n - 1 > start_idx {
        intervals.push((start_idx, n - 1));
    }

    // Merge wrap-around interval (across angle 0 / 2pi) if necessary
    if intervals.len() > 1 && intervals.first().is_some_and(|(s, _)| *s == 0) && intervals.last().is_some_and(|(_, e)| *e == n - 1) {
        if let Some(last) = intervals.pop() {
            if let Some(first) = intervals.first_mut() {
                first.0 = last.0;
            }
        }
    }

    let mut pieces = Vec::new();
    for (s_idx, e_idx) in intervals {
        let span_steps = if e_idx >= s_idx { e_idx - s_idx } else { (n - s_idx) + e_idx };
        let span_angle = span_steps as f64 * d_theta;
        if span_angle * radius < 0.1 {
            continue;
        }

        let theta_start = s_idx as f64 * d_theta;
        let theta_end = theta_start + span_angle;

        // In climb milling, CCW positive sweep (center on left, uncut material on right).
        // In conventional milling, CW negative sweep.
        let (a0, sweep) = if climb { (theta_start, span_angle) } else { (theta_end, -span_angle) };

        pieces.push(Piece::Arc { c: center, r: radius, a0, sweep });
    }

    pieces
}

/// Generates a continuous Archimedean spiral starting from `r_start` and expanding outward
/// by `stepover` per 360-degree revolution: `r(theta) = r_start + (stepover / 2pi) * theta`.
/// Stops as soon as the spiral approaches the pocket wall.
/// Returns the list of spiral points and the final radius reached.
pub(crate) fn generate_central_spiral(
    shape: &Shape,
    center: Vec2,
    r_start: f64,
    stepover: f64,
    wall: f64,
    climb: bool,
    tol: f64,
) -> (Vec<Vec2>, f64) {
    let mut points = Vec::new();
    let mut theta = 0.0f64;
    let b = stepover / TAU; // Radial growth per radian
    let mut r = r_start;
    let safety_margin = 0.02;
    let wall_req = wall + safety_margin;

    let p0 = center + Vec2::new(r_start, 0.0);
    if !shape.clear(p0, wall_req) {
        return (points, r_start);
    }
    points.push(p0);

    let max_theta = 500.0 * TAU;

    while theta < max_theta {
        // Dynamic angular step keeping chord error below tolerance
        let d_theta = ((8.0 * tol.max(1e-4) / r).sqrt()).clamp(0.02, 0.15);
        theta += d_theta;
        r = r_start + b * theta;

        let angle = if climb { theta } else { -theta };
        let pt = center + Vec2::new(angle.cos(), angle.sin()) * r;

        if !shape.clear(pt, wall_req) {
            // Reached pocket boundary
            break;
        }
        points.push(pt);
    }

    (points, r)
}

/// Executes full 2D adaptive clearing on one depth level `z`.
pub(crate) fn adaptive_pocket_level(
    b: &mut Builder,
    shape: &Shape,
    rings: &[Vec<Vec<Piece>>],
    tool: &Tool,
    params: &PocketParams,
    z: f64,
    cleared: f64,
    wall: f64,
    r: f64,
) -> Result<()> {
    let center = if let Some(entry) = params.entry.filter(|p| shape.clear(*p, wall)) { entry } else { find_chebyshev_center(shape, rings, wall) };
    let clearance_at_center = (shape.dist(center) - wall).max(0.05);
    let r_helix = (r * 0.45).min(params.stepover * 0.5).min(clearance_at_center * 0.8).clamp(0.05, 50.0);
    let start_helix = center + Vec2::new(r_helix, 0.0);

    // 1. Helical Ramp Entry at Chebyshev Center
    b.travel_to(start_helix)?;
    b.plunge_to(cleared)?;

    if cleared - z > 1e-6 {
        let ramp_angle = if params.ramp_angle > 0.0 { params.ramp_angle } else { 3.0f64.to_radians() };
        let total_drop = cleared - z;
        let max_drop_per_turn = (TAU * r_helix * ramp_angle.tan()).max(1e-4);
        let turns = ((total_drop / max_drop_per_turn).ceil()).clamp(1.0, 500.0) as usize;
        let drop_per_quad = total_drop / (turns as f64 * 4.0);
        let mut cur_z = cleared;
        let sweep_step = if params.climb { FRAC_PI_2 } else { -FRAC_PI_2 };

        for _ in 0..turns {
            for q in 0..4 {
                let next_z = (cur_z - drop_per_quad).max(z);
                let a0 = if params.climb { (q as f64) * FRAC_PI_2 } else { -(q as f64) * FRAC_PI_2 };
                let arc = Piece::Arc { c: center, r: r_helix, a0, sweep: sweep_step };
                b.cut_piece(&arc, cur_z, next_z)?;
                cur_z = next_z;
            }
        }
        // Bottom circular cleanup pass at floor depth z
        let sweep_full = if params.climb { TAU } else { -TAU };
        let arc = Piece::Arc { c: center, r: r_helix, a0: 0.0, sweep: sweep_full };
        b.cut_piece(&arc, z, z)?;
    } else {
        b.plunge_to(z)?;
    }

    let mut cur_pos = start_helix;
    let stepover = params.stepover.clamp(1e-3, tool.diameter);

    // 2. Continuous Archimedean Spiral in Central Enclosed Region
    let (spiral_pts, r_spiral_end) = generate_central_spiral(shape, center, r_helix, stepover, wall, params.climb, params.tolerance);

    for pt in &spiral_pts {
        b.cut_to(*pt, z)?;
    }
    if let Some(last) = spiral_pts.last() {
        cur_pos = *last;
    }

    // 3. Compute Radial Wavefront Arcs for Channels
    let mut max_dist = 0.0f64;
    for lp in shape.loops() {
        for p in lp {
            max_dist = max_dist.max(p.start().dist(center));
            max_dist = max_dist.max(p.end().dist(center));
        }
    }

    let mut radius = (r_spiral_end + stepover * 0.5).max(r_helix + stepover);
    let mut arc_passes: Vec<Vec<Piece>> = Vec::new();

    while radius <= max_dist + stepover {
        let arcs = extract_arc_intervals(shape, center, radius, wall, params.climb);
        if !arcs.is_empty() {
            arc_passes.push(arcs);
        }
        radius += stepover;
    }

    // 4. Chain and cut radial arcs branch by branch
    // Track unmachined arcs per radial pass
    let mut remaining: Vec<Vec<Piece>> = arc_passes;

    while remaining.iter().any(|pass| !pass.is_empty()) {
        // Find best next arc: search closest starting point among active radial fronts
        let mut best_pass_idx = None;
        let mut best_arc_idx = None;
        let mut best_dist = f64::INFINITY;

        for (p_idx, pass) in remaining.iter().enumerate() {
            for (a_idx, arc) in pass.iter().enumerate() {
                let d = cur_pos.dist(arc.start());
                if d < best_dist {
                    best_dist = d;
                    best_pass_idx = Some(p_idx);
                    best_arc_idx = Some(a_idx);
                }
            }
            if best_pass_idx.is_some() && p_idx > 3 {
                // Prioritize keeping the current local radial expansion consecutive
                break;
            }
        }

        let (p_idx, a_idx) = match (best_pass_idx, best_arc_idx) {
            (Some(p), Some(a)) => (p, a),
            _ => break,
        };

        let arc = remaining[p_idx].remove(a_idx);
        let start = arc.start();

        // Repositioning link move across cleared interior space (yellow move)
        if cur_pos.dist(start) > 1e-4 {
            let direct_clear = crate::ops::link_is_clear(shape, cur_pos, start, wall, r);
            if direct_clear {
                b.cut_to(start, z)?;
            } else {
                // Route through cleared core towards center
                let mid = (cur_pos + start) * 0.5;
                let dir_c = (center - mid).normalized().unwrap_or(Vec2::ZERO);
                let inward_pt = mid + dir_c * (stepover * 0.75);
                if shape.clear(inward_pt, wall)
                    && crate::ops::link_is_clear(shape, cur_pos, inward_pt, wall, r)
                    && crate::ops::link_is_clear(shape, inward_pt, start, wall, r)
                {
                    b.cut_to(inward_pt, z)?;
                    b.cut_to(start, z)?;
                } else {
                    b.return_to(start)?;
                    b.plunge_to(z)?;
                }
            }
        }

        // Forward cutting arc
        b.cut_piece(&arc, z, z)?;
        cur_pos = arc.end();
    }

    // 4. Final Perimeter Clean-up Pass (Offset loop at wall distance)
    // Ensures crisp walls and cleans cusps left by radial passes
    if let Some(outer_ring) = rings.first() {
        for lp in outer_ring {
            let lp = if params.climb { lp.clone() } else { crate::offset::reversed(lp) };
            let Some(first) = lp.first() else { continue };
            let start = first.start();

            if crate::ops::link_is_clear(shape, cur_pos, start, wall, r) {
                b.cut_to(start, z)?;
            } else {
                b.return_to(start)?;
                b.plunge_to(z)?;
            }

            b.cut_loop(&lp, z)?;
            cur_pos = start;
        }
    }

    Ok(())
}

/// Discretizes circle `K(center, radius)` for an open pocket and extracts arc segments that remain inside `shape` or mouth air, keeping `wall` distance from closed walls.
pub(crate) fn extract_open_arc_intervals(shape: &Shape, walls: &[Piece], entry: Vec2, radius: f64, wall: f64, lead: f64, climb: bool) -> Vec<Piece> {
    if radius <= 1e-4 {
        return Vec::new();
    }

    let circ = TAU * radius;
    let n = ((circ / 0.5).ceil()).clamp(120.0, 1440.0) as usize;
    let d_theta = TAU / n as f64;

    let safety_margin = (radius * (1.0 - (d_theta * 0.5).cos()) + 0.01).max(0.01);
    let wall_req = wall + safety_margin;

    let mut is_valid = vec![false; n];
    let mut bites = vec![false; n];
    let mut any_valid = false;

    for i in 0..n {
        let theta = i as f64 * d_theta;
        let pt = entry + Vec2::new(theta.cos(), theta.sin()) * radius;
        let fits_walls = walls.iter().all(|w| w.dist(pt) >= wall_req);
        let in_reach = shape.inside(pt, 1e-6) || shape.dist(pt) <= lead;
        let v = fits_walls && in_reach;
        is_valid[i] = v;
        if v {
            any_valid = true;
            bites[i] = shape.inside(pt, 1e-6) || shape.dist(pt) <= wall;
        }
    }

    if !any_valid {
        return Vec::new();
    }

    let mut intervals: Vec<(usize, usize)> = Vec::new();
    let mut in_interval = false;
    let mut start_idx = 0;

    for i in 0..n {
        if is_valid[i] && !in_interval {
            in_interval = true;
            start_idx = i;
        } else if !is_valid[i] && in_interval {
            in_interval = false;
            if i > 0 && i - 1 >= start_idx {
                intervals.push((start_idx, i - 1));
            }
        }
    }
    if in_interval && n - 1 >= start_idx {
        intervals.push((start_idx, n - 1));
    }

    if intervals.len() > 1 && intervals.first().is_some_and(|(s, _)| *s == 0) && intervals.last().is_some_and(|(_, e)| *e == n - 1) {
        if let Some(last) = intervals.pop() {
            if let Some(first) = intervals.first_mut() {
                first.0 = last.0;
            }
        }
    }

    let mut pieces = Vec::new();
    for (s_idx, e_idx) in intervals {
        let mut bites_metal = false;
        if e_idx >= s_idx {
            for idx in s_idx..=e_idx {
                if bites[idx] {
                    bites_metal = true;
                    break;
                }
            }
        } else {
            for idx in s_idx..n {
                if bites[idx] {
                    bites_metal = true;
                    break;
                }
            }
            if !bites_metal {
                for idx in 0..=e_idx {
                    if bites[idx] {
                        bites_metal = true;
                        break;
                    }
                }
            }
        }
        if !bites_metal {
            continue;
        }

        let span_steps = if e_idx >= s_idx { e_idx - s_idx } else { (n - s_idx) + e_idx };
        let span_angle = span_steps as f64 * d_theta;
        if span_angle * radius < 0.1 {
            continue;
        }

        let theta_start = s_idx as f64 * d_theta;
        let theta_end = theta_start + span_angle;

        let (a0, sweep) = if climb { (theta_start, span_angle) } else { (theta_end, -span_angle) };

        pieces.push(Piece::Arc { c: entry, r: radius, a0, sweep });
    }

    pieces
}

/// Whether the tool of radius `r` can feed straight from `a` to `b` in an open pocket at depth `z`:
/// keeps `wall` distance from closed walls, and every sample along the link is either in air
/// outside the pocket (`!shape.inside`) or within the already cleared radial wavefront radius.
fn is_open_link_clear(
    shape: &Shape,
    walls: &[Piece],
    a: Vec2,
    b: Vec2,
    wall: f64,
    r: f64,
    entry: Vec2,
    cleared_radius: f64,
) -> bool {
    let target_r = cleared_radius.max(b.dist(entry));
    let n = ((a.dist(b) / (r * 0.25).max(1e-3)).ceil()).clamp(1.0, 2000.0) as usize;
    (0..=n).all(|i| {
        let p = a.lerp(b, i as f64 / n as f64);
        walls.iter().all(|w| w.dist(p) >= wall - 1e-6)
            && (!shape.inside(p, 1e-6) || p.dist(entry) <= target_r + 1e-4)
    })
}

/// Executes full 2D adaptive clearing on an open pocket entered through `entry` in the air outside.
pub(crate) fn adaptive_pocket_open(shape: &Shape, params: &PocketParams, cut: &Cut, entry: Vec2, tool: &Tool, r: f64, wall: f64) -> Result<Toolpath> {
    if shape.inside(entry, r.max(1e-6)) || shape.dist(entry) < r - 1e-7 {
        return invalid("the entry point must be in air outside the pocket, clear of at least the tool radius");
    }

    let (mut cx, mut cy) = (0.0, 0.0);
    let mut outer_pts = 0usize;
    for lp in shape.loops().iter().filter(|lp| !lp.is_empty()) {
        if lp.iter().map(Piece::area_term).sum::<f64>() > 0.0 {
            for piece in lp {
                let p = piece.point_at(0.5);
                cx += p.x;
                cy += p.y;
                outer_pts += 1;
            }
        }
    }
    if outer_pts == 0 {
        return invalid("the pocket is too small for the tool");
    }
    let d = ((Vec2::new(cx / outer_pts as f64, cy / outer_pts as f64) - entry).normalized())
        .ok_or_else(|| CamError::Invalid("the entry point sits at the pocket's centre: pick one clear outside".into()))?;

    let mut walls: Vec<Piece> = Vec::new();
    let mut opens = 0usize;
    struct WallChain {
        ch: Vec<Piece>,
        closed: bool,
    }
    let mut wall_chains: Vec<WallChain> = Vec::new();

    for lp in shape.loops() {
        let outer = lp.iter().map(Piece::area_term).sum::<f64>() > 0.0;
        let facing: Vec<bool> = lp
            .iter()
            .map(|piece| {
                let t = piece.tangent_at(0.5);
                outer && Vec2::new(t.y, -t.x).dot(d) < -(crate::ops::OPENING_FACE - 1e-9)
            })
            .collect();
        let n = lp.len();
        if n == 0 {
            continue;
        }
        opens += facing.iter().filter(|f| **f).count();
        for (piece, f) in lp.iter().zip(&facing) {
            if !f {
                walls.push(*piece);
            }
        }
        if !outer {
            wall_chains.push(WallChain { ch: lp.clone(), closed: true });
            continue;
        }
        if facing.iter().all(|f| *f) {
            wall_chains.push(WallChain { ch: lp.clone(), closed: false });
            continue;
        }
        if facing.iter().all(|f| !*f) {
            wall_chains.push(WallChain { ch: lp.clone(), closed: true });
            continue;
        }
        for i in 0..n {
            if facing[i] || !facing[(i + n - 1) % n] {
                continue;
            }
            let mut ch = Vec::new();
            for k in 0..n {
                if facing[(i + k) % n] {
                    break;
                }
                ch.push(lp[(i + k) % n]);
            }
            wall_chains.push(WallChain { ch, closed: false });
        }
    }

    if opens == 0 {
        return invalid("the entry point does not face an opening: no pass can enter the pocket through the air");
    }

    let stepover = params.stepover.clamp(1e-3, tool.diameter);
    let lead = 0.5 * r + 0.25;

    let mut d_min = f64::INFINITY;
    let mut d_max = 0.0f64;
    for lp in shape.loops() {
        for piece in lp {
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let p = piece.point_at(t);
                let dist = p.dist(entry);
                d_min = d_min.min(dist);
                d_max = d_max.max(dist);
            }
        }
    }

    let mut radius = (d_min - lead).max(0.01);
    let mut arc_passes: Vec<(f64, Vec<Piece>)> = Vec::new();
    while radius <= d_max + stepover {
        let arcs = extract_open_arc_intervals(shape, &walls, entry, radius, wall, lead, params.climb);
        if !arcs.is_empty() {
            arc_passes.push((radius, arcs));
        }
        radius += stepover;
    }

    let mut b = Builder::new(cut);
    for z in cut.levels()? {
        b.travel_to(entry)?;
        b.plunge_to(z)?;

        let mut cur_pos = entry;
        let mut cleared_radius = arc_passes.first().map(|(r, _)| *r).unwrap_or(0.0);
        let mut remaining: Vec<(f64, Vec<Piece>)> = arc_passes.clone();

        while let Some(p_idx) = remaining.iter().position(|(_, pass)| !pass.is_empty()) {
            let pass_radius = remaining[p_idx].0;
            let mut best_arc_idx = 0;
            let mut best_dist = f64::INFINITY;
            for (a_idx, arc) in remaining[p_idx].1.iter().enumerate() {
                let d = cur_pos.dist(arc.start());
                if d < best_dist {
                    best_dist = d;
                    best_arc_idx = a_idx;
                }
            }

            let arc = remaining[p_idx].1.remove(best_arc_idx);
            let start = arc.start();

            if cur_pos.dist(start) > 1e-4 {
                let direct_clear = is_open_link_clear(shape, &walls, cur_pos, start, wall, r, entry, cleared_radius);
                if direct_clear {
                    b.cut_to(start, z)?;
                } else {
                    let mid = (cur_pos + start) * 0.5;
                    let dir_to_entry = (entry - mid).normalized().unwrap_or(Vec2::ZERO);
                    let inward_pt = mid + dir_to_entry * (stepover * 0.75);
                    if walls.iter().all(|w| w.dist(inward_pt) >= wall - 1e-6)
                        && is_open_link_clear(shape, &walls, cur_pos, inward_pt, wall, r, entry, cleared_radius)
                        && is_open_link_clear(shape, &walls, inward_pt, start, wall, r, entry, cleared_radius)
                    {
                        b.cut_to(inward_pt, z)?;
                        b.cut_to(start, z)?;
                    } else {
                        let inward_pt2 = mid + dir_to_entry * (stepover * 1.5);
                        if walls.iter().all(|w| w.dist(inward_pt2) >= wall - 1e-6)
                            && is_open_link_clear(shape, &walls, cur_pos, inward_pt2, wall, r, entry, cleared_radius)
                            && is_open_link_clear(shape, &walls, inward_pt2, start, wall, r, entry, cleared_radius)
                        {
                            b.cut_to(inward_pt2, z)?;
                            b.cut_to(start, z)?;
                        } else {
                            b.return_to(start)?;
                            b.plunge_to(z)?;
                        }
                    }
                }
            }

            b.cut_piece(&arc, z, z)?;
            cur_pos = arc.end();
            cleared_radius = cleared_radius.max(pass_radius);
        }

        // Perimeter finish pass along closed walls
        for wc in &wall_chains {
            let off = crate::ops::chain_offset(&wc.ch, wall, wc.closed)?;
            if off.is_empty() {
                continue;
            }
            let start = off[0].start();
            if is_open_link_clear(shape, &walls, cur_pos, start, wall, r, entry, cleared_radius) {
                b.cut_to(start, z)?;
            } else {
                b.return_to(start)?;
                b.plunge_to(z)?;
            }
            if wc.closed {
                b.cut_loop(&off, z)?;
                cur_pos = start;
            } else {
                for piece in &off {
                    b.cut_piece(piece, z, z)?;
                }
                cur_pos = off.last().unwrap().end();
            }
        }

        b.retract()?;
    }

    Ok(Toolpath::new(b.into_moves()))
}

/// Top-level 2D adaptive clearing pocket operation.
///
/// If an entry point is provided in air and the pocket has an opening allowing sideways entry,
/// executes sideways-entry adaptive clearing with continuous radial wavefront expansion.
/// Otherwise (e.g. closed pocket contour or no opening), executes closed-pocket adaptive clearing
/// by spiralling down inside the closed pocket with central helical ramp entry.
pub(crate) fn adaptive_pocket(
    shape: &Shape,
    params: &PocketParams,
    cut: &Cut,
    tool: &Tool,
    r: f64,
    wall: f64,
) -> Result<Toolpath> {
    if let Some(entry) = params.entry {
        if !shape.inside(entry, r.max(1e-6)) && shape.dist(entry) >= r - 1e-7 {
            if let Ok(path) = adaptive_pocket_open(shape, params, cut, entry, tool, r, wall) {
                return Ok(path);
            }
        }
    }
    adaptive_pocket_closed(shape, params, cut, tool, r, wall)
}

/// Executes closed-pocket adaptive clearing: spirals down inside the pocket with helical entry,
/// expands via central Archimedean spiral and radial channel wavefronts, and finishes walls.
pub(crate) fn adaptive_pocket_closed(
    shape: &Shape,
    params: &PocketParams,
    cut: &Cut,
    tool: &Tool,
    r: f64,
    wall: f64,
) -> Result<Toolpath> {
    let mut rings: Vec<Vec<Vec<Piece>>> = Vec::new();
    loop {
        let ring = shape.offset(wall + params.stepover * rings.len() as f64)?;
        if ring.is_empty() {
            break;
        }
        rings.push(ring);
        if rings.len() > crate::ops::MAX_RINGS {
            return Err(CamError::TooLarge(format!("the pocket needs more than {} passes; use a larger stepover", crate::ops::MAX_RINGS)));
        }
    }
    if rings.is_empty() {
        return invalid("the tool does not fit the pocket");
    }

    let mut b = Builder::new(cut);
    let mut cleared = cut.heights.top;
    for z in cut.levels()? {
        adaptive_pocket_level(&mut b, shape, &rings, tool, params, z, cleared, wall, r)?;
        cleared = z;
        b.retract()?;
    }

    Ok(Toolpath::new(b.into_moves()))
}
