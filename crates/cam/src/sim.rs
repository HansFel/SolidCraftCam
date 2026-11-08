//! Material removal simulation: the stock as a height field (a z-map) over work XY.
//!
//! Every cell holds the stock's top there. A toolpath lowers the cells its tool sweeps over,
//! and the difference is the material removed. This is enough for 2.5D and 3-axis milling; it
//! knows nothing about undercuts or a tool's side.

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::path::{Move, Toolpath, arc_sweep};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most cells one height field may have.
pub const MAX_CELLS: usize = 16_000_000;
/// Largest coordinate a stock may reach (mm).
const MAX_COORD: f64 = 1.0e6;
/// Most points one move is sampled at.
const MAX_MOVE_SAMPLES: f64 = 4_000_000.0;
/// Most points one [`StockMap::cut`] samples in total.
const MAX_TOTAL_SAMPLES: f64 = 40_000_000.0;
/// How many move indices a report list keeps.
const MAX_REPORTS: usize = 100;
/// Tangent of the half angle of a 118 degree drill point (59 degrees).
const DRILL_TIP_TAN: f64 = 1.664_279_482_350_9;

/// Stock as a height field over work XY, cells of `cell` mm. Heights in f32 (mm).
#[derive(Clone, Debug)]
pub struct StockMap {
    origin: Vec2,
    cell: f64,
    nx: usize,
    ny: usize,
    bottom: f64,
    top: Vec<f32>,
}

/// What cutting a toolpath did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SimReport {
    /// Material removed (mm^3).
    pub removed: f64,
    /// Indices into `moves` of rapid moves that went through stock (a crash), first 100.
    pub rapid_hits: Vec<usize>,
    /// Indices of feed/arc moves where material reached above the flute length (the holder or
    /// shank would rub), first 100.
    pub too_deep: Vec<usize>,
}

/// Material left above the target, per cell (0 where none, or where the target is `None`).
#[derive(Clone, Debug, PartialEq)]
pub struct RestMap {
    pub nx: usize,
    pub ny: usize,
    pub rest: Vec<f32>,
    pub max: f64,
    pub max_at: Option<Vec2>,
}

impl StockMap {
    /// A box from `min` to `max` (work coordinates). Refuses a cell <= 0 or not finite, an empty
    /// box, and more than 16_000_000 cells.
    pub fn new_box(min: Vec3, max: Vec3, cell: f64) -> Result<StockMap> {
        if !min.is_finite() || !max.is_finite() {
            return invalid("the stock box must be finite");
        }
        if [min.x, min.y, min.z, max.x, max.y, max.z].iter().any(|v| v.abs() > MAX_COORD) {
            return invalid(format!("the stock box must stay within {MAX_COORD} mm"));
        }
        if !(max.x > min.x) || !(max.y > min.y) || !(max.z > min.z) {
            return invalid("the stock box has no volume");
        }
        let (nx, ny) = grid_size(max.x - min.x, max.y - min.y, cell)?;
        let top = vec![max.z as f32; nx.saturating_mul(ny)];
        Ok(StockMap { origin: Vec2::new(min.x, min.y), cell, nx, ny, bottom: min.z, top })
    }

    /// A round bar along Z, centre `c`, radius `r`, from `z0` to `z1` (cells outside the circle
    /// hold no stock: height = bottom).
    pub fn new_round(c: Vec2, r: f64, z0: f64, z1: f64, cell: f64) -> Result<StockMap> {
        if !c.is_finite() || !r.is_finite() || !z0.is_finite() || !z1.is_finite() {
            return invalid("the stock bar must have finite values");
        }
        if !(r > 0.0) || r > MAX_COORD || c.x.abs() + r > MAX_COORD || c.y.abs() + r > MAX_COORD {
            return invalid("the stock bar needs a finite radius above 0");
        }
        if z0.abs() > MAX_COORD || z1.abs() > MAX_COORD {
            return invalid(format!("the stock bar must stay within {MAX_COORD} mm"));
        }
        if !(z1 > z0) {
            return invalid("the stock bar needs a height");
        }
        let (nx, ny) = grid_size(2.0 * r, 2.0 * r, cell)?;
        let origin = Vec2::new(c.x - r, c.y - r);
        let mut top = vec![z0 as f32; nx.saturating_mul(ny)];
        for j in 0..ny {
            for i in 0..nx {
                let p = Vec2::new(origin.x + (i as f64 + 0.5) * cell, origin.y + (j as f64 + 0.5) * cell);
                if p.dist(c) <= r + 1e-9
                    && let Some(cell_height) = top.get_mut(j.saturating_mul(nx).saturating_add(i))
                {
                    *cell_height = z1 as f32;
                }
            }
        }
        Ok(StockMap { origin, cell, nx, ny, bottom: z0, top })
    }

