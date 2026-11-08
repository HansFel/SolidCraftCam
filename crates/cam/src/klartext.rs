//! Programs in Heidenhain's conversational language (Klartext), as the TNC controls read it:
//! numbered blocks between `BEGIN PGM` and `END PGM`, `TOOL CALL`, straight lines `L` (`FMAX`
//! for rapids), arcs as a centre `CC` and a `C` with its direction `DR+`/`DR-`, dwells as cycle 9.
//! [`emit`] writes a program, [`parse_lines`] reads its motion back for the same check every
//! G-code program gets. Helices are written as short lines, holes move by move, and tapped holes
//! are refused (cycle 207 is not written yet).
//!
//! Written from Heidenhain's public conversational programming descriptions; check the first
//! programs in the control's own test run before cutting.

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::gcode::Program;
use crate::path::{Move, arc_sweep};
use crate::post::{CommentStyle, Post};
use crate::{CamError, Result, invalid};

/// Most lines [`parse_lines`] reads.
const MAX_LINES: usize = 5_000_000;
/// Longest program name and comment.
const MAX_TEXT: usize = 60;

/// Text safe in a block: letters, digits and a few signs; German letters spelled out.
fn plain(text: &str) -> String {
    let spelled = text.chars().flat_map(|c| {
        let ascii = match c {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'Ä' => "Ae",
            'Ö' => "Oe",
            'Ü' => "Ue",
            'ß' => "ss",
            _ => "",
        };
        let kept = (c.is_ascii_alphanumeric() || " ._-+/:=,#".contains(c)).then_some(c);
        ascii.chars().chain(kept)
    });
    spelled.take(MAX_TEXT).collect::<String>().trim().to_string()
}

/// A program name a TNC takes: letters, digits and `_`, at most 16 of them.
fn program_name(name: &str) -> String {
    let n: String = plain(name).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(16).collect();
    if n.is_empty() { "PGM".into() } else { n }
}

struct Writer<'a> {
    post: &'a Post,
    out: String,
    line: u32,
    x: Option<String>,
    y: Option<String>,
    z: Option<String>,
    pos: Option<Vec3>,
    feed: Option<String>,
    /// Whether the last block was a rapid: `FMAX` holds for its own block only, so the next
    /// cutting block states its feed again.
    after_rapid: bool,
    /// M functions waiting for the next positioning block.
    pending: Vec<&'static str>,
}

