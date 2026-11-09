//! 2.5D operations: contours and pockets. Each one turns planar geometry
//! and cutting data into a [`Toolpath`] and checks it with [`verify`] before returning it.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use solidcore_record::{Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::lead::{Leads, lead_in, lead_out};
use crate::offset::{MAX_PIECES, Shape, reversed};
use crate::open::{OpenContourParams, OpenPath, side_normal};
use crate::path::{DrillCycle, MAX_MOVES, Move, Toolpath};
use crate::piece::Piece;
use crate::tabs::Tabs;
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most depth passes of one operation.
const MAX_LEVELS: usize = 10_000;
/// Most offset rings of one pocket.
const MAX_RINGS: usize = 2_000;
/// Most times a ramp goes round its loop; a loop too short for that is plunged into instead.
const MAX_RAMP_LAPS: f64 = 2000.0;
/// Largest coordinate or height (mm).
pub(crate) const MAX_COORD: f64 = 1.0e6;

/// Peck drilling in work XY. Every peck feeds from retract height and fully retracts for
/// chip clearance. `stepdown` is the maximum peck depth. Each hole is also recorded as a
/// [`DrillCycle`], so a post with canned cycles can write it as one block.
pub fn drill(points: &[Vec2], tool: &Tool, cut: &Cut, dwell: f64) -> Result<Toolpath> {
    tool.validate()?;
    cut.validate()?;
    if !matches!(tool.kind, ToolKind::Drill | ToolKind::CentreDrill | ToolKind::Reamer) {
        return invalid("drilling needs a drill, a centre drill or a reamer");
    }
    if points.is_empty() || points.len() > 10_000 {
        return invalid("drilling needs 1 to 10000 hole centres");
    }
    if !dwell.is_finite() || !(0.0..=3600.0).contains(&dwell) {
        return invalid("dwell must be between 0 and 3600 seconds");
    }
    if cut.heights.top <= cut.heights.bottom || cut.heights.top - cut.heights.bottom > tool.flute_length {
        return invalid("drill depth must be positive and within the flute length");
    }
    if points.iter().any(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD) {
        return invalid("hole centres must be finite and within 1e6 mm");
    }
    let levels = cut.levels()?;
    if points.len().saturating_mul(levels.len().saturating_mul(3).saturating_add(3)) > MAX_MOVES {
        return Err(CamError::TooLarge("too many drilling moves".into()));
    }
    let mut b = Builder::new(cut);
    let mut cycles = Vec::with_capacity(points.len());
    for point in points {
        b.travel_to(*point)?;
        let start = b.moves.len();
        for (index, z) in levels.iter().enumerate() {
            b.plunge_to(*z)?;
            if index + 1 == levels.len() && dwell > 0.0 {
                b.push(Move::Dwell { seconds: dwell })?;
            }
            b.push(Move::Rapid { to: point.extend(cut.heights.retract) })?;
        }
        b.push(Move::Rapid { to: point.extend(cut.heights.clearance) })?;
        cycles.push(DrillCycle {
            start,
            end: b.moves.len(),
            at: *point,
            retract: cut.heights.retract,
            bottom: cut.heights.bottom,
            peck: (levels.len() > 1).then_some(cut.stepdown),
            dwell,
            feed: cut.feeds.plunge,
            tap: None,
        });
    }
    let path = Toolpath { moves: b.moves, cycles, comp: Vec::new() };
    verify(&path, &cut.heights)?;
    Ok(path)
}

/// Tapping in work XY: each hole fed from the retract height to `bottom` and back, one pitch
/// per spindle turn (`spindle_rpm` x the tap's pitch), recorded as a tapping cycle. Only a post
/// with a rigid tapping cycle can write it: the spindle has to reverse at the bottom in step
/// with the feed, which plain moves cannot say.
pub fn tap(points: &[Vec2], tool: &Tool, cut: &Cut, spindle_rpm: f64) -> Result<Toolpath> {
    tool.validate()?;
    cut.validate()?;
    let Some(pitch) = tool.pitch.filter(|_| tool.kind == ToolKind::Tap) else {
        return invalid("tapping needs a tap with its pitch");
    };
    if !(spindle_rpm > 0.0 && spindle_rpm <= 1.0e5) {
        return invalid("tapping needs a spindle speed above 0 and at most 100000 rev/min");
    }
    if points.is_empty() || points.len() > 10_000 {
        return invalid("tapping needs 1 to 10000 hole centres");
    }
    if cut.heights.top <= cut.heights.bottom || cut.heights.top - cut.heights.bottom > tool.flute_length {
        return invalid("the thread depth must be positive and within the tap's thread length");
    }
    if points.iter().any(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD) {
        return invalid("hole centres must be finite and within 1e6 mm");
    }
    let feed = spindle_rpm * pitch;
    let mut b = Builder::new(cut);
    let mut cycles = Vec::with_capacity(points.len());
    for point in points {
        b.travel_to(*point)?;
        let start = b.moves.len();
        b.push(Move::Rapid { to: point.extend(cut.heights.retract) })?;
        b.push(Move::Feed { to: point.extend(cut.heights.bottom), feed })?;
        b.push(Move::Feed { to: point.extend(cut.heights.retract), feed })?;
        b.push(Move::Rapid { to: point.extend(cut.heights.clearance) })?;
        cycles.push(DrillCycle {
            start,
            end: b.moves.len(),
            at: *point,
            retract: cut.heights.retract,
            bottom: cut.heights.bottom,
            peck: None,
            dwell: 0.0,
            feed,
            tap: Some(pitch),
        });
    }
    let path = Toolpath { moves: b.moves, cycles, comp: Vec::new() };
    verify(&path, &cut.heights)?;
    Ok(path)
}

/// The Z levels of an operation (work coordinates, mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Heights {
    /// Where the tool travels between cuts.
    pub clearance: f64,
    /// Where rapid motion towards the material stops and feed begins.
    pub retract: f64,
    /// Top of the material to cut.
    pub top: f64,
    /// Final depth.
    pub bottom: f64,
}

impl Heights {
    pub fn validate(&self) -> Result<()> {
        let all = [self.clearance, self.retract, self.top, self.bottom];
        if !all.iter().all(|z| z.is_finite() && z.abs() <= MAX_COORD) {
            return invalid("the heights must be finite numbers");
        }
        if self.clearance < self.retract || self.retract < self.top || self.top < self.bottom {
            return invalid("the heights must be ordered: clearance ≥ retract ≥ top ≥ bottom");
        }
        Ok(())
    }
}

/// Feed rates (mm/min).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feeds {
    pub cut: f64,
    pub plunge: f64,
}

impl Feeds {
    pub fn validate(&self) -> Result<()> {
        if [self.cut, self.plunge].iter().all(|f| *f > 0.0 && *f <= 1.0e6) { Ok(()) } else { invalid("the feed rates must be above 0") }
    }
}

/// What every operation needs to cut.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    pub heights: Heights,
    pub feeds: Feeds,
    /// Most material taken per depth pass (mm).
    pub stepdown: f64,
}

impl Cut {
    pub(crate) fn validate(&self) -> Result<()> {
        self.heights.validate()?;
        self.feeds.validate()?;
        if !(self.stepdown > 0.0) || !self.stepdown.is_finite() {
            return invalid("the stepdown must be above 0");
        }
        Ok(())
    }

    /// Depth passes from the top down to the bottom, evenly spaced and at most a stepdown apart.
    pub(crate) fn levels(&self) -> Result<Vec<f64>> {
        let (top, bottom) = (self.heights.top, self.heights.bottom);
        let n = ((top - bottom) / self.stepdown - 1e-9).ceil().max(1.0);
        if !(n <= MAX_LEVELS as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_LEVELS} depth passes")));
        }
        let n = n as usize;
        Ok((1..=n).map(|k| if k == n { bottom } else { top - (top - bottom) * k as f64 / n as f64 }).collect())
    }
}