    /// Removes what `tool` sweeps along `path`, starting from `start` (the tool tip before the
    /// first move). A flat end mill cuts a disc, a ball end mill a hemisphere and a drill its
    /// 118 degree cone tip. Rapid moves cut nothing: they are only checked for a crash.
    pub fn cut(&mut self, tool: &Tool, path: &Toolpath, start: Vec3) -> Result<SimReport> {
        tool.validate()?;
        if self.nx == 0 || self.ny == 0 {
            return invalid("the simulation grid is empty");
        }
        if !start.is_finite() {
            return invalid("the simulation start point must be finite");
        }
        if path.moves.len() > crate::path::MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {} moves", crate::path::MAX_MOVES)));
        }
        let mut report = SimReport::default();
        let mut removed = 0.0;
        let mut total = 0.0;
        let mut pos = start;
        for (index, movement) in path.moves.iter().enumerate() {
            match *movement {
                Move::Dwell { .. } => {}
                Move::Rapid { to } => {
                    if !to.is_finite() {
                        return invalid(format!("move {index} is not a finite point"));
                    }
                    let n = samples(pos, to, None, self.cell)?;
                    total += n;
                    if total > MAX_TOTAL_SAMPLES {
                        return Err(CamError::TooLarge("the simulation would sample too many points".into()));
                    }
                    let n = n as usize;
                    let mut hit = false;
                    for k in 1..=n {
                        let p = pos.lerp(to, k as f64 / n as f64);
                        if self.collides(tool, p) {
                            hit = true;
                            break;
                        }
                    }
                    if hit && report.rapid_hits.len() < MAX_REPORTS {
                        report.rapid_hits.push(index);
                    }
                    pos = to;
                }
                Move::Feed { to, .. } => {
                    if !to.is_finite() {
                        return invalid(format!("move {index} is not a finite point"));
                    }
                    let n = samples(pos, to, None, self.cell)?;
                    total += n;
                    if total > MAX_TOTAL_SAMPLES {
                        return Err(CamError::TooLarge("the simulation would sample too many points".into()));
                    }
                    let n = n as usize;
                    let mut deep = false;
                    for k in 1..=n {
                        let p = pos.lerp(to, k as f64 / n as f64);
                        deep |= self.cut_at(tool, p, &mut removed);
                    }
                    if deep && report.too_deep.len() < MAX_REPORTS {
                        report.too_deep.push(index);
                    }
                    pos = to;
                }
                Move::Arc { to, center, ccw, .. } => {
                    if !to.is_finite() {
                        return invalid(format!("move {index} is not a finite point"));
                    }
                    if !center.is_finite() {
                        return invalid(format!("move {index}: the arc's centre must be finite"));
                    }
                    let from = Vec2::new(pos.x, pos.y);
                    let radius = from.dist(center);
                    if !(radius > 1e-9) {
                        return invalid(format!("move {index}: the arc has no radius"));
                    }
                    let sweep = arc_sweep(pos, to, center, ccw);
                    let n = samples(pos, to, Some(radius * sweep.abs()), self.cell)?;
                    total += n;
                    if total > MAX_TOTAL_SAMPLES {
                        return Err(CamError::TooLarge("the simulation would sample too many points".into()));
                    }
                    let (n, a0) = (n as usize, (from - center).angle());
                    let mut deep = false;
                    for k in 1..=n {
                        let t = k as f64 / n as f64;
                        let p = if k == n {
                            to
                        } else {
                            let xy = center + Vec2::from_angle(a0 + sweep * t) * radius;
                            xy.extend(pos.z + (to.z - pos.z) * t)
                        };
                        deep |= self.cut_at(tool, p, &mut removed);
                    }
                    if deep && report.too_deep.len() < MAX_REPORTS {
                        report.too_deep.push(index);
                    }
                    pos = to;
                }
            }
        }
        report.removed = removed;
        Ok(report)
    }

    /// The grid size in cells (x, y).
    pub fn size(&self) -> (usize, usize) {
        (self.nx, self.ny)
    }

    pub fn cell(&self) -> f64 {
        self.cell
    }

    pub fn origin(&self) -> Vec2 {
        self.origin
    }

    /// Stock top of cell (i, j); `None` outside the grid.
    pub fn height(&self, i: usize, j: usize) -> Option<f64> {
        if i >= self.nx || j >= self.ny {
            return None;
        }
        self.top.get(j.saturating_mul(self.nx).saturating_add(i)).map(|h| f64::from(*h))
    }

    /// Triangles for the viewport: top surface plus the four side walls, positions in work mm,
    /// one normal per vertex. At most `max_triangles` (cells are merged into blocks when the
    /// grid is larger).
    pub fn mesh(&self, max_triangles: usize) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
        if self.nx == 0 || self.ny == 0 || max_triangles == 0 {
            return (Vec::new(), Vec::new(), Vec::new());
        }
        let triangles = |s: usize| -> usize {
            let bx = self.nx.div_ceil(s);
            let by = self.ny.div_ceil(s);
            2usize.saturating_mul(bx).saturating_mul(by).saturating_add(4usize.saturating_mul(bx.saturating_add(by)))
        };
        let coarsest = self.nx.max(self.ny).max(1);
        if triangles(coarsest) > max_triangles {
            return (Vec::new(), Vec::new(), Vec::new());
        }
        let (mut lo, mut hi) = (1usize, coarsest);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if triangles(mid) <= max_triangles { hi = mid } else { lo = mid + 1 }
        }
        let s = lo;
        let bx = self.nx.div_ceil(s);
        let by = self.ny.div_ceil(s);
        let mut mesh = MeshBuilder::default();
        let bottom = self.bottom as f32;
        for l in 0..by {
            let j0 = l.saturating_mul(s);
            let j1 = j0.saturating_add(s).min(self.ny);
            for k in 0..bx {
                let i0 = k.saturating_mul(s);
                let i1 = i0.saturating_add(s).min(self.nx);
                let mut h = self.bottom as f32;
                for j in j0..j1 {
                    for i in i0..i1 {
                        if let Some(v) = self.top.get(j.saturating_mul(self.nx).saturating_add(i))
                            && *v > h
                        {
                            h = *v;
                        }
                    }
                }
                let x0 = (self.origin.x + i0 as f64 * self.cell) as f32;
                let x1 = (self.origin.x + i1 as f64 * self.cell) as f32;
                let y0 = (self.origin.y + j0 as f64 * self.cell) as f32;
                let y1 = (self.origin.y + j1 as f64 * self.cell) as f32;
                mesh.quad([[x0, y0, h], [x1, y0, h], [x1, y1, h], [x0, y1, h]], [0.0, 0.0, 1.0]);
                if l == 0 {
                    mesh.quad([[x0, y0, bottom], [x1, y0, bottom], [x1, y0, h], [x0, y0, h]], [0.0, -1.0, 0.0]);
                }
                if l + 1 == by {
                    mesh.quad([[x0, y1, bottom], [x0, y1, h], [x1, y1, h], [x1, y1, bottom]], [0.0, 1.0, 0.0]);
                }
                if k == 0 {
                    mesh.quad([[x0, y0, bottom], [x0, y0, h], [x0, y1, h], [x0, y1, bottom]], [-1.0, 0.0, 0.0]);
                }
                if k + 1 == bx {
                    mesh.quad([[x1, y0, bottom], [x1, y1, bottom], [x1, y1, h], [x1, y0, h]], [1.0, 0.0, 0.0]);
                }
            }
        }
        (mesh.positions, mesh.normals, mesh.indices)
    }

    /// Rest material against a target height per cell (e.g. the model's top sampled per cell):
    /// for each cell `max(0, stock - target)`, plus the largest value and its cell.
    pub fn rest(&self, target: &dyn Fn(Vec2) -> Option<f64>) -> RestMap {
        let mut rest = vec![0.0f32; self.nx.saturating_mul(self.ny)];
        let mut max = 0.0f64;
        let mut max_at = None;
        for j in 0..self.ny {
            for i in 0..self.nx {
                let Some(stock) = self.height(i, j) else { continue };
                let p = Vec2::new(self.origin.x + (i as f64 + 0.5) * self.cell, self.origin.y + (j as f64 + 0.5) * self.cell);
                let Some(wanted) = target(p) else { continue };
                if !wanted.is_finite() {
                    continue;
                }
                let left = (stock - wanted).max(0.0);
                if let Some(slot) = rest.get_mut(j.saturating_mul(self.nx).saturating_add(i)) {
                    *slot = left as f32;
                }
                if left > max {
                    max = left;
                    max_at = Some(p);
                }
            }
        }
        RestMap { nx: self.nx, ny: self.ny, rest, max, max_at }
    }

    /// Whether the tool at `p` touches stock.
    fn collides(&self, tool: &Tool, p: Vec3) -> bool {
        let r = tool.radius();
        let (i0, i1) = self.span_x(p.x, r);
        let (j0, j1) = self.span_y(p.y, r);
        let r2 = r * r;
        for j in j0..j1 {
            let dy = self.origin.y + (j as f64 + 0.5) * self.cell - p.y;
            for i in i0..i1 {
                let Some(cell) = self.top.get(j.saturating_mul(self.nx).saturating_add(i)) else { continue };
                let old = f64::from(*cell);
                if old <= self.bottom + 1e-9 {
                    continue;
                }
                let dx = self.origin.x + (i as f64 + 0.5) * self.cell - p.x;
                let d2 = dx * dx + dy * dy;
                if d2 > r2 {
                    continue;
                }
                if old > surface(tool, p.z, d2.sqrt()) + 1e-6 {
                    return true;
                }
            }
        }
        false
    }

    /// Lowers every cell the tool reaches at `p`, adding the material removed to `removed`.
    /// Returns whether material in reach reached above the tool's flutes.
    fn cut_at(&mut self, tool: &Tool, p: Vec3, removed: &mut f64) -> bool {
        let r = tool.radius();
        let (i0, i1) = self.span_x(p.x, r);
        let (j0, j1) = self.span_y(p.y, r);
        let r2 = r * r;
        let flute_top = p.z + tool.flute_length;
        let mut deep = false;
        for j in j0..j1 {
            let dy = self.origin.y + (j as f64 + 0.5) * self.cell - p.y;
            for i in i0..i1 {
                let Some(cell) = self.top.get(j.saturating_mul(self.nx).saturating_add(i)) else { continue };
                let old = f64::from(*cell);
                if old <= self.bottom + 1e-9 {
                    continue;
                }
                let dx = self.origin.x + (i as f64 + 0.5) * self.cell - p.x;
                let d2 = dx * dx + dy * dy;
                if d2 > r2 {
                    continue;
                }
                let surf = surface(tool, p.z, d2.sqrt());
                if surf >= old {
                    continue;
                }
                if old > flute_top + 1e-6 {
                    deep = true;
                }
                let new = surf.max(self.bottom);
                if new < old {
                    *removed += (old - new) * self.cell * self.cell;
                    if let Some(slot) = self.top.get_mut(j.saturating_mul(self.nx).saturating_add(i)) {
                        *slot = new as f32;
                    }
                }
            }
        }
        deep
    }

    /// The cell indices along X whose centre lies within `r` of `x`.
    fn span_x(&self, x: f64, r: f64) -> (usize, usize) {
        self.span(self.origin.x, self.nx, x - r, x + r)
    }

    fn span_y(&self, y: f64, r: f64) -> (usize, usize) {
        self.span(self.origin.y, self.ny, y - r, y + r)
    }

    fn span(&self, origin: f64, n: usize, lo: f64, hi: f64) -> (usize, usize) {
        let a = clamp_index(((lo - origin) / self.cell).floor() - 1.0, n);
        let b = clamp_index(((hi - origin) / self.cell).ceil() + 1.0, n);
        (a, b.max(a))
    }
}

