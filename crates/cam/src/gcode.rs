//! Programs as G-code: [`emit`] writes one through a [`Post`], [`parse`] reads the motion
//! back (the subset `emit` writes), which is how the output is checked.

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::path::{DrillCycle, Move, Toolpath, arc_sweep};
use crate::post::{CommentStyle, Post, Tapping, Values, fill};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Most operations in one program.
const MAX_OPERATIONS: usize = 10_000;
/// Most lines [`parse`] reads.
const MAX_LINES: usize = 5_000_000;
/// Longest comment written.
const MAX_COMMENT: usize = 80;

/// One operation of a program: a tool, how it turns, and where it goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    pub tool: Tool,
    /// Spindle speed (rev/min), clockwise.
    pub spindle_rpm: f64,
    pub coolant: bool,
    pub path: Toolpath,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub name: String,
    pub operations: Vec<Operation>,
    /// Work offset 1 to 6 (`G54` to `G59`); left out, the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_offset: Option<u8>,
}

/// The code of work offset `n` (1 to 6): `G54` to `G59`.
pub fn work_offset_code(n: u8) -> Result<String> {
    if !(1..=6).contains(&n) {
        return invalid("the work offset must be 1 to 6 (G54 to G59)");
    }
    Ok(format!("G{}", 53 + u32::from(n)))
}

/// `v` with at most `decimals` decimals and no trailing zeros.
fn number(v: f64, decimals: u8) -> String {
    let mut s = format!("{:.*}", usize::from(decimals), v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".into() } else { s }
}

/// Text that is safe inside any comment style. German letters are spelled out in ASCII
/// (`Außenkontur` → `Aussenkontur`) rather than dropped; other characters are.
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
    spelled.take(MAX_COMMENT).collect::<String>().trim().to_string()
}

struct Writer<'a> {
    post: &'a Post,
    out: String,
    line: Option<u32>,
    /// The position as written (what the controller believes) and exactly.
    x: Option<String>,
    y: Option<String>,
    z: Option<String>,
    pos: Option<Vec3>,
    feed: Option<String>,
    /// Words written in front of the next motion block: cutter compensation on or off.
    prefix: Option<String>,
}

