//! Contours whose cutter radius the control compensates (`G41`/`G42` … `G40`).
//!
//! The program then carries the contour (or the nominal tool's centre path, for wear
//! compensation) and the control moves the tool sideways by the radius in its tool table, so a
//! reground or undersized cutter still cuts the part to size. Controls refuse paths they cannot
//! offset ("gouging", "interference"), and they are strict about how compensation is switched
//! on and off. So every compensated pass is written the way all of them accept:
//!
//! - compensation off, a rapid to the entry point, square off the contour on the tool's side,
//!   and down to the pass depth there;
//! - compensation on with a straight entry move, at least 1.25 tool diameters long (LinuxCNC
//!   wants the lead-in at least a radius, the move after `G40` longer than a diameter; Fanuc
//!   switches only on straight moves): square onto the contour, or in line with a lead arc that
//!   then rolls onto it (the control runs the tool round such an arc at its radius less the
//!   tool's, so the arc must be larger than the tool's radius);
//! - the contour at that depth, nothing else in between (no Z-only moves, no dwells, which
//!   Fanuc's two-block look-ahead loses the path on), arcs split below half a turn;
//! - compensation off with a straight exit move square off the contour (or after a lead arc
//!   rolling off it, in line with the arc), as long as the entry;
//! - up. Every depth pass repeats this: compensation is never on across a Z move.
//!
//! Before writing, the programmed contour is offset here the way a control does it (lines moved
//! square to themselves, arcs to a smaller or larger radius, a round corner added at outside
//! corners, inside corners cut back to where the offsets meet). A corner or arc that leaves no
//! room is refused, naming its place, and the result must match the path the CAM works out for
//! the tool: where they differ the control would cut into the part (contours closer together
//! than the tool, a gap narrower than it), and that is refused too. The toolpath's moves are
//! that tool centre path; its [`CompRun`]s hold what the program says instead.

use serde::{Deserialize, Serialize};
use solidcore_record::{Region2, Vec2};
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::lead::{Lead, lead_in, lead_out};
use crate::offset::{Shape, reversed};
use crate::ops::{Builder, ContourParams, Cut, MAX_COORD, Side, check_leave, check_ramp, free_normal, lead_is_clear, mills, run_overlap, start_at};
use crate::path::{CompRun, CompSide, Move, Toolpath};
use crate::piece::{Piece, turn};
use crate::tool::Tool;
use crate::{Result, invalid};

/// Entry and exit lines are at least this many tool diameters long.
pub const ENTRY_DIAMETERS: f64 = 1.25;
/// The control's offset may differ this much from the CAM's (mm) before it counts as a gouge.
const MATCH_TOL: f64 = 1e-3;

/// Who offsets a contour by the tool's radius.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compensation {
    /// The CAM: the program is the tool centre's path for the nominal diameter.
    #[default]
    Cam,
    /// The control: the program is the contour (with the stock to leave), and the control adds
    /// the radius of the tool as measured, from its tool table.
    Control,
    /// The control, for wear only: the program is the nominal tool's centre path, and the
    /// control adds the difference between the measured and the nominal radius (a value near
    /// zero in its tool table).
    Wear,
}

impl Compensation {
    pub fn is_cam(&self) -> bool {
        *self == Compensation::Cam
    }
}

/// How a contour is compensated on the control.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompParams {
    pub mode: Compensation,
    /// The control's offset register (`D`), 1 to 9999; usually the tool number.
    pub register: u32,
    /// How far the entry and exit lines reach off the contour (mm); at least
    /// [`ENTRY_DIAMETERS`] tool diameters (for wear compensation, the tool radius), more when
    /// given.
    pub lead: f64,
    /// How far the cut runs on past its start before the exit line (mm).
    pub overlap: f64,
    /// An arc rolling onto the contour after the entry line; `None`: the entry line lands
    /// square on the contour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_in: Option<CompArc>,
    /// An arc rolling off the contour before the exit line; `None`: the exit line leaves square.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_out: Option<CompArc>,
}

/// A lead arc on a compensated contour. With compensation on the control its radius must
/// exceed the tool's by [`ARC_ROOM`]: the control runs the tool round it at the difference.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompArc {
    /// mm
    pub radius: f64,
    /// Radians, above 0 and at most a half turn.
    pub sweep: f64,
}