impl Writer<'_> {
    fn block(&mut self, text: &str) {
        self.out.push_str(&format!("{} {text}\n", self.line));
        self.line = self.line.saturating_add(1);
    }

    fn comment(&mut self, text: &str) {
        if self.post.comments != CommentStyle::None {
            let text = plain(text);
            self.block(&format!("; {text}"));
        }
    }

    /// A signed coordinate: `+10.5`, `-2`.
    fn coord(&self, mm: f64) -> String {
        let v = mm / self.post.units.mm();
        let s = format!("{v:.*}", usize::from(self.post.decimals));
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
        let s = if s == "-0" { "0".to_string() } else { s };
        if s.starts_with('-') { s } else { format!("+{s}") }
    }

    fn feed_value(&self, feed: f64) -> String {
        let v = feed / self.post.units.mm();
        let s = format!("{v:.*}", usize::from(self.post.feed_decimals));
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    }

    fn take_m(&mut self) -> String {
        let m: Vec<&str> = std::mem::take(&mut self.pending);
        m.iter().map(|m| format!(" {m}")).collect()
    }

    /// `L` to `to` on the axes that change; rapid (`FMAX`) or at `feed`.
    fn line_to(&mut self, to: Vec3, xy: bool, z: bool, feed: Option<f64>) -> Result<()> {
        let (nx, ny, nz) = (self.coord(to.x), self.coord(to.y), self.coord(to.z));
        let mut words = String::new();
        if xy && self.x.as_ref() != Some(&nx) {
            words.push_str(&format!(" X{nx}"));
            self.x = Some(nx);
        }
        if xy && self.y.as_ref() != Some(&ny) {
            words.push_str(&format!(" Y{ny}"));
            self.y = Some(ny);
        }
        if z && self.z.as_ref() != Some(&nz) {
            words.push_str(&format!(" Z{nz}"));
            self.z = Some(nz);
        }
        if words.is_empty() {
            return Ok(());
        }
        let speed = match feed {
            None => {
                self.after_rapid = true;
                " FMAX".to_string()
            }
            Some(f) => {
                if !(f > 0.0) || !f.is_finite() || self.feed_value(f) == "0" {
                    return Err(CamError::Unsafe("a feed is not above 0 at the post's precision".into()));
                }
                let value = self.feed_value(f);
                let word = if self.after_rapid || self.feed.as_ref() != Some(&value) { format!(" F{value}") } else { String::new() };
                (self.feed, self.after_rapid) = (Some(value), false);
                word
            }
        };
        let m = self.take_m();
        self.block(&format!("L{words} R0{speed}{m}"));
        Ok(())
    }

    fn rapid(&mut self, to: Vec3) -> Result<()> {
        // Up first when going up (and when nothing is known yet), down last when going down.
        if self.pos.is_none_or(|p| to.z > p.z) {
            self.line_to(to, false, true, None)?;
            self.line_to(to, true, false, None)?;
        } else {
            self.line_to(to, true, false, None)?;
            self.line_to(to, false, true, None)?;
        }
        self.pos = Some(to);
        Ok(())
    }

    fn feed(&mut self, to: Vec3, feed: f64) -> Result<()> {
        self.line_to(to, true, true, Some(feed))?;
        self.pos = Some(to);
        Ok(())
    }

    fn arc(&mut self, to: Vec3, center: Vec2, ccw: bool, feed: f64) -> Result<()> {
        let Some(from) = self.pos else { return Err(CamError::Unsafe("an arc before the first positioning move".into())) };
        let (nx, ny, nz) = (self.coord(to.x), self.coord(to.y), self.coord(to.z));
        let flat = self.z.as_ref() == Some(&nz);
        // An end written as the start would be a full circle; a helix goes out as lines.
        if (self.x.as_ref() == Some(&nx) && self.y.as_ref() == Some(&ny)) || !flat || !self.post.arcs {
            if self.x.as_ref() == Some(&nx) && self.y.as_ref() == Some(&ny) {
                return self.feed(to, feed);
            }
            let start = Vec2::new(from.x, from.y) - center;
            let (r, sweep) = (start.len(), arc_sweep(from, to, center, ccw));
            let tol = self.post.arc_tolerance;
            let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { std::f64::consts::FRAC_PI_4 };
            let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
            for i in 1..n {
                let t = i as f64 / n as f64;
                let p = center + Vec2::from_angle(start.angle() + sweep * t) * r;
                self.feed(p.extend(from.z + (to.z - from.z) * t), feed)?;
            }
            return self.feed(to, feed);
        }
        if !(feed > 0.0) || !feed.is_finite() || self.feed_value(feed) == "0" {
            return Err(CamError::Unsafe("a feed is not above 0 at the post's precision".into()));
        }
        let (cx, cy) = (self.coord(center.x), self.coord(center.y));
        self.block(&format!("CC X{cx} Y{cy}"));
        let value = self.feed_value(feed);
        let word = if self.after_rapid || self.feed.as_ref() != Some(&value) { format!(" F{value}") } else { String::new() };
        (self.feed, self.after_rapid) = (Some(value), false);
        let m = self.take_m();
        self.block(&format!("C X{nx} Y{ny} DR{} R0{word}{m}", if ccw { "+" } else { "-" }));
        (self.x, self.y, self.pos) = (Some(nx), Some(ny), Some(to));
        Ok(())
    }
}