/// Which side of a profile the tool runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Around the regions: the regions are the part that stays.
    Outside,
    /// Inside the regions' boundary: the regions are the opening.
    Inside,
    /// Tool centre on the boundary.
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourParams {
    pub side: Side,
    /// Climb milling (with a clockwise spindle the wall is on the right of the tool's travel);
    /// otherwise conventional.
    pub climb: bool,
    /// Material left on the wall (mm).
    pub stock_to_leave: f64,
    /// Chord error allowed on spline boundaries (mm).
    pub tolerance: f64,
    /// Steepest descent into each depth pass, along the profile (radians); 0 plunges straight down.
    pub ramp_angle: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PocketParams {
    /// Distance between passes (mm), at most the tool diameter.
    pub stepover: f64,
    pub climb: bool,
    /// Material left on the walls (mm).
    pub stock_to_leave: f64,
    /// Chord error allowed on spline boundaries (mm).
    pub tolerance: f64,
    /// Steepest descent into uncut material, along the first pass (radians); 0 plunges straight down.
    pub ramp_angle: f64,
    /// Open pocket: a point in the air outside the pocket, on its open side (work XY). The
    /// passes then run parallel to the contour the pocket is open through, in bands a
    /// stepover apart from the opening inwards, each band grazing the standing metal by
    /// one stepover and no more; without it the pocket is closed and entered as before, by
    /// a plunge and ramp.
    pub entry: Option<Vec2>,
}

/// Collects moves, keeping the tool's position.
pub(crate) struct Builder {
    moves: Vec<Move>,
    pos: Option<Vec3>,
    h: Heights,
    feeds: Feeds,
}

impl Builder {
    pub(crate) fn new(cut: &Cut) -> Builder {
        Builder { moves: Vec::new(), pos: None, h: cut.heights, feeds: cut.feeds }
    }

    /// The moves collected.
    pub(crate) fn into_moves(self) -> Vec<Move> {
        self.moves
    }

    pub(crate) fn push(&mut self, m: Move) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        if let Some(to) = m.target() {
            self.pos = Some(to);
        }
        self.moves.push(m);
        Ok(())
    }

    fn at(&self, p: Vec2) -> bool {
        self.pos.is_some_and(|q| Vec2::new(q.x, q.y).dist(p) <= 1e-9)
    }

    /// Up to the clearance height (if below it), then across to `p`.
    pub(crate) fn travel_to(&mut self, p: Vec2) -> Result<()> {
        if let Some(q) = self.pos {
            if self.at(p) && q.z >= self.h.clearance {
                return Ok(());
            }
            if q.z < self.h.clearance {
                self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) })?;
            }
        }
        self.push(Move::Rapid { to: p.extend(self.h.clearance) })
    }

    /// Straight down to `z`: rapid to the retract height, then at the plunge feed.
    pub(crate) fn plunge_to(&mut self, z: f64) -> Result<()> {
        let Some(q) = self.pos else { return Err(CamError::Unsafe("a plunge before the first positioning move".into())) };
        if q.z > self.h.retract {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.retract) })?;
        }
        if z < self.pos.map_or(z, |p| p.z) {
            self.push(Move::Feed { to: Vec3::new(q.x, q.y, z), feed: self.feeds.plunge })?;
        }
        Ok(())
    }

    pub(crate) fn cut_to(&mut self, p: Vec2, z: f64) -> Result<()> {
        if self.at(p) && self.pos.is_some_and(|q| (q.z - z).abs() <= 1e-9) {
            return Ok(());
        }
        self.push(Move::Feed { to: p.extend(z), feed: self.feeds.cut })
    }

    /// Cuts along `piece` from wherever the tool is (within the offsetting's joining tolerance
    /// of the piece's start), going evenly from depth `z0` to `z1`.
    pub(crate) fn cut_piece(&mut self, piece: &Piece, z0: f64, z1: f64) -> Result<()> {
        match *piece {
            Piece::Line { b, .. } => self.cut_to(b, z1),
            Piece::Arc { c, sweep, .. } => {
                // Halves, so that no arc move comes near a full turn.
                let parts = if sweep.abs() > PI { 2 } else { 1 };
                for k in 1..=parts {
                    let t = k as f64 / parts as f64;
                    let to = piece.point_at(t).extend(z0 + (z1 - z0) * t);
                    self.push(Move::Arc { to, center: c, ccw: sweep > 0.0, feed: self.feeds.cut })?;
                }
                Ok(())
            }
        }
    }

    pub(crate) fn cut_loop(&mut self, lp: &[Piece], z: f64) -> Result<()> {
        lp.iter().try_for_each(|piece| self.cut_piece(piece, z, z))
    }

    /// Goes down to `z` along `lp`, no steeper than `angle`, round as often as that takes; the
    /// tool ends where the loop starts. Straight down when there is no angle or the loop is too
    /// short to ramp along.
    pub(crate) fn ramp_loop(&mut self, lp: &[Piece], z: f64, angle: f64) -> Result<()> {
        let Some(from) = self.pos.map(|p| p.z) else { return Err(CamError::Unsafe("a ramp before the first positioning move".into())) };
        let drop = from - z;
        if !(drop > 1e-9) {
            return Ok(());
        }
        let length: f64 = lp.iter().map(Piece::length).sum();
        let laps = (drop / angle.tan() / length - 1e-9).ceil().max(1.0);
        if !(angle > 0.0) || !(length > 1e-3) || !(laps <= MAX_RAMP_LAPS) {
            return self.plunge_to(z);
        }
        let total = laps * length;
        let mut done = 0.0;
        for _ in 0..laps as usize {
            for piece in lp {
                let z0 = from - drop * done / total;
                done += piece.length();
                self.cut_piece(piece, z0, from - drop * (done / total).min(1.0))?;
            }
        }
        // Exactly at depth, whatever the rounding of the last step.
        match self.pos {
            Some(p) => self.cut_to(Vec2::new(p.x, p.y), z),
            None => Ok(()),
        }
    }

    /// Up to the retract height (if below it), then across to `p` at that height: the return
    /// between passes, where the tool goes back to where it plunges.
    pub(crate) fn return_to(&mut self, p: Vec2) -> Result<()> {
        if let Some(q) = self.pos
            && q.z < self.h.retract
        {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.retract) })?;
        }
        if !self.at(p) {
            self.push(Move::Rapid { to: p.extend(self.h.retract) })?;
        }
        Ok(())
    }

    /// Up to `z` at a rapid: over a tab the tool must not cut.
    fn lift_to(&mut self, z: f64) -> Result<()> {
        let Some(q) = self.pos else { return Err(CamError::Unsafe("a lift before the first positioning move".into())) };
        if z > q.z + 1e-9 {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, z) })?;
        }
        Ok(())
    }

    /// How many moves are collected.
    pub(crate) fn len(&self) -> usize {
        self.moves.len()
    }

    /// Where the tool is (work coordinates).
    pub(crate) fn pos(&self) -> Option<Vec3> {
        self.pos
    }

    pub(crate) fn retract(&mut self) -> Result<()> {
        match self.pos {
            Some(q) if q.z < self.h.clearance => self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) }),
            _ => Ok(()),
        }
    }

    pub(crate) fn finish(mut self) -> Result<Toolpath> {
        self.retract()?;
        let path = Toolpath::new(self.moves);
        verify(&path, &self.h)?;
        Ok(path)
    }
}

pub(crate) fn check_ramp(angle: f64) -> Result<()> {
    if (0.0..=1.5).contains(&angle) { Ok(()) } else { invalid("the ramp angle must be between 0 and 85 degrees") }
}

pub(crate) fn check_leave(leave: f64) -> Result<()> {
    if leave.is_finite() && (0.0..=MAX_COORD).contains(&leave) { Ok(()) } else { invalid("the stock to leave must be 0 or more") }
}

/// A milling operation needs a tool that cuts with its side: a drill only cuts at its point.
pub(crate) fn mills(tool: &Tool) -> Result<()> {
    if tool.kind.cuts_down() {
        invalid(format!("a {} cannot mill: it cuts only straight down", tool.kind.label()))
    } else if tool.kind == ToolKind::ThreadMill {
        invalid("a thread mill cuts threads only")
    } else {
        Ok(())
    }
}

/// The loop turned to start at its corner nearest to `p`.
fn start_near(lp: &[Piece], p: Vec2) -> Vec<Piece> {
    let first = lp.iter().enumerate().min_by(|a, b| a.1.start().dist(p).total_cmp(&b.1.start().dist(p))).map_or(0, |(i, _)| i);
    lp.iter().cycle().skip(first).take(lp.len()).copied().collect()
}