impl CompArc {
    fn lead(&self) -> Lead {
        Lead::Arc { radius: self.radius, sweep: self.sweep }
    }
}

impl CompParams {
    pub fn validate(&self) -> Result<()> {
        if self.mode.is_cam() {
            return invalid("this contour is compensated by the CAM: there is nothing for the control to compensate");
        }
        if !(1..=9999).contains(&self.register) {
            return invalid("the compensation register must be 1 to 9999");
        }
        if !(self.lead.is_finite() && (0.0..=MAX_COORD).contains(&self.lead)) {
            return invalid("the entry and exit length must be 0 or more");
        }
        if !(self.overlap.is_finite() && (0.0..=MAX_COORD).contains(&self.overlap)) {
            return invalid("the overlap must be 0 or more");
        }
        for arc in self.arc_in.iter().chain(&self.arc_out) {
            arc.lead().validate()?;
        }
        Ok(())
    }
}

/// [`crate::contour_from`] with the cutter radius compensated by the control: each loop of
/// `regions` on `params.side`, through every depth pass, written as described in the module
/// documentation. The toolpath's moves are the tool centre's path, its [`Toolpath::comp`] runs
/// what the program says. Refused for a ramp, a contour on the line and wherever the control
/// could not offset the contour without cutting into the part.
pub fn contour_compensated(
    regions: &[Region2],
    tool: &Tool,
    params: &ContourParams,
    comp: &CompParams,
    cut: &Cut,
    start: Option<Vec2>,
) -> Result<Toolpath> {
    if start.is_some_and(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD) {
        return invalid("the start point must be finite and within 1e6 mm");
    }
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    comp.validate()?;
    check_leave(params.stock_to_leave, tool)?;
    check_ramp(params.ramp_angle)?;
    if params.ramp_angle > 0.0 {
        return invalid("with compensation on the control the tool goes down at its entry point: ramping along the contour cannot be written");
    }
    let sign = match params.side {
        Side::Outside => -1.0,
        Side::Inside => 1.0,
        Side::On => return invalid("a contour on the line has no side for the control to put the tool on"),
    };
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let r = tool.radius();
    let stock = params.stock_to_leave;
    // The tool centre's loops as the CAM works them out: what the control must make, too.
    let mut centres = shape.offset(sign * (r + stock))?;
    if centres.is_empty() {
        return invalid("the tool does not fit the profile");
    }
    let programmed_loops = match comp.mode {
        // Stock left (or, negative, cut away) moves the programmed contour off (into) the part.
        Compensation::Control if stock != 0.0 => shape.offset(sign * stock)?,
        Compensation::Control => shape.loops().to_vec(),
        Compensation::Wear | Compensation::Cam => centres.clone(),
    };
    // Stock cut away moves the contour into the part, where its outside corners stay sharp; the
    // control then rounds them a radius out, round the moved corner, where one offset by the
    // radius less the stock rounds them round the part's own corner. Both leave the same part,
    // so the control is held to the offset of what it is given.
    if comp.mode == Compensation::Control && stock < 0.0 {
        centres = Shape::from_loops(programmed_loops.clone()).offset(sign * r)?;
    }
    if programmed_loops.len() != centres.len() {
        return invalid(
            "the contours are closer together than the tool (or a gap is narrower than it): with compensation on the control the tool would cut into the part; compensate in the CAM, or use a smaller tool",
        );
    }
    // Each loop: where it starts, and what the control makes of it.
    let mut passes = Vec::with_capacity(programmed_loops.len());
    for lp in &programmed_loops {
        let lp = start_on_piece(lp, start, tool.diameter);
        let centre = match comp.mode {
            Compensation::Control => control_offset(&lp, sign * r)?,
            Compensation::Wear | Compensation::Cam => lp.clone(),
        };
        passes.push((lp, centre));
    }
    if comp.mode == Compensation::Control {
        let made: Vec<Vec<Piece>> = passes.iter().map(|(_, c)| c.clone()).collect();
        matches_offset(&made, &centres, r)?;
    }
    // Loops come with the regions on their left: round a part that is conventional milling.
    let natural_is_climb = params.side != Side::Outside;
    let length = match comp.mode {
        Compensation::Control => (ENTRY_DIAMETERS * tool.diameter).max(r + comp.lead),
        Compensation::Wear | Compensation::Cam => r.max(comp.lead),
    };
    let levels = cut.levels()?;
    let mut b = Builder::new(cut);
    let mut runs = Vec::new();
    for (lp, centre) in passes {
        let (lp, centre) = if params.climb == natural_is_climb { (lp, centre) } else { (reversed(&lp), reversed(&centre)) };
        let mut run = lp.clone();
        if comp.overlap > 0.0 {
            run_overlap(&mut run, &lp, comp.overlap)?;
        }
        let centre_run = match comp.mode {
            Compensation::Control if comp.overlap > 0.0 => control_offset(&run, side_sign(&lp, &centre, sign))?,
            Compensation::Control => centre,
            Compensation::Wear | Compensation::Cam => run.clone(),
        };
        let (Some(first), Some(last), Some(c_first), Some(c_last)) = (run.first(), run.last(), centre_run.first(), centre_run.last()) else {
            continue;
        };
        let (s, e) = (first.start(), last.end());
        let (n_in, n_out) = match comp.mode {
            Compensation::Control => (direction(c_first.start() - s)?, direction(c_last.end() - e)?),
            Compensation::Wear | Compensation::Cam => {
                (free_normal(&shape, params.side, s, first.tangent_at(0.0)), free_normal(&shape, params.side, e, last.tangent_at(1.0)))
            }
        };
        let side = if first.tangent_at(0.0).perp().dot(n_in) > 0.0 { CompSide::Left } else { CompSide::Right };
        // How far the control moves the programmed path: the radius, or (wear) next to nothing.
        let off = if comp.mode == Compensation::Control { r } else { 0.0 };
        let (prog_in, centre_in) = approach(comp.arc_in, s, first.tangent_at(0.0), n_in, c_first.start(), off, length, r)?;
        let (prog_out, centre_out) = departure(comp.arc_out, e, last.tangent_at(1.0), n_out, c_last.end(), off, length, r)?;
        let Some(entry) = prog_in.first().map(Piece::start) else {
            return invalid("the contour has no way on for the tool");
        };
        if centre_in.iter().chain(&centre_out).any(|piece| !lead_is_clear(&shape, params.side, piece, r + stock - 1e-6)) {
            return invalid(format!(
                "with compensation on the control the tool comes on and leaves along a straight line {:.3} mm long (then an arc, when one is chosen), and at ({:.3}, {:.3}) there is no room for it: pick another start point or a smaller arc",
                length, s.x, s.y
            ));
        }
        for (k, &z) in levels.iter().enumerate() {
            if k == 0 {
                b.travel_to(entry)?;
            } else {
                b.return_to(entry)?;
            }
            b.plunge_to(z)?;
            let begin = b.len();
            centre_in.iter().try_for_each(|piece| b.cut_piece(piece, z, z))?;
            b.cut_loop(&centre_run, z)?;
            centre_out.iter().try_for_each(|piece| b.cut_piece(piece, z, z))?;
            let end = b.len();
            // What the program says: from the entry point onto the contour, round it, and off.
            let mut p = Builder::new(cut);
            p.push(Move::Rapid { to: entry.extend(z) })?;
            prog_in.iter().try_for_each(|piece| p.cut_piece(piece, z, z))?;
            p.cut_loop(&run, z)?;
            prog_out.iter().try_for_each(|piece| p.cut_piece(piece, z, z))?;
            let programmed: Vec<Move> = p.into_moves().into_iter().skip(1).collect();
            runs.push(CompRun { start: begin, end, side, register: comp.register, programmed });
        }
        b.retract()?;
    }
    b.retract()?;
    let path = Toolpath { moves: b.into_moves(), cycles: Vec::new(), comp: runs };
    verify(&path, &cut.heights)?;
    Ok(path)
}