impl Writer<'_> {
    fn block(&mut self, text: &str) {
        if let (Some(n), Some(numbering)) = (self.line, self.post.line_numbers) {
            self.out.push_str(&format!("N{n} "));
            self.line = Some(n.saturating_add(numbering.step));
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn comment(&mut self, text: &str) {
        let text = plain(text);
        match self.post.comments {
            CommentStyle::Parentheses => self.out.push_str(&format!("({text})\n")),
            CommentStyle::Semicolon => self.out.push_str(&format!("; {text}\n")),
            CommentStyle::None => {}
        }
    }

    fn template(&mut self, lines: &[String], values: &Values) -> Result<()> {
        for line in lines {
            let text = fill(line, values)?;
            self.block(&text);
        }
        Ok(())
    }

    /// A number as the controller wants it: with a decimal point even when whole, if the post says so.
    fn value(&self, v: f64, decimals: u8) -> String {
        let s = number(v, decimals);
        if self.post.trailing_decimal && !s.contains('.') { format!("{s}.") } else { s }
    }

    fn coord(&self, mm: f64) -> String {
        self.value(mm / self.post.units.mm(), self.post.decimals)
    }

    /// Forget where the tool is: the next move states every axis.
    fn reset(&mut self) {
        (self.x, self.y, self.z, self.pos, self.feed) = (None, None, None, None, None);
    }

    /// A `G0`/`G1` block for the axes among `xy` and `z` that change; nothing if none does.
    /// The word of motion code `n` (0–3): `G1`, or `G01` for a post that pads its codes.
    fn motion(&self, n: u8) -> String {
        if self.post.two_digit_codes { format!("G{n:02}") } else { format!("G{n}") }
    }

    fn linear(&mut self, code: u8, to: Vec3, xy: bool, z: bool, feed: Option<f64>) {
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
            return;
        }
        let feed = self.feed_word(feed);
        let code = self.motion(code);
        let prefix = self.take_prefix();
        self.block(&format!("{prefix}{code}{words}{feed}"));
    }

    /// The words waiting for the next motion block, with a space after them.
    fn take_prefix(&mut self) -> String {
        self.prefix.take().map(|p| format!("{} ", p.trim())).unwrap_or_default()
    }

    /// ` F…` when the feed changes.
    fn feed_word(&mut self, feed: Option<f64>) -> String {
        let Some(feed) = feed else { return String::new() };
        let f = self.value(feed / self.post.units.mm(), self.post.feed_decimals);
        if self.feed.as_ref() == Some(&f) {
            return String::new();
        }
        self.feed = Some(f.clone());
        format!(" F{f}")
    }

    fn rapid(&mut self, to: Vec3) {
        // At program start or after a tool change the machine position is unknown:
        // establish clearance in Z before any XY positioning.
        if self.pos.is_none_or(|p| to.z > p.z) {
            self.linear(0, to, false, true, None);
            self.linear(0, to, true, false, None);
        } else {
            self.linear(0, to, true, false, None);
            self.linear(0, to, false, true, None);
        }
        self.pos = Some(to);
    }

    fn feed(&mut self, to: Vec3, feed: f64) {
        self.linear(1, to, true, true, Some(feed));
        self.pos = Some(to);
    }

    fn arc(&mut self, to: Vec3, center: Vec2, ccw: bool, feed: f64) -> Result<()> {
        let Some(from) = self.pos else { return Err(CamError::Unsafe("an arc before the first positioning move".into())) };
        let (nx, ny) = (self.coord(to.x), self.coord(to.y));
        // An arc whose end is written the same as its start would be read as a full circle.
        if self.x.as_ref() == Some(&nx) && self.y.as_ref() == Some(&ny) {
            self.feed(to, feed);
            return Ok(());
        }
        // A helix on a controller that only knows flat arcs goes out as lines too.
        let helix = self.z.as_ref() != Some(&self.coord(to.z));
        if !self.post.arcs || (helix && !self.post.helical_arcs) {
            let start = Vec2::new(from.x, from.y) - center;
            let (r, sweep) = (start.len(), arc_sweep(from, to, center, ccw));
            let tol = self.post.arc_tolerance;
            let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { std::f64::consts::FRAC_PI_4 };
            let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
            for i in 1..n {
                let t = i as f64 / n as f64;
                let p = center + Vec2::from_angle(start.angle() + sweep * t) * r;
                self.feed(p.extend(from.z + (to.z - from.z) * t), feed);
            }
            self.feed(to, feed);
            return Ok(());
        }
        let nz = self.coord(to.z);
        let mut words = format!("{} X{nx} Y{ny}", self.motion(if ccw { 3 } else { 2 }));
        if self.z.as_ref() != Some(&nz) {
            words.push_str(&format!(" Z{nz}"));
        }
        words.push_str(&format!(" I{} J{}", self.coord(center.x - from.x), self.coord(center.y - from.y)));
        words.push_str(&self.feed_word(Some(feed)));
        (self.x, self.y, self.z, self.pos) = (Some(nx), Some(ny), Some(nz), Some(to));
        self.block(&words);
        Ok(())
    }

    /// Tapped hole `c` from height `z0` as the post's rigid tapping block; a post that cannot
    /// tap refuses it, as plain moves would drive the tap in without reversing the spindle.
    fn tap_cycle(&mut self, c: &DrillCycle, pitch: f64, z0: f64) -> Result<()> {
        let name = plain(&self.post.name);
        if !(pitch > 0.0 && pitch.is_finite()) {
            return Err(CamError::Unsafe("a tapped hole needs a thread pitch above 0".into()));
        }
        let f = self.value(c.feed / self.post.units.mm(), self.post.feed_decimals);
        if number(c.feed / self.post.units.mm(), self.post.feed_decimals) == "0" {
            return Err(CamError::Unsafe("a tapping feed is not above 0 at the post's precision".into()));
        }
        let rpm = number(c.feed / pitch, 0);
        let (nx, ny, bottom, retract, start) = (self.coord(c.at.x), self.coord(c.at.y), self.coord(c.bottom), self.coord(c.retract), self.coord(z0));
        let k = self.value(pitch / self.post.units.mm(), self.post.decimals.max(3));
        let rapid = self.motion(0);
        match self.post.tapping {
            Tapping::None => return invalid(format!("post `{name}` has no rigid tapping cycle, so it cannot write a tapped hole")),
            Tapping::G84 | Tapping::G84Rigid => {
                if self.post.tapping == Tapping::G84Rigid {
                    self.block(&format!("M29 S{rpm}"));
                }
                self.block(&format!("G98 G84 X{nx} Y{ny} Z{bottom} R{retract} F{f}"));
                self.block("G80");
            }
            Tapping::G33_1 => {
                self.block(&format!("{rapid} X{nx} Y{ny}"));
                self.block(&format!("{rapid} Z{retract}"));
                self.block(&format!("G33.1 Z{bottom} K{k}"));
                self.block(&format!("{rapid} Z{start}"));
            }
            Tapping::G331 => {
                self.block(&format!("{rapid} X{nx} Y{ny}"));
                self.block(&format!("{rapid} Z{retract}"));
                self.block("SPOS=0");
                self.block(&format!("G331 Z{bottom} K{k} S{rpm}"));
                self.block(&format!("G332 Z{retract} K{k}"));
                self.block(&format!("{rapid} Z{start}"));
            }
        }
        (self.x, self.y, self.z, self.feed) = (Some(nx), Some(ny), Some(start), Some(f));
        self.pos = Some(c.at.extend(z0));
        Ok(())
    }

    /// Hole `c` as one canned cycle from height `z0`, which `G98` returns to.
    fn cycle(&mut self, c: &DrillCycle, z0: f64) -> Result<()> {
        let f = self.value(c.feed / self.post.units.mm(), self.post.feed_decimals);
        if number(c.feed / self.post.units.mm(), self.post.feed_decimals) == "0" {
            return Err(CamError::Unsafe("a drilling feed is not above 0 at the post's precision".into()));
        }
        let (nx, ny) = (self.coord(c.at.x), self.coord(c.at.y));
        let mut words = format!(
            "G98 G8{} X{nx} Y{ny} Z{} R{}",
            match (c.peck, c.dwell > 0.0) {
                (Some(_), _) => 3,
                (None, true) => 2,
                (None, false) => 1,
            },
            self.coord(c.bottom),
            self.coord(c.retract)
        );
        if let Some(q) = c.peck {
            words.push_str(&format!(" Q{}", self.coord(q)));
        }
        if c.dwell > 0.0 {
            // In the unit the post's own dwell uses.
            let p = if self.post.dwell.contains("{milliseconds}") { number(c.dwell * 1000.0, 0) } else { self.value(c.dwell, 3) };
            words.push_str(&format!(" P{p}"));
        }
        words.push_str(&format!(" F{f}"));
        self.block(&words);
        self.block("G80");
        (self.x, self.y, self.z, self.feed) = (Some(nx), Some(ny), Some(self.coord(z0)), Some(f));
        self.pos = Some(c.at.extend(z0));
        Ok(())
    }
}