/// The loop turned to start at its point nearest to `p`, splitting the piece there.
pub(crate) fn start_at(lp: &[Piece], p: Vec2) -> Vec<Piece> {
    let nearest = lp.iter().enumerate().min_by(|a, b| a.1.dist(p).total_cmp(&b.1.dist(p)));
    let Some((index, piece)) = nearest else { return lp.to_vec() };
    let t = piece.param_near(p);
    let (before, after) = (piece.sub(0.0, t), piece.sub(t, 1.0));
    let mut out = Vec::with_capacity(lp.len() + 1);
    if after.length() > 1e-9 {
        out.push(after);
    }
    out.extend(lp.iter().skip(index + 1).chain(lp.iter().take(index)).copied());
    if before.length() > 1e-9 {
        out.push(before);
    }
    if out.is_empty() { lp.to_vec() } else { out }
}

/// Follows the boundaries of `regions` on the chosen side, one loop after the other, each
/// down through every depth pass before the next.
pub fn contour(regions: &[Region2], tool: &Tool, params: &ContourParams, cut: &Cut) -> Result<Toolpath> {
    contour_from(regions, tool, params, cut, None)
}

/// [`contour`], each loop starting at its point nearest to `start` (work XY) when given.
pub fn contour_from(regions: &[Region2], tool: &Tool, params: &ContourParams, cut: &Cut, start: Option<Vec2>) -> Result<Toolpath> {
    contour_with(regions, tool, params, &Leads::default(), cut, start)
}

/// [`contour_from`] with leads: the tool comes onto each loop from its free side, and leaves it
/// to the free side, along `leads` (a closed loop ramps down along itself only with `Direct`
/// leads and no overlap).
pub fn contour_with(regions: &[Region2], tool: &Tool, params: &ContourParams, leads: &Leads, cut: &Cut, start: Option<Vec2>) -> Result<Toolpath> {
    contour_with_tabs(regions, tool, params, leads, &Tabs::default(), cut, start)
}

/// [`contour_with`] also leaving holding tabs (`bridges`) on each loop: on every depth pass that
/// reaches below a tab's top the tool lifts over the tab and crosses it at that height, so the
/// part stays attached to the stock until the tabs are cut by hand.
#[allow(clippy::too_many_arguments)]
pub fn contour_with_tabs(
    regions: &[Region2],
    tool: &Tool,
    params: &ContourParams,
    leads: &Leads,
    tabs: &Tabs,
    cut: &Cut,
    start: Option<Vec2>,
) -> Result<Toolpath> {
    if start.is_some_and(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD) {
        return invalid("the start point must be finite and within 1e6 mm");
    }
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    leads.validate()?;
    tabs.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let away = tool.radius() + params.stock_to_leave;
    let loops = match params.side {
        Side::Outside => shape.offset(-away)?,
        Side::Inside => shape.offset(away)?,
        Side::On => shape.loops().to_vec(),
    };
    if loops.is_empty() {
        return invalid("the tool does not fit the profile");
    }
    // Loops come with the regions on their left. Around a part that puts the wall on the left
    // (conventional), inside an opening on the right (climb).
    let natural_is_climb = params.side != Side::Outside;
    let levels = cut.levels()?;
    let mut b = Builder::new(cut);
    for lp in &loops {
        let lp = if params.climb == natural_is_climb { lp.clone() } else { reversed(lp) };
        let lp = match start {
            Some(p) => start_at(&lp, p),
            None => lp,
        };
        let Some(first) = lp.first() else { continue };
        if leads.is_direct() && tabs.is_empty() {
            b.travel_to(first.start())?;
            // Down to the last pass (through what it cut), then into the material along the loop.
            b.plunge_to(cut.heights.top)?;
            for &z in &levels {
                b.ramp_loop(&lp, z, params.ramp_angle)?;
                b.cut_loop(&lp, z)?;
                b.cut_to(first.start(), z)?;
            }
        } else {
            cut_loop_with_leads(&mut b, &shape, tool, params, leads, tabs, cut, &levels, &lp)?;
        }
        b.retract()?;
    }
    b.finish()
}

/// One closed loop, every depth pass with its lead-in and lead-out: plunge at the lead-in's
/// start, lead in, cut the loop (and the overlap past its start), lead out; then back to the
/// plunge point for the next pass. Passes below the tabs' top lift over each tab.
#[allow(clippy::too_many_arguments)]
fn cut_loop_with_leads(
    b: &mut Builder,
    shape: &Shape,
    tool: &Tool,
    params: &ContourParams,
    leads: &Leads,
    tabs: &Tabs,
    cut: &Cut,
    levels: &[f64],
    lp: &[Piece],
) -> Result<()> {
    let Some(first) = lp.first() else { return Ok(()) };
    let s = first.start();
    let t_in = first.tangent_at(0.0);
    let in_piece = lead_in(&leads.lead_in, s, t_in, free_normal(shape, params.side, s, t_in));
    if let Some(piece) = &in_piece
        && !lead_is_clear(shape, params.side, piece, tool.radius())
    {
        return invalid("the lead-in would cut into the part: make it shorter or pick another");
    }
    let mut run = lp.to_vec();
    if leads.overlap > 0.0 {
        run_overlap(&mut run, lp, leads.overlap)?;
    }
    let Some(run_end) = run.last() else { return Ok(()) };
    let e = run_end.end();
    let t_out = run_end.tangent_at(1.0);
    let out_piece = lead_out(&leads.lead_out, e, t_out, free_normal(shape, params.side, e, t_out));
    if let Some(piece) = &out_piece
        && !lead_is_clear(shape, params.side, piece, tool.radius())
    {
        return invalid("the lead-out would cut into the part: make it shorter or pick another");
    }
    // Tabs: their centres along the loop, so the overlap past the start meets the same ones.
    let loop_len: f64 = lp.iter().map(Piece::length).sum();
    let run_len: f64 = run.iter().map(Piece::length).sum();
    let (centres, step) = tabs.centres(loop_len)?;
    tabs.check_fit(step, cut.heights.top - cut.heights.bottom)?;
    let windows = tab_windows(&centres, tabs.width, loop_len, run_len);
    let tab_top = cut.heights.bottom + tabs.height;
    let plunge = in_piece.map_or(s, |piece| piece.start());
    b.travel_to(plunge)?;
    for (k, &z) in levels.iter().enumerate() {
        if k > 0 {
            b.return_to(plunge)?;
        }
        b.plunge_to(z)?;
        if let Some(piece) = &in_piece {
            b.cut_piece(piece, z, z)?;
        }
        if windows.is_empty() || z >= tab_top - 1e-9 {
            b.cut_loop(&run, z)?;
        } else {
            cut_run_with_tabs(b, &run, z, tab_top, &windows)?;
        }
        if let Some(piece) = &out_piece {
            b.cut_piece(piece, z, z)?;
        }
    }
    Ok(())
}

/// The tab windows as arc-length spans along the cut, the overlap past the start included.
fn tab_windows(centres: &[f64], width: f64, loop_len: f64, run_len: f64) -> Vec<(f64, f64)> {
    if centres.is_empty() {
        return Vec::new();
    }
    let half = width * 0.5;
    let mut windows: Vec<(f64, f64)> = centres.iter().map(|&c| (c - half, (c + half).min(loop_len))).collect();
    let overlap = (run_len - loop_len).max(0.0);
    if overlap > 0.0 {
        let first_run = windows.clone();
        for (a, b) in first_run {
            let (a, b) = (a + loop_len, (b + loop_len).min(run_len));
            if a < run_len - 1e-9 {
                windows.push((a, b));
            }
        }
    }
    windows.sort_by(|x, y| x.0.total_cmp(&y.0));
    windows
}