/// The smallest a lead arc's radius may exceed the tool's radius by when the control
/// compensates (mm): the control runs the tool round the arc at the difference.
pub const ARC_ROOM: f64 = 0.1;

/// The way onto the contour at `s` (travel `t`, the tool on side `n`): what the program says
/// and the tool centre's path, both from the entry point, where compensation goes on. Without an
/// arc, a straight line `length` long square onto the contour; with one, a straight line
/// `length` long in line with the arc onto its start, then the arc rolling onto the contour.
/// The tool centre starts at the entry point itself (compensation is still off there) and ends
/// at `cs`, `off` from `s` on the tool's side.
#[allow(clippy::too_many_arguments)]
fn approach(arc: Option<CompArc>, s: Vec2, t: Vec2, n: Vec2, cs: Vec2, off: f64, length: f64, r: f64) -> Result<(Vec<Piece>, Vec<Piece>)> {
    let Some(arc) = arc else {
        let entry = s + n * length;
        return Ok((vec![Piece::Line { a: entry, b: s }], vec![Piece::Line { a: entry, b: cs }]));
    };
    let (programmed, centre) = comp_arc(lead_in(&arc.lead(), s, t, n), off, r)?;
    let start = programmed.start();
    let entry = start - programmed.tangent_at(0.0) * length;
    Ok((vec![Piece::Line { a: entry, b: start }, programmed], vec![Piece::Line { a: entry, b: centre.start() }, centre]))
}