/// The height a drill cycle starts and ends at, when `moves[c.start..c.end]` drill exactly the
/// hole `c` describes from `from` (where the tool is before them); `None` when they do not, so the
/// moves are written as they are.
fn cycle_start(c: &DrillCycle, moves: &[Move], from: Option<Vec3>) -> Option<f64> {
    let from = from?;
    let run = moves.get(c.start..c.end).filter(|run| !run.is_empty())?;
    let fine = |v: f64| v.is_finite() && v.abs() <= crate::ops::MAX_COORD;
    let peck_ok = c.peck.is_none_or(|q| q > 0.0 && q.is_finite());
    if !(fine(c.at.x) && fine(c.at.y) && fine(c.retract) && fine(c.bottom) && c.retract > c.bottom && peck_ok) {
        return None;
    }
    if !(c.feed > 0.0 && c.feed.is_finite() && c.dwell >= 0.0 && c.dwell <= 3600.0) || (c.peck.is_some() && c.dwell > 0.0) {
        return None;
    }
    let at = |p: Vec3| Vec2::new(p.x, p.y).dist(c.at) <= 1e-9;
    let z0 = from.z;
    if !at(from) || z0 < c.retract {
        return None;
    }
    let mut lowest = z0;
    for m in run {
        match *m {
            Move::Rapid { to } | Move::Feed { to, .. } if at(to) && to.z <= z0 + 1e-9 && to.z >= c.bottom - 1e-9 => lowest = lowest.min(to.z),
            Move::Dwell { .. } => {}
            _ => return None,
        }
    }
    let last = run.last().and_then(Move::target)?;
    ((lowest - c.bottom).abs() <= 1e-9 && (last.z - z0).abs() <= 1e-9).then_some(z0)
}