/// Cuts `run` at depth `z`, lifting over each tab window to `tab_top` and crossing it there.
fn cut_run_with_tabs(b: &mut Builder, run: &[Piece], z: f64, tab_top: f64, windows: &[(f64, f64)]) -> Result<()> {
    let mut pos = 0.0;
    for piece in run {
        let len = piece.length();
        let (from, to) = (pos, pos + len);
        pos = to;
        if !(len > 1e-12) {
            continue;
        }
        let mut cuts = vec![from, to];
        for &(a, b) in windows {
            if a > from + 1e-9 && a < to - 1e-9 {
                cuts.push(a);
            }
            if b > from + 1e-9 && b < to - 1e-9 {
                cuts.push(b);
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() <= 1e-9);
        for pair in cuts.windows(2) {
            let (s0, s1) = (pair[0], pair[1]);
            if !(s1 > s0 + 1e-12) {
                continue;
            }
            let (t0, t1) = (((s0 - from) / len).clamp(0.0, 1.0), ((s1 - from) / len).clamp(0.0, 1.0));
            let part = piece.sub(t0, t1);
            let mid = (s0 + s1) * 0.5;
            if windows.iter().any(|&(a, b)| mid > a && mid < b) {
                b.lift_to(tab_top)?;
                b.cut_piece(&part, tab_top, tab_top)?;
                b.plunge_to(z)?;
            } else {
                b.cut_piece(&part, z, z)?;
            }
        }
    }
    Ok(())
}

/// The unit normal of the side the tool is on at `p`, where the contour runs along `t`:
/// away from the region around a part and into an opening (both the side farther from the
/// boundary), the left of the travel on it.
pub(crate) fn free_normal(shape: &Shape, side: Side, p: Vec2, t: Vec2) -> Vec2 {
    let n = t.perp();
    if side == Side::On {
        return n;
    }
    let probe = (shape.dist(p) * 0.5).max(1e-3);
    if shape.dist(p + n * probe) >= shape.dist(p - n * probe) { n } else { -n }
}

/// Whether a lead stays clear of the part. The tool may touch an open contour it rides on.
pub(crate) fn lead_is_clear(shape: &Shape, side: Side, piece: &Piece, r: f64) -> bool {
    let n = ((piece.length() / (r * 0.25).max(1e-3)).ceil()).clamp(1.0, 512.0) as usize;
    (0..=n).all(|i| {
        let p = piece.point_at(i as f64 / n as f64);
        match side {
            Side::Inside => shape.clear(p, r),
            Side::Outside => shape.dist(p) >= r - 1e-6,
            // The whole loop rides the boundary: only the free side can be told.
            Side::On => true,
        }
    })
}

/// Appends the first `overlap` mm of `lp` to `run`, the loop running on past its start.
pub(crate) fn run_overlap(run: &mut Vec<Piece>, lp: &[Piece], overlap: f64) -> Result<()> {
    let total: f64 = lp.iter().map(Piece::length).sum();
    if !(overlap < total) {
        return invalid("the overlap must be shorter than the contour");
    }
    let mut left = overlap;
    for piece in lp {
        if !(left > 1e-9) {
            break;
        }
        let len = piece.length();
        if len >= left {
            run.push(piece.sub(0.0, left / len));
            left = 0.0;
        } else {
            run.push(*piece);
            left -= len;
        }
    }
    Ok(())
}

/// Follows each open path, offset by the tool radius (+ stock) to `side`, through every depth
/// pass, each pass in the same direction: lead in, cut, lead out, up, back to the plunge point.
pub fn contour_open(paths: &[OpenPath], tool: &Tool, params: &OpenContourParams, leads: &Leads, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    params.validate()?;
    leads.validate()?;
    if paths.is_empty() || paths.len() > 1_000 {
        return invalid("open milling needs 1 to 1000 paths");
    }
    let levels = cut.levels()?;
    let mut b = Builder::new(cut);
    for path in paths {
        let chain = crate::open::offset_path(path, params, tool.radius())?;
        cut_open_chain(&mut b, &chain, params, leads, &levels)?;
        b.retract()?;
    }
    b.finish()
}

fn cut_open_chain(b: &mut Builder, chain: &[Piece], params: &OpenContourParams, leads: &Leads, levels: &[f64]) -> Result<()> {
    let (Some(first), Some(last)) = (chain.first(), chain.last()) else {
        return invalid("the tool does not fit the open contour");
    };
    let t_in = first.tangent_at(0.0);
    let in_piece = lead_in(&leads.lead_in, first.start(), t_in, side_normal(params.side, t_in));
    let t_out = last.tangent_at(1.0);
    let out_piece = lead_out(&leads.lead_out, last.end(), t_out, side_normal(params.side, t_out));
    let plunge = in_piece.map_or(first.start(), |piece| piece.start());
    b.travel_to(plunge)?;
    for (k, &z) in levels.iter().enumerate() {
        if k > 0 {
            b.return_to(plunge)?;
        }
        b.plunge_to(z)?;
        if let Some(piece) = &in_piece {
            b.cut_piece(piece, z, z)?;
        }
        b.cut_loop(chain, z)?;
        if let Some(piece) = &out_piece {
            b.cut_piece(piece, z, z)?;
        }
    }
    Ok(())
}

/// Clears `regions` (holes are islands) with rings parallel to the boundary, from the inside out.
/// With `entry` set (an open pocket) the rings give way to bands of passes running along the
/// opening, each entered through the air on the open side.
pub fn pocket(regions: &[Region2], tool: &Tool, params: &PocketParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    let r = tool.radius();
    if !(params.stepover > 0.0) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let wall = r + params.stock_to_leave;
    if let Some(at) = params.entry {
        if !params.climb {
            return invalid("an open pocket feeds every pass parallel to its open side: the passes cannot run the other way, so it is always climb");
        }
        if !(at.is_finite() && at.x.abs() <= MAX_COORD && at.y.abs() <= MAX_COORD) {
            return invalid("the entry point must be finite and within 1e6 mm");
        }
        return pocket_open(&shape, params, cut, at, r, wall);
    }
    let mut rings: Vec<Vec<Vec<Piece>>> = Vec::new();
    loop {
        let ring = shape.offset(wall + params.stepover * rings.len() as f64)?;
        if ring.is_empty() {
            break;
        }
        rings.push(ring);
        if rings.len() > MAX_RINGS {
            return Err(CamError::TooLarge(format!("the pocket needs more than {MAX_RINGS} passes; use a larger stepover")));
        }
    }
    if rings.is_empty() {
        return invalid("the tool does not fit the pocket");
    }
    // Rings more than a radius apart leave material wherever the next ring in does not run
    // within a radius: the middle of a part of the pocket it no longer fits in, a passage that
    // narrows. A ring a radius further in clears it; it is cut where some stretch of it is out
    // of the next ring's reach (corners between rings are cleared by the spurs below).
    if params.stepover > r + 1e-9 {
        let mut passes = Vec::new();
        for (k, ring) in rings.iter().enumerate() {
            passes.push(ring.clone());
            let next: Vec<&Piece> = rings.get(k + 1).into_iter().flatten().flatten().collect();
            let reached = |p: Vec2| next.iter().any(|piece| piece.dist(p) <= r + 1e-6);
            let extra: Vec<Vec<Piece>> = shape
                .offset(wall + params.stepover * k as f64 + r)?
                .into_iter()
                .filter(|lp| lp.iter().any(|piece| [0.25, 0.5, 0.75].iter().any(|t| !reached(piece.point_at(*t)))))
                .collect();
            if !extra.is_empty() {
                passes.push(extra);
            }
        }
        rings = passes;
    }
    let mut b = Builder::new(cut);
    // The depth already cut wherever this pass's rings run.
    let mut cleared = cut.heights.top;
    for z in cut.levels()? {
        // Whether the tool is down in this level's cut.
        let mut down: Option<Vec2> = None;
        for lp in rings.iter().rev().flatten() {
            // Offset loops keep the pocket on their left, so the wall is on the right: climb.
            let lp = if params.climb { lp.clone() } else { reversed(lp) };
            let lp = start_near(&lp, down.unwrap_or_default());
            let Some(first) = lp.first() else { continue };
            let start = first.start();
            match down {
                Some(from) if link_is_clear(&shape, from, start, wall, r) => b.cut_to(start, z)?,
                _ => {
                    b.travel_to(start)?;
                    b.plunge_to(cleared)?;
                    b.ramp_loop(&lp, z, params.ramp_angle)?;
                }
            }
            // Rings far apart do not reach into a sharp corner between them: there the tool
            // steps towards the next ring's corner and back.
            let inward = if params.climb { 1.0 } else { -1.0 };
            for (piece, next) in lp.iter().zip(lp.iter().cycle().skip(1)) {
                b.cut_piece(piece, z, z)?;
                if let Some(to) = corner_spur(piece, next, inward, params.stepover, r).filter(|to| link_is_clear(&shape, piece.end(), *to, wall, r)) {
                    b.cut_to(to, z)?;
                    b.cut_to(piece.end(), z)?;
                }
            }
            b.cut_to(start, z)?;
            down = Some(start);
        }
        cleared = z;
        b.retract()?;
    }
    b.finish()
}

/// Where to step to from the corner between `piece` and `next` so that the corner between
/// this ring and the next one in (`gap` away) is cleared: along the corner's bisector, to within
/// a radius of the next ring's corner. `inward`: 1 with the pocket on the loop's left, −1 on
/// its right. `None` where the rings' own passes already clear the corner.
fn corner_spur(piece: &Piece, next: &Piece, inward: f64, gap: f64, r: f64) -> Option<Vec2> {
    let (t_in, t_out) = (piece.tangent_at(1.0), next.tangent_at(0.0));
    let bend = crate::piece::turn(t_in, t_out);
    // Half the corner's inside angle has this sine.
    let sharp = (bend.abs() * 0.5).cos();
    if bend * inward <= 1e-6 || sharp < 0.05 || gap <= r * (1.0 + sharp) + 1e-9 {
        return None;
    }
    let dir = (t_out - t_in).normalized()?;
    Some(piece.end() + dir * (gap / sharp - r))
}

/// Whether the tool can feed straight from `a` to `b` without touching a wall.
fn link_is_clear(shape: &Shape, a: Vec2, b: Vec2, wall: f64, r: f64) -> bool {
    let n = ((a.dist(b) / (r * 0.25).max(1e-3)).ceil()).clamp(1.0, 2000.0) as usize;
    (0..=n).all(|i| shape.clear(a.lerp(b, i as f64 / n as f64), wall))
}

/// How squarely a boundary piece has to face the entry to belong to the open side a pass
/// enters through: its outward normal must oppose the entry direction by more than 45°.
const OPENING_FACE: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The open pocket: `entry` is a point in the air outside the pocket, on its open side.
/// The passes are bands parallel to the contour the pocket is open through, a stepover
/// apart: each band is the locus a fixed distance from that opening contour, so the tool
/// bites the standing metal by exactly one stepover wherever it feeds, and not even the
/// first pass cuts a whole tool width: it grazes the opening one stepover deep. The bands
/// are cut from the opening inwards, so every band descends beside the groove the previous
/// one left, feeds along its floor where the pass begins further along, and retracts at
/// the end; the metal still uncut lies to the right of every feed, as in a climb.
///
/// Boundary pieces of the outer loop facing the entry make up the opening: the tool cuts
/// flush to them. Everything else, the rest of the wall and every hole standing as an
/// island, keeps `wall` clear. A band split by an island descends at its beginning,
/// along a ramp where `ramp_angle` leaves room and straight down otherwise, cutting no
/// more than a stepover there either. Where the last band cannot reach the wall without
/// biting deeper than a stepover, a final pass along the wall trims the band left
/// standing, engaged by no more than a stepover.
fn pocket_open(shape: &Shape, params: &PocketParams, cut: &Cut, entry: Vec2, r: f64, wall: f64) -> Result<Toolpath> {
    if shape.inside(entry, r.max(1e-6)) || shape.dist(entry) < r - 1e-7 {
        return invalid("the entry point must be in air outside the pocket, clear of at least the tool radius");
    }
    // The direction the tool enters from: the centre of the outer loops, away from `entry`.
    let (mut cx, mut cy) = (0.0, 0.0);
    let mut outer_pts = 0usize;
    for lp in shape.loops().iter().filter(|lp| lp.len() >= 3) {
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
    // Opening and walls, told apart by how each piece faces the entry. Loops keep the
    // region on their left, so (t.y, -t.x) is a piece's outward normal; the opening pieces
    // of a loop are grouped into the chains its bands offset from, and every other piece,
    // including every hole, is a wall.
    let mut walls: Vec<Piece> = Vec::new();
    let mut chains: Vec<Vec<Piece>> = Vec::new();
    for lp in shape.loops() {
        let outer = lp.iter().map(Piece::area_term).sum::<f64>() > 0.0;
        let facing: Vec<bool> = lp
            .iter()
            .map(|piece| {
                let t = piece.tangent_at(0.5);
                outer && Vec2::new(t.y, -t.x).dot(d) < -(OPENING_FACE - 1e-9)
            })
            .collect();
        if !outer {
            walls.extend(lp.iter().copied());
            continue;
        }
        let n = facing.len();
        if n == 0 {
            continue;
        }
        for (piece, f) in lp.iter().zip(&facing) {
            if !f {
                walls.push(*piece);
            }
        }
        if facing.iter().all(|f| *f) {
            chains.push(lp.clone());
            continue;
        }
        for i in 0..n {
            if facing[i] && !facing[(i + n - 1) % n] {
                let mut ch = Vec::new();
                for k in 0..n {
                    if !facing[(i + k) % n] {
                        break;
                    }
                    ch.push(lp[(i + k) % n]);
                }
                chains.push(ch);
            }
        }
    }
    if chains.is_empty() {
        return invalid("the entry point does not face an opening: no pass can enter the pocket through the air");
    }
    let s = params.stepover;
    let pitch = (r * 0.25).max(0.05);
    // A pass goes down through the opening where its centre stands outside the metal; the
    // disc may still graze the metal by its stepover there, which is the whole point.
    let air = |p: Vec2| !shape.inside(p, 1e-6);
    let fits = |p: Vec2| walls.iter().all(|w| w.dist(p) >= wall - 1e-6);
    // Every pass at every depth: XY only, the depth levels wrap around them.
    struct Pass {
        path: Vec<Vec2>,
        air: bool,
    }
    let mut passes: Vec<Pass> = Vec::new();
    let wall_loops = shape.offset(wall)?;
    for ch in &chains {
        let (Some(ch0), Some(ch_last)) = (ch.first(), ch.last()) else { continue };
        let (e0, e_last) = (ch0.start(), ch_last.end());
        let graze = |p: Vec2| {
            let f = ch.iter().map(|piece| piece.dist(p)).fold(f64::INFINITY, f64::min);
            if shape.inside(p, 1e-6) { f } else { -f }
        };
        let sunk = |p: Vec2, t: f64| graze(p) <= t - s + 1e-6;
        let mut last_enter: Option<Vec2> = None;
        let mut blank = 0usize;
        let mut t_last: Option<f64> = None;
        let mut j = 1usize;
        while j <= MAX_RINGS && blank < 2 {
            // Band j sits at a reach of j stepovers from the opening face: its centre
            // line, seen from the face, sits one stepover further in than the last.
            let t = s * j as f64 - r;
            j += 1;
            let kept = chain_offset(ch, t)?;
            let mut cuts = false;
            if !kept.is_empty() {
                let mut capped: Vec<Piece> = Vec::with_capacity(kept.len() + 2);
                capped.push(Piece::Line { a: e0, b: kept[0].start() });
                capped.extend(kept);
                let end = capped[capped.len() - 1].end();
                capped.push(Piece::Line { a: end, b: e_last });
                let pts = densify(&loop_poly(&capped, 0.1), pitch);
                let good = |p: Vec2| fits(p) && (shape.inside(p, 1e-6) || graze(p) <= 1e-6);
                for run in band_runs(&pts, &good, s, &graze) {
                    // A band that has passed the closed end runs wholly in air beyond it:
                    // with its disc missing the metal everywhere (a graze below -r leaves
                    // nothing bitten) its pass would cut nothing. Bands that bite nothing
                    // for two in a row end the chain: the reach has crossed the pocket.
                    if !run.iter().any(|q| graze(*q) > -r) {
                        continue;
                    }
                    cuts = true;
                    // Come down on the previous band's floor where that reaches this band's
                    // beginning; otherwise at the pass's own first grazing point, and where
                    // there is none, at its beginning, cutting no more than a stepover.
                    let (enter, own) = match last_enter {
                        Some(p) if sunk(p, t) && p.dist(run[0]) <= 2.0 * s + 1e-6 && link_is_clear(shape, p, run[0], wall, r) => (p, 0usize),
                        _ => {
                            let own = run.iter().position(|q| sunk(*q, t)).unwrap_or(0);
                            (run[own], own)
                        }
                    };
                    last_enter = Some(enter);
                    let mut path = vec![enter];
                    path.extend_from_slice(&run[own..]);
                    passes.push(Pass { path, air: air(enter) });
                }
            }
            if cuts {
                blank = 0;
                t_last = Some(t);
            } else {
                blank += 1;
            }
        }
        // The band left standing at the wall, where no band's reach and no further bite
        // than a stepover can go: trim along the offset wall where it grazes past the
        // last band by no more than a stepover.
        if let Some(t_last) = t_last {
            let strip = |p: Vec2| {
                let f = graze(p);
                f > t_last + 1e-6 && f <= t_last + s + 1e-6
            };
            for oc in &wall_loops {
                let mut poly = loop_poly(oc, 0.1);
                if let Some(first) = poly.first().copied() {
                    poly.push(first);
                }
                let in_strip = |p: Vec2| fits(p) && strip(p) && shape.inside(p, 1e-6);
                for run in band_runs(&densify(&poly, pitch), &in_strip, s, &graze) {
                    let enter = match last_enter {
                        Some(p) if graze(p) <= t_last + 1e-6 && p.dist(run[0]) <= 2.0 * s + 1e-6 && link_is_clear(shape, p, run[0], wall, r) => p,
                        _ => run[0],
                    };
                    last_enter = Some(enter);
                    let path = std::iter::once(enter).chain(run).collect();
                    passes.push(Pass { path, air: air(enter) });
                }
            }
        }
    }
    if passes.is_empty() {
        return invalid("the tool does not fit the pocket");
    }
    if !passes.iter().any(|p| p.air) {
        return invalid("the entry point does not face an opening: no pass can enter the pocket through the air");
    }
    let levels = cut.levels()?;
    let n_pts: usize = passes.iter().map(|p| p.path.len()).sum();
    if levels.len().saturating_mul(passes.len().saturating_mul(8).saturating_add(n_pts).saturating_add(16)) > MAX_MOVES {
        return Err(CamError::TooLarge("too many open pocket moves".into()));
    }
    let mut b = Builder::new(cut);
    for (k, &z) in levels.iter().enumerate() {
        // The depth already cut wherever this level's passes stand.
        let cleared = if k == 0 { cut.heights.top } else { levels[k - 1] };
        for pass in &passes {
            let Some((&head, rest)) = pass.path.split_first() else { continue };
            b.travel_to(head)?;
            if pass.air {
                b.plunge_to(z)?;
                for p in rest {
                    b.cut_to(*p, z)?;
                }
            } else {
                b.plunge_to(cleared)?;
                // Along the floor and the pass itself, no steeper than the ramp; the
                // first pass of a chain and the wall's band come down cutting where no
                // floor reaches, at no more than a stepover engaged.
                let resume = ramp_down(&mut b, &pass.path, cleared, z, params.ramp_angle, pitch)?;
                for p in &pass.path[resume.min(pass.path.len())..] {
                    b.cut_to(*p, z)?;
                }
            }
            // Retract after every pass: each one is fed down again before it cuts, so
            // all passes run the same way and none turns to cut the other.
            b.retract()?;
        }
    }
    b.finish()
}

/// The chain of pieces moved `d` into the region its loop keeps on the left (away from it
/// when negative), joined by arcs round the corners that bend away from the offset, and
/// cut back where neighbouring offset pieces cross: what comes closer to the chain than
/// `d` folds into the offset and goes. The pieces come back in chain order, unclosed; the
/// band of a pocket opening hangs from its ends back to the chain itself.
fn chain_offset(ch: &[Piece], d: f64) -> Result<Vec<Piece>> {
    let ad = d.abs();
    if !(ad > 1e-9) {
        return Ok(ch.to_vec());
    }
    // Crossings nearer a piece's end than this are not cut, and folds this thin go with
    // them: the same rule as the region offset (see offset.rs).
    let tol = 2.0 * (ad * 1e-9).sqrt() + 1e-6;
    let mut raw: Vec<Piece> = Vec::new();
    for (i, piece) in ch.iter().enumerate() {
        match *piece {
            Piece::Line { a, b } => {
                let n = piece.tangent_at(0.0).perp() * d;
                raw.push(Piece::Line { a: a + n, b: b + n });
            }
            Piece::Arc { c, r, a0, sweep } => {
                let r = r - d * sweep.signum();
                if r > 1e-9 {
                    raw.push(Piece::Arc { c, r, a0, sweep });
                }
            }
        }
        if let Some(next) = ch.get(i + 1) {
            // A corner that opens away from the offset side: the offset rolls round the vertex.
            let dir = piece.tangent_at(1.0);
            let bend = crate::piece::turn(dir, next.tangent_at(0.0));
            if bend * d < 0.0 && bend.abs() > 1e-9 {
                raw.push(Piece::Arc { c: piece.end(), r: ad, a0: (dir.perp() * d.signum()).angle(), sweep: bend });
            }
        }
    }
    let mut cuts = vec![Vec::new(); raw.len()];
    let mut hits = Vec::new();
    for (i, a) in raw.iter().enumerate() {
        for (j, b) in raw.iter().enumerate().skip(i + 1) {
            hits.clear();
            a.intersections(b, &mut hits);
            for &(t, u) in &hits {
                cuts[i].push(t);
                cuts[j].push(u);
            }
        }
    }
    let folded = |p: Vec2| ch.iter().any(|piece| piece.dist(p) < ad - 1e-6);
    let mut kept = Vec::new();
    for (piece, ts) in raw.iter().zip(cuts.iter_mut()) {
        let len = piece.length();
        ts.retain(|t| t * len > tol && (1.0 - t) * len > tol);
        ts.sort_by(f64::total_cmp);
        let mut t0 = 0.0;
        for &t in ts.iter().chain(std::iter::once(&1.0)) {
            if t < 1.0 && !((t - t0) * len > tol) {
                continue;
            }
            if kept.len() >= MAX_PIECES {
                return Err(CamError::TooLarge("the offset profile crosses itself too often".into()));
            }
            let sub = piece.sub(t0, t);
            if !folded(sub.point_at(0.5)) {
                kept.push(sub);
            }
            t0 = t;
        }
    }
    Ok(kept)
}

/// The polyline with points no further than `pitch` apart.
fn densify(pts: &[Vec2], pitch: f64) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len() + 16);
    for w in pts.windows(2) {
        out.push(w[0]);
        let n = (w[0].dist(w[1]) / pitch).ceil().max(1.0) as usize;
        for k in 1..n {
            out.push(w[0].lerp(w[1], k as f64 / n as f64));
        }
    }
    out.extend(pts.last().copied());
    out
}