/// The stock top a tool's cutting surface reaches at distance `d` from its axis, tip at `tip`.
fn surface(tool: &Tool, tip: f64, d: f64) -> f64 {
    match tool.kind {
        ToolKind::FlatEndmill => tip,
        ToolKind::BallEndmill => {
            let r = tool.radius();
            tip + r - (r * r - d * d).max(0.0).sqrt()
        }
        ToolKind::Drill if tool.tip_angle.is_none() => tip + d / DRILL_TIP_TAN,
        // A cone: flat to the tip's radius, then rising at the half angle.
        ToolKind::Drill | ToolKind::Chamfer | ToolKind::VBit | ToolKind::CentreDrill => {
            let rise = (d - tool.tip_radius()).max(0.0);
            match tool.half_angle().map(f64::tan).filter(|t| *t > 1e-9) {
                Some(tan) => tip + rise / tan,
                None => tip,
            }
        }
        // A torus at the corner: flat inside, rounding up over the last corner radius.
        ToolKind::BullNose => {
            let rc = tool.corner_radius.unwrap_or(0.0).clamp(0.0, tool.radius());
            let x = d - (tool.radius() - rc);
            if x <= 0.0 { tip } else { tip + rc - (rc * rc - x * x).max(0.0).sqrt() }
        }
        ToolKind::TSlot | ToolKind::Reamer | ToolKind::Tap | ToolKind::ThreadMill | ToolKind::Plasma => tip,
    }
}