/// Writes `program` for the controller `post` describes.
pub fn emit(program: &Program, post: &Post) -> Result<String> {
    post.validate()?;
    if post.language == crate::post::Language::Klartext {
        return crate::klartext::emit(program, post);
    }
    if program.operations.is_empty() {
        return invalid("the program has no operation");
    }
    if program.operations.len() > MAX_OPERATIONS {
        return Err(CamError::TooLarge(format!("more than {MAX_OPERATIONS} operations")));
    }
    let mut w =
        Writer { post, out: String::new(), line: post.line_numbers.map(|n| n.start), x: None, y: None, z: None, pos: None, feed: None, prefix: None };
    let offset = program.work_offset.unwrap_or(1);
    // A post that leaves the offset to the operator can only mean the first one.
    if offset != 1 && !post.writes_work_offset() {
        return invalid(format!(
            "post `{}` writes no work offset, so the program would run in whatever offset the machine is in rather than {}",
            plain(&post.name),
            work_offset_code(offset)?
        ));
    }
    let mut values =
        Values { program: plain(&program.name), units: post.units.code().into(), work_offset: work_offset_code(offset)?, ..Values::default() };
    w.comment(&program.name);
    w.template(&post.header, &values)?;
    let mut tool: Option<u32> = None;
    let mut rpm: Option<String> = None;
    let mut coolant = false;
    for op in &program.operations {
        op.tool.validate()?;
        if !(op.spindle_rpm > 0.0) || !(op.spindle_rpm <= 1.0e6) {
            return invalid(format!("operation `{}`: the spindle speed must be above 0", plain(&op.name)));
        }
        let speed = number(op.spindle_rpm, 0);
        if speed == "0" {
            return invalid("the spindle speed rounds to zero in the posted program");
        }
        values.tool = op.tool.number.to_string();
        values.diameter = number(op.tool.diameter / post.units.mm(), post.decimals);
        values.rpm = speed.clone();
        w.comment(&format!("{}: T{} D{} {}", op.name, op.tool.number, number(op.tool.diameter, 3), op.tool.kind.label()));
        if tool != Some(op.tool.number) {
            if tool.is_some() || post.tool_change_first {
                if coolant {
                    w.template(&post.coolant_off, &values)?;
                    coolant = false;
                }
                w.template(&post.tool_change, &values)?;
            }
            // A tool change stops the spindle.
            rpm = None;
            tool = Some(op.tool.number);
        }
        // A torch is lit at each pierce and put out before each rapid, below; a spindle runs throughout.
        let torch = op.tool.kind == crate::ToolKind::Plasma;
        if torch && rpm.is_some() {
            // A spindle left running by the step before stops before the torch is lit.
            w.template(&post.spindle_off, &values)?;
            rpm = None;
        } else if !torch && rpm.as_ref() != Some(&speed) {
            w.template(&post.spindle_on, &values)?;
            rpm = Some(speed);
        }
        if op.coolant != coolant {
            w.template(if op.coolant { &post.coolant_on } else { &post.coolant_off }, &values)?;
            coolant = op.coolant;
        }
        w.reset();
        check_comp(op, post)?;
        let mut runs = op.path.comp.iter().peekable();
        // Tapped holes are always written as their cycle; drilled ones when the post has canned cycles.
        let mut cycles = op.path.cycles.iter().filter(|c| post.canned_cycles || c.tap.is_some()).peekable();
        let mut skip_to = 0usize;
        let mut lit = false;
        for (index, m) in op.path.moves.iter().enumerate() {
            if index < skip_to {
                continue;
            }
            if torch {
                let rapid = matches!(m, Move::Rapid { .. });
                if !rapid && !lit {
                    w.template(&post.spindle_on, &values)?;
                    lit = true;
                } else if rapid && lit {
                    w.template(&post.spindle_off, &values)?;
                    lit = false;
                }
            }
            // A compensated pass: switched on with its entry line, off with its exit line.
            if let Some(run) = runs.next_if(|r| r.start == index) {
                values.register = run.register.to_string();
                let on = fill(
                    match run.side {
                        crate::CompSide::Left => &post.comp_left,
                        crate::CompSide::Right => &post.comp_right,
                    },
                    &values,
                )?;
                let off = fill(&post.comp_off, &values)?;
                let last = run.programmed.len().saturating_sub(1);
                for (i, m) in run.programmed.iter().enumerate() {
                    if i == 0 {
                        w.prefix = Some(on.clone());
                    } else if i == last {
                        w.prefix = Some(off.clone());
                    }
                    write_move(&mut w, &mut values, &op.name, m)?;
                    if w.prefix.is_some() {
                        return Err(CamError::Unsafe(format!(
                            "operation `{}`: the entry or exit line of a compensated contour is too short to write",
                            plain(&op.name)
                        )));
                    }
                }
                skip_to = run.end;
                continue;
            }
            while cycles.next_if(|c| c.start < index).is_some() {}
            if let Some(c) = cycles.next_if(|c| c.start == index) {
                match (cycle_start(c, &op.path.moves, w.pos), c.tap) {
                    (Some(z0), Some(pitch)) => w.tap_cycle(c, pitch, z0)?,
                    (Some(z0), None) => w.cycle(c, z0)?,
                    (None, Some(_)) => {
                        return Err(CamError::Unsafe(format!(
                            "operation `{}`: a tapped hole whose moves do not match its cycle cannot be written",
                            plain(&op.name)
                        )));
                    }
                    (None, None) => {
                        write_move(&mut w, &mut values, &op.name, m)?;
                        continue;
                    }
                }
                skip_to = c.end;
                continue;
            }
            if op.path.cycles.iter().any(|c| c.tap.is_some() && c.start <= index && index < c.end) {
                return Err(CamError::Unsafe(format!("operation `{}`: a tapped hole cannot be written move by move", plain(&op.name))));
            }
            write_move(&mut w, &mut values, &op.name, m)?;
        }
        if lit {
            w.template(&post.spindle_off, &values)?;
        }
    }
    if coolant {
        w.template(&post.coolant_off, &values)?;
    }
    w.template(&post.spindle_off, &values)?;
    w.template(&post.footer, &values)?;
    Ok(w.out)
}