/// The way off the contour at `e`, as [`approach`] the way on: an arc rolling off it, when one
/// is chosen, then the straight line where compensation goes off; the tool centre leaves from
/// `ce` and ends at the exit point itself.
#[allow(clippy::too_many_arguments)]
fn departure(arc: Option<CompArc>, e: Vec2, t: Vec2, n: Vec2, ce: Vec2, off: f64, length: f64, r: f64) -> Result<(Vec<Piece>, Vec<Piece>)> {
    let Some(arc) = arc else {
        let exit = e + n * length;
        return Ok((vec![Piece::Line { a: e, b: exit }], vec![Piece::Line { a: ce, b: exit }]));
    };
    let (programmed, centre) = comp_arc(lead_out(&arc.lead(), e, t, n), off, r)?;
    let end = programmed.end();
    let exit = end + programmed.tangent_at(1.0) * length;
    Ok((vec![programmed, Piece::Line { a: end, b: exit }], vec![centre, Piece::Line { a: centre.end(), b: exit }]))
}

/// A lead arc as programmed and as the tool centre runs it: round the same centre, `off`
/// closer to it (the tool rides on the arc's inner side, the side its centre is on).
fn comp_arc(arc: Option<Piece>, off: f64, r: f64) -> Result<(Piece, Piece)> {
    let Some(Piece::Arc { c, r: radius, a0, sweep }) = arc else {
        return invalid("a lead arc could not be laid onto the contour");
    };
    if off > 0.0 && radius < r + ARC_ROOM - 1e-9 {
        return invalid(format!(
            "with compensation on the control a lead arc must be larger than the tool's radius: the arc is {radius:.3} mm, the tool's radius {r:.3} mm; give the arc at least {:.3} mm, or choose a square lead",
            r + ARC_ROOM
        ));
    }
    Ok((Piece::Arc { c, r: radius, a0, sweep }, Piece::Arc { c, r: radius - off, a0, sweep }))
}

/// The unit vector along `v`.
fn direction(v: Vec2) -> Result<Vec2> {
    match v.normalized() {
        Some(n) if n.is_finite() => Ok(n),
        _ => invalid("the contour's start has no side for the tool"),
    }
}

/// The offset distance that moves `lp` onto `centre` (`sign` times the radius, the loop maybe
/// reversed since): which side of the loop's travel the tool is on.
fn side_sign(lp: &[Piece], centre: &[Piece], sign: f64) -> f64 {
    let (Some(first), Some(c)) = (lp.first(), centre.first()) else { return sign };
    let r = first.start().dist(c.start());
    if first.tangent_at(0.0).perp().dot(c.start() - first.start()) > 0.0 { r } else { -r }
}

/// The loop turned to start on a piece, at least a tool diameter from its ends where it is long
/// enough (its middle otherwise): nearest to `start`, or in the middle of the longest straight
/// piece (the longest piece when it has none). The control's entry lands square on it.
fn start_on_piece(lp: &[Piece], start: Option<Vec2>, diameter: f64) -> Vec<Piece> {
    let pick = match start {
        Some(p) => lp.iter().min_by(|a, b| a.dist(p).total_cmp(&b.dist(p))).map(|piece| (piece, piece.param_near(p))),
        None => {
            // The first of the longest, so the same profile always starts at the same place.
            let longest = |only_lines: bool| {
                lp.iter().filter(|piece| !only_lines || matches!(piece, Piece::Line { .. })).fold(None::<&Piece>, |best, piece| match best {
                    Some(b) if b.length() >= piece.length() - 1e-9 => Some(b),
                    _ => Some(piece),
                })
            };
            longest(true).or_else(|| longest(false)).map(|piece| (piece, 0.5))
        }
    };
    let Some((piece, t)) = pick else { return lp.to_vec() };
    let len = piece.length();
    let margin = if len > 0.0 { (diameter / len).min(0.5) } else { 0.5 };
    start_at(lp, piece.point_at(t.clamp(margin, 1.0 - margin)))
}