/// The longest stretches of the polyline where `good` holds, each end found by bisection
/// between the last bad and the first good sample, and every run facing the climb: the
/// deeper graze, further from the opening, on the right of the feed.
fn band_runs(pts: &[Vec2], good: &dyn Fn(Vec2) -> bool, stepover: f64, graze: &dyn Fn(Vec2) -> f64) -> Vec<Vec<Vec2>> {
    let n = pts.len();
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    if n < 2 {
        return out;
    }
    // The edge between a good point and its bad neighbour, cut back to `good`'s border.
    let edge = |a: Vec2, b: Vec2| {
        let (mut lo, mut hi) = (0.0f64, 1.0);
        for _ in 0..40 {
            let m = 0.5 * (lo + hi);
            if good(a.lerp(b, m)) {
                lo = m;
            } else {
                hi = m;
            }
        }
        a.lerp(b, lo)
    };
    let mut i = 0usize;
    while i < n {
        if !good(pts[i]) {
            i += 1;
            continue;
        }
        let i0 = i;
        while i < n && good(pts[i]) {
            i += 1;
        }
        let i1 = i - 1;
        let mut run = Vec::with_capacity(i1 - i0 + 3);
        run.push(if i0 == 0 { pts[i0] } else { edge(pts[i0], pts[i0 - 1]) });
        if i1 > i0 + 1 {
            run.extend_from_slice(&pts[i0 + 1..i1]);
        }
        let end = if i1 == n - 1 { pts[i1] } else { edge(pts[i1], pts[i1 + 1]) };
        if run.last().is_none_or(|q| q.dist(end) > 1e-9) {
            run.push(end);
        }
        if run.len() < 2 || run.iter().zip(run.iter().skip(1)).map(|(a, b)| a.dist(*b)).sum::<f64>() < 1e-3 {
            continue;
        }
        run.reverse();
        let k = (run.len() - 1) / 2;
        if let Some(h) = (run[k + 1] - run[k]).normalized() {
            let right = -h.perp();
            if graze(run[k] - right * stepover) > graze(run[k] + right * stepover) + 1e-9 {
                run.reverse();
            }
        }
        out.push(run);
    }
    out
}