/// Whether the compensated passes of `op` can be written by `post`: the post knows the words,
/// and each pass is cutting moves only, entered and left on a straight line, in order and
/// inside the toolpath, with a register of 1 to 9999.
pub(crate) fn check_comp(op: &Operation, post: &Post) -> Result<()> {
    if op.path.comp.is_empty() {
        return Ok(());
    }
    if !post.compensates() {
        return invalid(format!(
            "post `{}` writes no cutter compensation (G41/G42): compensate step `{}` in the CAM, or choose a post for a control that has it",
            plain(&post.name),
            plain(&op.name)
        ));
    }
    if op.tool.kind == crate::ToolKind::Plasma {
        return invalid(format!("operation `{}`: a plasma cut is offset by its kerf in the CAM, not on the control", plain(&op.name)));
    }
    let mut end = 0usize;
    for run in &op.path.comp {
        let straight = |m: Option<&Move>| matches!(m, Some(Move::Feed { .. }));
        if run.start < end
            || run.start >= run.end
            || run.end > op.path.moves.len()
            || run.programmed.len() < 3
            || !straight(run.programmed.first())
            || !straight(run.programmed.last())
            || run.programmed.iter().any(|m| matches!(m, Move::Rapid { .. } | Move::Dwell { .. }))
            || !(1..=9999).contains(&run.register)
            || op.path.cycles.iter().any(|c| c.start < run.end && run.start < c.end)
        {
            return Err(CamError::Unsafe(format!(
                "operation `{}`: a compensated contour is not written as one cut from its entry to its exit line",
                plain(&op.name)
            )));
        }
        end = run.end;
    }
    Ok(())
}