/// Writes `program` in Klartext. The work offset is the control's active preset: a program for
/// another offset than the first is refused, as Klartext has no `G54` to say it.
pub fn emit(program: &Program, post: &Post) -> Result<String> {
    if program.operations.is_empty() {
        return invalid("the program has no operation");
    }
    if program.work_offset.unwrap_or(1) != 1 {
        return invalid("a Heidenhain program runs in the control's active preset: set the preset there and give the setup work offset 1");
    }
    let units = match post.units {
        crate::post::Units::Mm => "MM",
        crate::post::Units::Inch => "INCH",
    };
    let name = program_name(&program.name);
    let mut w = Writer { post, out: String::new(), line: 0, x: None, y: None, z: None, pos: None, feed: None, after_rapid: true, pending: Vec::new() };
    w.block(&format!("BEGIN PGM {name} {units}"));
    w.comment(&program.name);
    let mut tool: Option<u32> = None;
    let mut rpm: Option<String> = None;
    let mut coolant = false;
    for op in &program.operations {
        op.tool.validate()?;
        if !(op.spindle_rpm > 0.0 && op.spindle_rpm <= 1.0e6) {
            return invalid(format!("operation `{}`: the spindle speed must be above 0", plain(&op.name)));
        }
        if op.path.cycles.iter().any(|c| c.tap.is_some()) {
            return invalid(format!("post `{}` cannot write a tapped hole yet (cycle 207)", plain(&post.name)));
        }
        if op.tool.kind == crate::ToolKind::Plasma {
            return invalid(format!("post `{}` writes no plasma cuts: use a G-code post such as `plasma`", plain(&post.name)));
        }
        let speed = format!("{:.0}", op.spindle_rpm);
        w.comment(&format!("{}: T{} D{:.3} {}", op.name, op.tool.number, op.tool.diameter, op.tool.kind.label()));
        if tool != Some(op.tool.number) || rpm.as_ref() != Some(&speed) {
            if tool != Some(op.tool.number) && coolant {
                w.pending.push("M9");
                coolant = false;
            }
            // A tool call stops nothing by itself: the spindle starts with the next move.
            if !w.pending.is_empty()
                && let Some(p) = w.pos
            {
                let m = w.take_m();
                w.block(&format!("L Z{} R0 FMAX{m}", w.coord(p.z)));
            }
            let number = if tool == Some(op.tool.number) { String::new() } else { format!(" {}", op.tool.number) };
            w.block(&format!("TOOL CALL{number} Z S{speed}"));
            (tool, rpm) = (Some(op.tool.number), Some(speed));
            // After a tool change the position is unknown: the next rapid states every axis.
            (w.x, w.y, w.z, w.pos, w.feed, w.after_rapid) = (None, None, None, None, None, true);
            w.pending.push("M3");
        }
        if op.coolant != coolant {
            w.pending.push(if op.coolant { "M8" } else { "M9" });
            coolant = op.coolant;
        }
        for m in &op.path.moves {
            if m.target().is_some_and(|to| !to.is_finite()) {
                return Err(CamError::Unsafe(format!("operation `{}` has a coordinate that is not a finite number", plain(&op.name))));
            }
            match *m {
                Move::Rapid { to } => w.rapid(to)?,
                Move::Feed { to, feed } => w.feed(to, feed)?,
                Move::Arc { to, center, ccw, feed } => w.arc(to, center, ccw, feed)?,
                Move::Dwell { seconds } => {
                    if !(seconds >= 0.0) || !seconds.is_finite() {
                        return Err(CamError::Unsafe("a dwell must be 0 seconds or longer".into()));
                    }
                    w.block("CYCL DEF 9.0 DWELL TIME");
                    w.block(&format!("CYCL DEF 9.1 DWELL {seconds:.3}"));
                }
            }
        }
    }
    // Spindle and coolant off, and the end of the program, on a last block where the tool is.
    w.pending.push("M5");
    if coolant {
        w.pending.push("M9");
    }
    w.pending.push("M30");
    let z = w.pos.map_or_else(|| "+0".to_string(), |p| w.coord(p.z));
    let m = w.take_m();
    w.block(&format!("L Z{z} R0 FMAX{m}"));
    w.block(&format!("END PGM {name} {units}"));
    Ok(w.out)
}