/// Down from `cleared` to `z` along `path` at no steeper an angle than `angle`, feeding
/// evenly as it goes where the path leaves room and straight down where it does not. The
/// index of the first path point not yet fed through comes back.
fn ramp_down(b: &mut Builder, path: &[Vec2], cleared: f64, z: f64, angle: f64, pitch: f64) -> Result<usize> {
    let drop = cleared - z;
    if !(drop > 1e-9) || path.len() < 2 {
        b.plunge_to(z)?;
        return Ok(1);
    }
    let ramp = if angle > 0.0 { drop / angle.tan() } else { f64::INFINITY };
    let total: f64 = path.windows(2).map(|w| w[0].dist(w[1])).sum();
    if !ramp.is_finite() || total < ramp + 1e-6 {
        b.plunge_to(z)?;
        return Ok(1);
    }
    let steps = (ramp / pitch).ceil().clamp(1.0, 4000.0) as usize;
    let mut idx = 0usize;
    let mut at = 0.0;
    for st in 1..=steps {
        let want = ramp * st as f64 / steps as f64;
        while idx + 1 < path.len() && at + path[idx].dist(path[idx + 1]) < want {
            at += path[idx].dist(path[idx + 1]);
            idx += 1;
        }
        let seg = path[idx].dist(path[idx + 1]);
        let p = if seg > 1e-9 { path[idx].lerp(path[idx + 1], ((want - at) / seg).min(1.0)) } else { path[idx + 1] };
        b.cut_to(p, cleared - drop * st as f64 / steps as f64)?;
    }
    Ok(idx + 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceParams {
    /// Distance between lanes (mm), at most the tool diameter.
    pub stepover: f64,
    /// How far the tool's edge clears the stock's edge at the ends of each lane and beside the
    /// first and last lane (mm), so no rim is left standing.
    pub overrun: f64,
}

/// Faces the rectangle `lo`–`hi` (work XY, the stock's outline) from the top height down to the
/// bottom in passes no deeper than the stepdown. Each pass runs lanes along X, side by side in Y
/// no more than the stepover apart, back and forth; every lane starts and ends with the tool
/// clear of the stock, so the tool goes down in the air and steps across outside the material.
pub fn face(lo: Vec2, hi: Vec2, tool: &Tool, params: &FaceParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    let ok = |p: Vec2| p.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD;
    if !ok(lo) || !ok(hi) || !(hi.x > lo.x) || !(hi.y > lo.y) {
        return invalid("the area to face needs a finite outline with some width and depth");
    }
    if !(params.stepover > 0.0) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    if !params.overrun.is_finite() || !(0.0..=MAX_COORD).contains(&params.overrun) {
        return invalid("the overrun must be 0 or more");
    }
    if !(cut.heights.top > cut.heights.bottom) {
        return invalid("facing needs a start height above the final height");
    }
    let r = tool.radius();
    let clear = r + params.overrun;
    let (x0, x1) = (lo.x - clear, hi.x + clear);
    // The first and last lane put the tool's edge `overrun` past the stock's edge.
    let (first, last) = (lo.y - params.overrun + r, hi.y + params.overrun - r);
    let lanes: Vec<f64> = if last <= first {
        vec![(lo.y + hi.y) * 0.5]
    } else {
        let n = ((last - first) / params.stepover - 1e-9).ceil().max(1.0);
        if !(n <= MAX_RINGS as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_RINGS} facing lanes")));
        }
        let n = n as usize;
        (0..=n).map(|k| first + (last - first) * k as f64 / n as f64).collect()
    };
    let levels = cut.levels()?;
    if levels.len().saturating_mul(lanes.len().saturating_mul(2).saturating_add(4)) > MAX_MOVES {
        return Err(CamError::TooLarge("too many facing moves".into()));
    }
    let mut b = Builder::new(cut);
    for &z in &levels {
        let Some(&y) = lanes.first() else { continue };
        b.travel_to(Vec2::new(x0, y))?;
        b.plunge_to(z)?;
        for (k, &y) in lanes.iter().enumerate() {
            let (from, to) = if k % 2 == 0 { (x0, x1) } else { (x1, x0) };
            b.cut_to(Vec2::new(from, y), z)?;
            b.cut_to(Vec2::new(to, y), z)?;
        }
        b.retract()?;
    }
    b.finish()
}

/// Walk a closed piece loop as a polyline: lines keep their ends, arcs are sampled so that
/// no chord is longer than `chord`.
fn loop_poly(lp: &[Piece], chord: f64) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    for piece in lp {
        let n = match *piece {
            Piece::Line { .. } => 1,
            Piece::Arc { r, sweep, .. } => (r * sweep.abs() / chord).ceil().clamp(1.0, 2000.0) as usize,
        };
        for i in 0..n {
            let p = piece.point_at(i as f64 / n as f64);
            if out.last().is_none_or(|q| q.dist(p) > 1e-9) {
                out.push(p);
            }
        }
    }
    out
}