/// One move as the post writes it.
fn write_move(w: &mut Writer<'_>, values: &mut Values, op: &str, m: &Move) -> Result<()> {
    let post = w.post;
    {
        {
            if m.target().is_some_and(|to| !to.is_finite()) {
                return Err(CamError::Unsafe(format!("operation `{}` has a coordinate that is not a finite number", plain(op))));
            }
            match *m {
                Move::Rapid { to } => w.rapid(to),
                Move::Feed { to, feed } | Move::Arc { to, feed, .. }
                    if !(feed > 0.0) || !feed.is_finite() || number(feed / post.units.mm(), post.feed_decimals) == "0" =>
                {
                    let _ = to;
                    return Err(CamError::Unsafe(format!("operation `{}` has a feed that is not above 0 at the post's precision", plain(op))));
                }
                Move::Feed { to, feed } => w.feed(to, feed),
                Move::Arc { to, center, ccw, feed } => w.arc(to, center, ccw, feed)?,
                Move::Dwell { seconds } => {
                    if !(seconds >= 0.0) || !seconds.is_finite() {
                        return Err(CamError::Unsafe("a dwell must be 0 seconds or longer".into()));
                    }
                    values.seconds = w.value(seconds, 3);
                    values.milliseconds = number(seconds * 1000.0, 0);
                    let dwell = fill(&post.dwell, values)?;
                    w.block(&dwell);
                }
            }
        }
    }
    Ok(())
}

/// Whether `line` (after an optional `N` number) is a Sinumerik assignment such as `SPOS=0`.
fn assignment(line: &str) -> bool {
    let line = line.trim_start();
    let line = match line.strip_prefix(['N', 'n']) {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start(),
        _ => line,
    };
    let name: String = line.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    name.len() >= 2 && name.starts_with(|c: char| c.is_ascii_alphabetic()) && line[name.len()..].trim_start().starts_with('=')
}

/// The words of one line: `(letter, value)`.
fn words(line: &str, n: usize) -> Result<Vec<(char, f64)>> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            ';' | '%' => break,
            '(' => {
                for d in chars.by_ref() {
                    if d == ')' {
                        break;
                    }
                }
            }
            c if c.is_ascii_alphabetic() => {
                let mut text = String::new();
                while let Some(d) = chars.peek().copied().filter(|d| d.is_ascii_digit() || "+-.".contains(*d)) {
                    text.push(d);
                    chars.next();
                }
                let value: f64 = text.parse().map_err(|_| CamError::Parse(n, format!("`{c}{text}` is not a word with a number")))?;
                if !value.is_finite() {
                    return Err(CamError::Parse(n, format!("`{c}{text}` is not a finite number")));
                }
                out.push((c.to_ascii_uppercase(), value));
            }
            other => return Err(CamError::Parse(n, format!("unexpected `{other}`"))),
        }
    }
    Ok(out)
}

/// Reads the motion of a G-code program in absolute coordinates: `G0`–`G3` with `X Y Z I J F`,
/// `G4 P`, `G20`/`G21`. Everything else (spindle, tools, coolant, offsets) is skipped. The
/// result is in millimetres and mm/min.
pub fn parse(text: &str) -> Result<Vec<Move>> {
    Ok(parse_lines(text)?.into_iter().map(|(_, m)| m).collect())
}

