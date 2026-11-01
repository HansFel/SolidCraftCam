//! Slots: a tool whose centre runs along a path of lines and arcs, open or closed. A slot as
//! wide as the tool is one pass on the path; a wider one adds passes offset to both sides until
//! the walls are reached.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::open::{OpenPath, check_self, offset_pieces};
use crate::ops::{Builder, Cut, mills};
use crate::path::Toolpath;
use crate::piece::Piece;
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Largest slot width, stock left or extension (mm).
const MAX_SLOT: f64 = 1.0e4;
/// How far apart a path's ends may be to count as closed (mm).
const CLOSED_TOL: f64 = 1e-6;
/// Most times a ramp goes back and forth along an open path's first part.
const MAX_RAMP_LEGS: f64 = 2000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotParams {
    /// Slot width (mm). `None` or the tool diameter: one pass on the path. Wider: after the
    /// centre pass, passes offset to both sides by at most `stepover` until the walls are reached
    /// (the outermost at `width/2 - radius - stock_to_leave` from the path).
    pub width: Option<f64>,
    /// Distance between side passes (mm), > 0 and at most the tool diameter.
    pub stepover: f64,
    /// Material left on both walls (mm, >= 0); ignored for a one-pass slot.
    pub stock_to_leave: f64,
    /// Chord error for spline/conic segments (mm), as [`crate::ContourParams`].
    pub tolerance: f64,
    /// Steepest descent into each depth pass, along the path (radians, 0..=PI/2); 0 plunges.
    /// An open path ramps back and forth (zigzag) along its first part.
    pub ramp_angle: f64,
}

impl SlotParams {
    pub fn validate(&self, tool: &Tool) -> Result<()> {
        if let Some(width) = self.width
            && (!width.is_finite() || !(width > 0.0) || width > MAX_SLOT)
        {
            return invalid("the slot width must be above 0");
        }
        let width = self.width.unwrap_or(tool.diameter);
        if width < tool.diameter - 1e-9 {
            return invalid("the slot is narrower than the tool");
        }
        if !self.stepover.is_finite() || !(self.stepover > 0.0) || self.stepover > tool.diameter + 1e-9 {
            return invalid("the stepover must be above 0 and at most the tool diameter");
        }
        if !self.stock_to_leave.is_finite() || !(0.0..=MAX_SLOT).contains(&self.stock_to_leave) {
            return invalid("the stock to leave must be 0 or more");
        }
        if !self.tolerance.is_finite() || !(self.tolerance > 0.0) {
            return invalid("the tolerance must be above 0");
        }
        if !self.ramp_angle.is_finite() || !(0.0..=PI / 2.0).contains(&self.ramp_angle) {
            return invalid("the ramp angle must be between 0 and 90 degrees");
        }
        Ok(())
    }

    /// The passes as tool-centre paths: the path itself, then copies offset to both sides.
    fn passes(&self, path: &[Piece], tool: &Tool, closed: bool) -> Result<Vec<Vec<Piece>>> {
        let width = self.width.unwrap_or(tool.diameter);
        // Where the outermost pass puts the tool centre: its outer edge at width/2 - stock.
        let wall = width * 0.5 - tool.radius() - self.stock_to_leave;
        let mut steps = vec![0.0];
        if wall > 1e-9 {
            let mut d = self.stepover;
            while d < wall - 1e-9 {
                steps.push(d);
                d += self.stepover;
            }
            steps.push(wall);
        }
        let mut passes = Vec::with_capacity(steps.len() * 2);
        passes.push(path.to_vec());
        for &d in steps.iter().skip(1) {
            for side in [d, -d] {
                let offset = offset_pieces(path, side, closed)?;
                check_self(&offset, closed)?;
                passes.push(offset);
            }
        }
        Ok(passes)
    }
}

/// Mills a slot along each path through every depth of `cut`. A path whose end meets its start
/// (within 1e-6) is closed.
pub fn slot(paths: &[OpenPath], tool: &Tool, params: &SlotParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    params.validate(tool)?;
    if paths.is_empty() || paths.len() > 1_000 {
        return invalid("slotting needs 1 to 1000 paths");
    }
    let levels = cut.levels()?;
    let mut b = Builder::new(cut);
    for path in paths {
        let pieces = path.pieces(params.tolerance, true)?;
        let closed = is_closed(&pieces);
        let passes = params.passes(&pieces, tool, closed)?;
        let mut first = true;
        for pass in &passes {
            let Some(start) = pass.first().map(Piece::start) else { continue };
            for (k, &z) in levels.iter().enumerate() {
                // A closed pass ends where it began, at depth: the next one carries on from there.
                let stay = k > 0 && closed;
                if first && k == 0 {
                    b.travel_to(start)?;
                } else if !stay {
                    b.return_to(start)?;
                }
                if params.ramp_angle > 0.0 {
                    // Down to the top of the material at the plunge feed, then into it along the
                    // path; a closed pass ramps round itself, an open one back and forth.
                    b.plunge_to(cut.heights.top)?;
                    if closed {
                        b.ramp_loop(pass, z, params.ramp_angle)?;
                    } else {
                        ramp_open(&mut b, pass, z, params.ramp_angle)?;
                    }
                } else {
                    b.plunge_to(z)?;
                }
                b.cut_loop(pass, z)?;
            }
            b.retract()?;
            first = false;
        }
    }
    b.finish()
}

/// Whether a path's end meets its start.
fn is_closed(path: &[Piece]) -> bool {
    match (path.first(), path.last()) {
        (Some(a), Some(b)) => a.start().dist(b.end()) <= CLOSED_TOL,
        _ => false,
    }
}

/// Down to `z` along the first part of an open path, back and forth as often as the drop takes,
/// ending at the path's start at depth.
fn ramp_open(b: &mut Builder, path: &[Piece], z: f64, angle: f64) -> Result<()> {
    let Some(from) = b.pos().map(|p| p.z) else {
        return Err(CamError::Unsafe("a ramp before the first positioning move".into()));
    };
    let drop = from - z;
    if !(drop > 1e-9) {
        return Ok(());
    }
    let length: f64 = path.iter().map(Piece::length).sum();
    let reach = drop / angle.tan();
    if !(angle > 0.0) || !(length > 1e-3) || !reach.is_finite() {
        return b.plunge_to(z);
    }
    let part_len = reach.min(length);
    let legs = ((reach / part_len - 1e-9).ceil()).max(2.0);
    // An even number of legs ends where they began, so the cut starts at the path's start.
    let legs = if (legs as usize) % 2 == 1 { legs + 1.0 } else { legs };
    if !(legs <= MAX_RAMP_LEGS) {
        return b.plunge_to(z);
    }
    let part = first_part(path, part_len);
    if part.is_empty() {
        return b.plunge_to(z);
    }
    let step = drop / legs;
    for k in 0..legs as usize {
        let (z0, z1) = (from - step * k as f64, from - step * (k as f64 + 1.0));
        let leg: Vec<Piece> = if k % 2 == 0 { part.clone() } else { part.iter().rev().map(Piece::reversed).collect() };
        for piece in &leg {
            b.cut_piece(piece, z0, z1)?;
        }
    }
    Ok(())
}

/// The first `length` mm of `path`.
fn first_part(path: &[Piece], length: f64) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut left = length;
    for piece in path {
        if !(left > 1e-9) {
            break;
        }
        let len = piece.length();
        if len >= left {
            out.push(piece.sub(0.0, left / len));
            left = 0.0;
        } else {
            out.push(*piece);
            left -= len;
        }
    }
    out
}