/// Where the horizontal line at height `y` crosses the closed polylines, left to right. Each
/// span counts its low end only, so a crossing at a vertex is counted once and a tangent
/// touches yield no crossing at all; an odd leftover from rounding is dropped.
fn scan_crossings(polys: &[Vec<Vec2>], y: f64) -> Vec<f64> {
    let mut xs: Vec<f64> = Vec::new();
    for poly in polys {
        for (a, b) in poly.iter().zip(poly.iter().skip(1).chain(std::iter::once(poly.first()).flatten())) {
            if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                xs.push(a.x + (b.x - a.x) * ((y - a.y) / (b.y - a.y)));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    if xs.len() % 2 == 1 {
        xs.pop();
    }
    xs
}

/// Whether a tool of radius `r` centred on `p` misses every island (a hole of the faced area):
/// not inside one, and no closer to one's wall than its radius.
fn islands_clear(islands: Option<&Shape>, p: Vec2, r: f64) -> bool {
    islands.is_none_or(|s| !s.inside(p, r.max(1e-6)) && s.dist(p) >= r - 1e-7)
}

/// Whether a tool of radius `r` can feed straight from `a` to `b` without touching an island.
fn island_link_clear(islands: Option<&Shape>, a: Vec2, b: Vec2, r: f64) -> bool {
    let n = ((a.dist(b) / (r * 0.25).max(1e-3)).ceil()).clamp(1.0, 4000.0) as usize;
    (0..=n).all(|i| islands_clear(islands, a.lerp(b, i as f64 / n as f64), r))
}

/// Faces the exact outlines of `regions` (work XY, inner loops standing as islands the tool
/// keeps a radius clear of) from the top height down to the bottom in passes no deeper than
/// the stepdown. Each pass runs lanes along X across the outline, side by side in Y no more
/// than the stepover apart, back and forth; each lane ends with the tool's edge `overrun`
/// clear of the outline, so the tool goes down in the air and no rim is left standing. Where
/// the next lane cannot be fed to in a straight line without touching an island, the tool
/// retracts, rapids over and plunges again. With `entry` the tool plunges in air at that
/// point and feeds horizontally onto the sweep at the entry's end: from beyond an end edge it
/// first grazes that edge on extra lanes outside the outline, spaced one stepover, so
/// engagement grows stepover by stepover instead of a full-width bite; from beside the area it
/// joins the first lane outside the outline, where the lanes already clear it. Without an
/// entry each pass starts at the near end of the lowest lane, as [`face`] does.
pub fn face_regions(regions: &[Region2], tool: &Tool, params: &FaceParams, cut: &Cut, entry: Option<Vec2>) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    if regions.is_empty() || regions.len() > 64 {
        return invalid("facing takes from one to 64 regions");
    }
    if !(params.stepover > 0.0) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    if !params.overrun.is_finite() || !(0.0..=MAX_COORD).contains(&params.overrun) {
        return invalid("the overrun must be 0 or more");
    }
    if !(cut.heights.top > cut.heights.bottom) {
        return invalid("facing needs a start height above the final height");
    }
    if let Some(p) = entry
        && !(p.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD)
    {
        return invalid("the entry point must be finite and within 1e6 mm");
    }
    let shape = Shape::from_regions(regions, 1e-4)?;
    let r = tool.radius();
    let clear = r + params.overrun;
    // The islands as their own boundaries: the tool may never overlap one.
    let holes: Vec<Region2> = regions.iter().flat_map(|rg| rg.holes.iter()).map(|h| Region2 { outer: h.clone(), holes: Vec::new() }).collect();
    let islands = if holes.is_empty() { None } else { Some(Shape::from_regions(&holes, 1e-4)?) };
    if let Some(p) = entry {
        // Plunging there must miss the area to face and every island: in air on both sides.
        if shape.inside(p, r.max(1e-6)) || shape.dist(p) < r - 1e-7 || !islands_clear(islands.as_ref(), p, r) {
            return invalid("the entry point must be in air outside the area to face, clear of at least the tool radius");
        }
    }
    // The material outline as polylines: the scanlines cut it directly, and each lane's
    // intervals are grown by `clear` at their ends, then cut back wherever that would put
    // the tool into an island.
    let chord = 0.1;
    let polys: Vec<Vec<Vec2>> = shape.loops().iter().filter(|lp| lp.len() >= 3).map(|lp| loop_poly(lp, chord)).filter(|p| p.len() >= 3).collect();
    let mut pts_iter = polys.iter().flatten();
    let (Some(first), Some(second)) = (pts_iter.next(), pts_iter.next()) else {
        return invalid("the area to face is too small for the tool");
    };
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (first.x.min(second.x), first.x.max(second.x), first.y.min(second.y), first.y.max(second.y));
    for p in std::iter::once(first).chain(std::iter::once(second)).chain(pts_iter) {
        xmin = xmin.min(p.x);
        xmax = xmax.max(p.x);
        ymin = ymin.min(p.y);
        ymax = ymax.max(p.y);
    }
    // The first and last lane put the tool's edge `overrun` past the outline's edge.
    let (lo_y, hi_y) = (ymin + r - params.overrun, ymax + params.overrun - r);
    let base: Vec<f64> = if hi_y <= lo_y {
        vec![(ymin + ymax) * 0.5]
    } else {
        let n = ((hi_y - lo_y) / params.stepover - 1e-9).ceil().max(1.0);
        if !(n <= MAX_RINGS as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_RINGS} facing lanes")));
        }
        let n = n as usize;
        (0..=n).map(|k| lo_y + (hi_y - lo_y) * k as f64 / n as f64).collect()
    };
    // With an entry beyond an end edge, extra grazing lanes outside the outline, one stepover
    // apart while the tool's disc still meets the area, cut outermost first: the entry's first
    // bite is a sliver of the edge and every pass takes one stepover more, rather than the
    // first lane engaging the tool's whole width. The sweep then starts at the entry's end.
    let (first_y, last_y) = if hi_y <= lo_y {
        let mid = (ymin + ymax) * 0.5;
        (mid, mid)
    } else {
        (lo_y, hi_y)
    };
    let mut lanes: Vec<f64> = Vec::new();
    match entry {
        Some(p) if p.y < first_y => {
            let mut y = first_y;
            while y - params.stepover > ymin - r + 1e-9 && lanes.len() < 64 {
                y -= params.stepover;
                lanes.push(y);
            }
            lanes.reverse();
            lanes.extend_from_slice(&base);
        }
        Some(p) if p.y > last_y => {
            let mut y = last_y;
            while y + params.stepover < ymax + r - 1e-9 && lanes.len() < 64 {
                y += params.stepover;
                lanes.push(y);
            }
            lanes.reverse();
            lanes.extend(base.iter().rev());
        }
        _ => lanes = base,
    }
    if lanes.len() > MAX_RINGS {
        return Err(CamError::TooLarge(format!("more than {MAX_RINGS} facing lanes")));
    }
    // The intervals each lane cuts, left to right: every crossing interval of the outline,
    // grown by `clear` at both ends and broken where an island's keep-off band bites into it.
    let pitch = (r * 0.25).max(0.05);
    // A grazing lane's centre stands outside the outline, where its own scanline meets no
    // material: its intervals come from the outline's end strip, the part of the edge the
    // grazing disc actually reaches.
    let edge = ((ymax - ymin) * 1e-3).max(1e-6);
    let mut intervals: Vec<Vec<(f64, f64)>> = Vec::with_capacity(lanes.len());
    for &y in &lanes {
        let xs = scan_crossings(&polys, y.clamp(ymin + edge, ymax - edge));
        let mut lane: Vec<(f64, f64)> = Vec::new();
        for (a, b) in xs.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])) {
            let (p0, p1) = (a - clear, b + clear);
            let n = ((p1 - p0) / pitch).ceil().clamp(4.0, 4000.0) as usize;
            let at = |i: usize| Vec2::new(p0 + (p1 - p0) * i as f64 / n as f64, y);
            // The maximal runs of samples the tool may occupy, kept where they reach the material.
            let mut runs: Vec<(usize, usize)> = Vec::new();
            let mut run: Option<usize> = None;
            for i in 0..=n {
                if islands_clear(islands.as_ref(), at(i), r) {
                    if run.is_none() {
                        run = Some(i);
                    }
                } else if let Some(i0) = run.take() {
                    runs.push((i0, i - 1));
                }
            }
            if let Some(i0) = run {
                runs.push((i0, n));
            }
            for (i0, i1) in runs {
                let (u, v) = (at(i0).x, at(i1).x);
                if v - u > 1e-6 && u < b && v > a {
                    lane.push((u, v));
                }
            }
        }
        intervals.push(lane);
    }
    // The cut points in order and, between neighbours, whether the tool may feed at depth.
    // Lane 0 runs left to right unless the entry sits right of the domain's middle.
    let reverse = entry.is_some_and(|p| p.x > (xmin + xmax) * 0.5);
    let mut pts: Vec<Vec2> = Vec::new();
    let mut links: Vec<bool> = Vec::new();
    for (k, lane) in intervals.iter().enumerate() {
        let mut ordered = lane.clone();
        if (k % 2 == 0) != reverse {
            ordered.reverse();
        }
        for (a, b) in ordered {
            let y = lanes[k];
            let (from, to) = if (k % 2 == 0) != reverse { (Vec2::new(a, y), Vec2::new(b, y)) } else { (Vec2::new(b, y), Vec2::new(a, y)) };
            if let Some(prev) = pts.last() {
                links.push(island_link_clear(islands.as_ref(), *prev, from, r));
            }
            pts.push(from);
            pts.push(to);
        }
    }
    if pts.is_empty() {
        return invalid("the area to face leaves no room for a lane of this tool");
    }
    if let Some(p) = entry
        && !island_link_clear(islands.as_ref(), p, pts[0], r)
    {
        return invalid("the entry point faces an island: the tool would run into it entering the cut");
    }
    let levels = cut.levels()?;
    if levels.len().saturating_mul(pts.len().saturating_add(6)) > MAX_MOVES {
        return Err(CamError::TooLarge("too many facing moves".into()));
    }
    let mut b = Builder::new(cut);
    for &z in &levels {
        b.travel_to(entry.unwrap_or(pts[0]))?;
        b.plunge_to(z)?;
        // Inside an interval the tool always feeds; between intervals it feeds only where
        // the straight link misses every island, and retracts and plunges again where it does not.
        for (i, &pt) in pts.iter().enumerate() {
            let feed = i == 0 || i % 2 == 1 || links[i / 2 - 1];
            if feed {
                b.cut_to(pt, z)?;
            } else {
                b.travel_to(pt)?;
                b.plunge_to(z)?;
            }
        }
        b.retract()?;
    }
    b.finish()
}