/// [`parse`], each move with the number of the line it is written on (from 1).
pub fn parse_lines(text: &str) -> Result<Vec<(usize, Move)>> {
    let mut units = 1.0;
    let mut mode: Option<u8> = None;
    let mut feed: Option<f64> = None;
    // Canned cycles: back to the initial height (`G98`) or to R (`G99`), and the modal R and Q.
    let mut to_initial = true;
    let (mut r_plane, mut q_peck, mut cycle_z): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
    let (mut x, mut y, mut z): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
    let mut pos: Option<Vec3> = None;
    let mut moves = Vec::new();
    // The spindle speed, for tapping blocks that give the pitch (`K`) rather than the feed.
    let mut spindle: Option<f64> = None;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if n > MAX_LINES {
            return Err(CamError::TooLarge(format!("more than {MAX_LINES} lines")));
        }
        // A Sinumerik assignment (`SPOS=0`) moves no axis.
        if assignment(line) {
            continue;
        }
        let ws = words(line, n)?;
        let mut dwell = false;
        let mut tapping: Option<u16> = None;
        for &(c, v) in &ws {
            if c != 'G' {
                continue;
            }
            match (v * 10.0).round() as i64 {
                0 => mode = Some(0),
                10 => mode = Some(1),
                20 => mode = Some(2),
                30 => mode = Some(3),
                40 => dwell = true,
                331 | 3310 | 3320 => tapping = Some((v * 10.0).round() as u16),
                200 => units = 25.4,
                210 => units = 1.0,
                800 => mode = None,
                810 => mode = Some(81),
                820 => mode = Some(82),
                830 => mode = Some(83),
                840 => mode = Some(84),
                910 => return Err(CamError::Parse(n, "not supported yet: incremental coordinates (G91)".into())),
                980 => to_initial = true,
                990 => to_initial = false,
                _ => {}
            }
        }
        let word = |letter: char| ws.iter().find(|(c, _)| *c == letter).map(|(_, v)| *v);
        if let Some(s) = word('S').filter(|s| *s > 0.0) {
            spindle = Some(s);
        }
        if dwell {
            // `G4 P` in seconds as this crate's own posts write it, `G04 X` or Sinumerik's `G4 F`.
            moves.push((n, Move::Dwell { seconds: word('X').or(word('P')).or(word('F')).unwrap_or(0.0).max(0.0) }));
            continue;
        }
        if let Some(code) = tapping {
            // Rigid tapping: in to Z at one pitch a turn, and (G33.1) straight back out.
            let (Some(from), Some(bottom)) = (pos, word('Z').map(|v| v * units)) else {
                return Err(CamError::Parse(n, "a tapping block before the position is known, or without Z".into()));
            };
            let (Some(k), Some(rpm)) = (word('K').map(|v| v * units).filter(|k| *k > 0.0), spindle) else {
                return Err(CamError::Parse(n, "a tapping block without a pitch K above 0 or a spindle speed".into()));
            };
            let to = Vec3::new(from.x, from.y, bottom);
            moves.push((n, Move::Feed { to, feed: k * rpm }));
            if code == 331 {
                moves.push((n, Move::Feed { to: from, feed: k * rpm }));
            } else {
                pos = Some(to);
                z = Some(bottom);
            }
            continue;
        }
        if let Some(f) = word('F') {
            feed = Some(f * units);
        }
        let (tx, ty, tz, ti, tj) = (word('X'), word('Y'), word('Z'), word('I'), word('J'));
        if tx.is_none() && ty.is_none() && tz.is_none() && ti.is_none() && tj.is_none() {
            continue;
        }
        let Some(mode) = mode else { return Err(CamError::Parse(n, "coordinates before any G0, G1, G2 or G3".into())) };
        if mode >= 81 {
            r_plane = word('R').map(|v| v * units).or(r_plane);
            q_peck = word('Q').map(|v| v * units).or(q_peck);
            let (Some(px), Some(py), Some(z0)) = (tx.map(|v| v * units).or(x), ty.map(|v| v * units).or(y), pos.map(|p| p.z)) else {
                return Err(CamError::Parse(n, "a drilling cycle before the position is known".into()));
            };
            let (Some(bottom), Some(r)) = (tz.map(|v| v * units).or(cycle_z), r_plane) else {
                return Err(CamError::Parse(n, "a drilling cycle without Z or R".into()));
            };
            let Some(feed) = feed.filter(|f| *f > 0.0) else { return Err(CamError::Parse(n, "a drilling cycle without a feed rate".into())) };
            let peck = if mode == 83 {
                Some(q_peck.filter(|q| *q > 0.0).ok_or_else(|| CamError::Parse(n, "G83 without a peck depth Q above 0".into()))?)
            } else {
                None
            };
            let dwell = if mode == 82 { word('P').unwrap_or(0.0).max(0.0) } else { 0.0 };
            drill_cycle(&mut moves, n, Vec3::new(px, py, z0), r, bottom, peck, dwell, feed, to_initial, mode == 84)?;
            if moves.len() > crate::path::MAX_MOVES {
                return Err(CamError::TooLarge("the drilling cycles make too many moves".into()));
            }
            // `bottom` stays the cycle's modal Z; the tool itself ends at the return height.
            pos = moves.last().and_then(|(_, m)| m.target()).or(pos);
            (x, y, z) = (Some(px), Some(py), pos.map(|p| p.z));
            cycle_z = Some(bottom);
            continue;
        }
        x = tx.map(|v| v * units).or(x);
        y = ty.map(|v| v * units).or(y);
        z = tz.map(|v| v * units).or(z);
        let (Some(px), Some(py), Some(pz)) = (x, y, z) else { continue };
        let to = Vec3::new(px, py, pz);
        match mode {
            0 => moves.push((n, Move::Rapid { to })),
            _ => {
                let Some(feed) = feed.filter(|f| *f > 0.0) else { return Err(CamError::Parse(n, "a cutting move without a feed rate".into())) };
                if mode == 1 {
                    moves.push((n, Move::Feed { to, feed }));
                } else {
                    let Some(from) = pos else { return Err(CamError::Parse(n, "an arc before the position is known".into())) };
                    if ti.is_none() && tj.is_none() {
                        return Err(CamError::Parse(n, "not supported yet: arcs without I and J".into()));
                    }
                    let center = Vec2::new(from.x + ti.unwrap_or(0.0) * units, from.y + tj.unwrap_or(0.0) * units);
                    moves.push((n, Move::Arc { to, center, ccw: mode == 3, feed }));
                }
            }
        }
        pos = Some(to);
    }
    Ok(moves)
}