/// `run` (pieces end to end) offset by `d` the way a control compensates it: each line moved
/// square to itself, each arc to a radius `d` smaller or larger, a round corner of radius `|d|`
/// added where the path turns away from the offset side, and the two offsets cut back to where
/// they meet where it turns towards it. Refused where an arc or a piece leaves no room.
pub(crate) fn control_offset(run: &[Piece], d: f64) -> Result<Vec<Piece>> {
    let ad = d.abs();
    let mut moved = Vec::with_capacity(run.len());
    for piece in run {
        moved.push(match *piece {
            Piece::Line { a, b } => {
                let n = piece.tangent_at(0.0).perp() * d;
                Piece::Line { a: a + n, b: b + n }
            }
            Piece::Arc { c, r, a0, sweep } => {
                let radius = r - d * sweep.signum();
                if !(radius > 1e-6) {
                    let at = piece.point_at(0.5);
                    return invalid(format!(
                        "the arc of radius {r:.3} mm at ({:.3}, {:.3}) is tighter than the tool (radius {ad:.3} mm): the control would report gouging; use a smaller tool or compensate in the CAM",
                        at.x, at.y
                    ));
                }
                Piece::Arc { c, r: radius, a0, sweep }
            }
        });
    }
    let mut out = Vec::with_capacity(moved.len() * 2);
    let mut pieces = run.iter().zip(moved);
    let Some((mut piece, mut current)) = pieces.next() else { return Ok(out) };
    for (next, following) in pieces {
        let dir = piece.tangent_at(1.0);
        let bend = turn(dir, next.tangent_at(0.0));
        let corner = piece.end();
        if bend.abs() <= 1e-9 {
            out.push(current);
            current = following;
        } else if bend * d < 0.0 {
            out.push(current);
            out.push(Piece::Arc { c: corner, r: ad, a0: (dir.perp() * d.signum()).angle(), sweep: bend });
            current = following;
        } else {
            let tight = || {
                invalid(format!(
                    "the inside corner at ({:.3}, {:.3}) leaves no room for the tool (radius {ad:.3} mm) next to it: the control would report gouging; use a smaller tool or compensate in the CAM",
                    corner.x, corner.y
                ))
            };
            let Some((t, u)) = current.intersect_extended(&following) else { return tight() };
            if !(t > 1e-9 && t <= 1.0 + 1e-9 && (-1e-9..1.0 - 1e-9).contains(&u)) {
                return tight();
            }
            out.push(current.sub(0.0, t.min(1.0)));
            current = following.sub(u.max(0.0), 1.0);
        }
        piece = next;
    }
    out.push(current);
    Ok(out)
}

/// Whether the loops the control makes follow the CAM's tool centre loops, each way round
/// within [`MATCH_TOL`]: otherwise the control would leave the path somewhere, into the part.
fn matches_offset(made: &[Vec<Piece>], centres: &[Vec<Piece>], r: f64) -> Result<()> {
    let step = (r * 0.25).max(0.05);
    let near = |p: Vec2, loops: &[Vec<Piece>]| loops.iter().flatten().map(|piece| piece.dist(p)).fold(f64::INFINITY, f64::min);
    for (from, to) in [(made, centres), (centres, made)] {
        for piece in from.iter().flatten() {
            let n = ((piece.length() / step).ceil() as usize).clamp(2, 4096);
            for i in 0..=n {
                let p = piece.point_at(i as f64 / n as f64);
                if near(p, to) > MATCH_TOL {
                    return invalid(format!(
                        "near ({:.3}, {:.3}) the control's offset of the contour would not follow the tool's path: it would cut into the part (contours or walls closer together than the tool); compensate in the CAM, or use a smaller tool",
                        p.x, p.y
                    ));
                }
            }
        }
    }
    Ok(())
}