/// How many points a move is sampled at, so no sample is more than half a cell from the next.
/// `arc_length` is given for an arc, `None` for a line (its XY length is used).
fn samples(from: Vec3, to: Vec3, arc_length: Option<f64>, cell: f64) -> Result<f64> {
    let length = arc_length.unwrap_or_else(|| Vec2::new(from.x, from.y).dist(Vec2::new(to.x, to.y)));
    let n = if length > 0.0 { (length / (cell * 0.5)).ceil() } else { 1.0 };
    if !n.is_finite() || n > MAX_MOVE_SAMPLES {
        return Err(CamError::TooLarge("the simulation would sample too many points along one move".into()));
    }
    Ok(n.max(1.0))
}

fn clamp_index(v: f64, n: usize) -> usize {
    if !(v > 0.0) {
        0
    } else if v >= n as f64 {
        n
    } else {
        v as usize
    }
}

fn grid_size(span_x: f64, span_y: f64, cell: f64) -> Result<(usize, usize)> {
    if !cell.is_finite() || cell <= 0.0 {
        return invalid("the simulation cell must be above 0 and finite");
    }
    if !span_x.is_finite() || !span_y.is_finite() || !(span_x > 0.0) || !(span_y > 0.0) {
        return invalid("the simulation grid needs a stock with some width and depth");
    }
    let nx = (span_x / cell).ceil();
    let ny = (span_y / cell).ceil();
    if !nx.is_finite() || !ny.is_finite() || nx <= 0.0 || ny <= 0.0 {
        return invalid("the simulation grid needs a stock with some width and depth");
    }
    if nx * ny > MAX_CELLS as f64 {
        return invalid(format!("the simulation grid is too fine: {nx} x {ny} cells, more than {MAX_CELLS}"));
    }
    Ok((nx as usize, ny as usize))
}

/// Collects vertices, normals and indices for [`StockMap::mesh`].
#[derive(Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    fn quad(&mut self, corners: [[f32; 3]; 4], normal: [f32; 3]) {
        let base = self.positions.len() as u32;
        for corner in corners {
            self.positions.push(corner);
            self.normals.push(normal);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}