/// A signed number after `letter` in `word` (`X+10.5` → 10.5).
fn value(word: &str, letter: &str) -> Option<f64> {
    word.strip_prefix(letter).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite())
}

/// Reads the motion of a Klartext program: `L` with `X Y Z`, `FMAX` or `F`; `CC` and `C` with
/// `DR+`/`DR-`; dwells of cycle 9. Each move comes with the number of the line it is on (from 1).
pub fn parse_lines(text: &str) -> Result<Vec<(usize, Move)>> {
    let (mut x, mut y, mut z): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
    let mut units = 1.0;
    let mut feed: Option<f64> = None;
    let mut center: Option<Vec2> = None;
    let mut moves = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if n > MAX_LINES {
            return Err(CamError::TooLarge(format!("more than {MAX_LINES} lines")));
        }
        let code = line.split(';').next().unwrap_or_default();
        let mut words = code.split_whitespace().peekable();
        // The block number.
        if words.peek().is_some_and(|w| w.chars().all(|c| c.is_ascii_digit())) {
            words.next();
        }
        let words: Vec<&str> = words.collect();
        let Some(first) = words.first().copied() else { continue };
        let rest = words.get(1..).unwrap_or_default();
        match first {
            "BEGIN" | "END" => {
                if rest.contains(&"INCH") {
                    units = 25.4;
                }
            }
            "CYCL" if rest.get(1) == Some(&"9.1") => {
                let seconds = rest.iter().rev().find_map(|w| w.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
                moves.push((n, Move::Dwell { seconds }));
            }
            "CC" => {
                let cx = rest.iter().find_map(|w| value(w, "X")).map(|v| v * units).or(x);
                let cy = rest.iter().find_map(|w| value(w, "Y")).map(|v| v * units).or(y);
                let (Some(cx), Some(cy)) = (cx, cy) else { return Err(CamError::Parse(n, "a circle centre before the position is known".into())) };
                center = Some(Vec2::new(cx, cy));
            }
            "L" | "C" => {
                if rest.iter().any(|w| w.starts_with("IX") || w.starts_with("IY") || w.starts_with("IZ")) {
                    return Err(CamError::Parse(n, "not supported yet: incremental coordinates".into()));
                }
                let rapid = rest.contains(&"FMAX");
                if let Some(f) = rest.iter().find_map(|w| value(w, "F")) {
                    feed = Some(f * units);
                }
                let from = match (x, y, z) {
                    (Some(a), Some(b), Some(c)) => Some(Vec3::new(a, b, c)),
                    _ => None,
                };
                let tx = rest.iter().find_map(|w| value(w, "X")).map(|v| v * units);
                let ty = rest.iter().find_map(|w| value(w, "Y")).map(|v| v * units);
                let tz = rest.iter().find_map(|w| value(w, "Z")).map(|v| v * units);
                if tx.is_none() && ty.is_none() && tz.is_none() {
                    continue;
                }
                (x, y, z) = (tx.or(x), ty.or(y), tz.or(z));
                let (Some(px), Some(py), Some(pz)) = (x, y, z) else { continue };
                let to = Vec3::new(px, py, pz);
                if first == "C" {
                    let (Some(c), Some(_)) = (center, from) else { return Err(CamError::Parse(n, "an arc before its centre CC or the position".into())) };
                    let ccw = if rest.contains(&"DR+") {
                        true
                    } else if rest.contains(&"DR-") {
                        false
                    } else {
                        return Err(CamError::Parse(n, "an arc without its direction DR+ or DR-".into()));
                    };
                    let Some(feed) = feed.filter(|f| *f > 0.0) else { return Err(CamError::Parse(n, "an arc without a feed rate".into())) };
                    moves.push((n, Move::Arc { to, center: c, ccw, feed }));
                } else if rapid {
                    moves.push((n, Move::Rapid { to }));
                } else {
                    let Some(feed) = feed.filter(|f| *f > 0.0) else { return Err(CamError::Parse(n, "a cutting move without a feed rate".into())) };
                    moves.push((n, Move::Feed { to, feed }));
                }
            }
            _ => {}
        }
    }
    Ok(moves)
}