/// How far `G83` stops above the bottom of the previous peck before feeding on (mm), as the
/// RS-274/NGC interpreter does (0.010 in).
const PECK_CLEARANCE: f64 = 0.254;

/// The moves of one canned drilling cycle (`G81`, `G82`, `G83`) at `start` (its XY and the
/// initial height) down to `bottom`, as the RS-274/NGC description has them.
#[allow(clippy::too_many_arguments)]
fn drill_cycle(
    moves: &mut Vec<(usize, Move)>,
    n: usize,
    start: Vec3,
    r: f64,
    bottom: f64,
    peck: Option<f64>,
    dwell: f64,
    feed: f64,
    to_initial: bool,
    tapping: bool,
) -> Result<()> {
    if bottom > r {
        return Err(CamError::Parse(n, "a drilling cycle whose Z is above its R".into()));
    }
    let at = |z: f64| Vec3::new(start.x, start.y, z);
    // Below R the cycle first goes up to R and treats that as its initial height.
    let initial = start.z.max(r);
    moves.push((n, Move::Rapid { to: at(initial) }));
    moves.push((n, Move::Rapid { to: at(r) }));
    match peck {
        Some(q) => {
            if (r - bottom) / q > 1.0e5 {
                return Err(CamError::TooLarge(format!("line {n}: a G83 with too many pecks")));
            }
            let mut depth = r;
            loop {
                let next = (depth - q).max(bottom);
                moves.push((n, Move::Feed { to: at(next), feed }));
                moves.push((n, Move::Rapid { to: at(r) }));
                if next <= bottom {
                    break;
                }
                moves.push((n, Move::Rapid { to: at((next + PECK_CLEARANCE).min(r)) }));
                depth = next;
            }
        }
        // G84 feeds back out as it went in.
        None if tapping => {
            moves.push((n, Move::Feed { to: at(bottom), feed }));
            moves.push((n, Move::Feed { to: at(r), feed }));
        }
        None => {
            moves.push((n, Move::Feed { to: at(bottom), feed }));
            if dwell > 0.0 {
                moves.push((n, Move::Dwell { seconds: dwell }));
            }
            moves.push((n, Move::Rapid { to: at(r) }));
        }
    }
    if to_initial && initial > r {
        moves.push((n, Move::Rapid { to: at(initial) }));
    }
    Ok(())
}
