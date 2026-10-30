//! Holding tabs (bridges): strips of material left uncut on a through contour so that the part
//! stays attached to the stock. On every depth pass that reaches below a tab's top, the tool
//! lifts over each tab, crosses it at the tab's top height and comes back down to depth.

use serde::{Deserialize, Serialize};

use crate::ops::MAX_COORD;
use crate::{CamError, Result, invalid};

/// Most tabs on one loop.
const MAX_TABS: u32 = 1_000;

/// Where the tabs are and how big they are. `count` 0 and `spacing` 0: no tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tabs {
    /// Tabs round each loop, evenly spaced, none on the loop's start. 0: `spacing` decides.
    pub count: u32,
    /// Distance between tab centres along the loop when `count` is 0 (mm, > 0).
    pub spacing: f64,
    /// Length of each tab along the loop (mm, > 0), at most the distance between tabs.
    pub width: f64,
    /// How much of the wall's height each tab keeps, up from the cut's bottom (mm, > 0).
    pub height: f64,
}

impl Tabs {
    pub fn is_empty(&self) -> bool {
        self.count == 0 && self.spacing <= 0.0
    }

    /// The values that must hold whatever the loop: sizes positive and bounded.
    pub fn validate(&self) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.count > MAX_TABS {
            return Err(CamError::TooLarge(format!("more than {MAX_TABS} tabs on one contour")));
        }
        if !self.width.is_finite() || !(self.width > 0.0) || self.width > MAX_COORD {
            return invalid("the tab width must be above 0");
        }
        if !self.height.is_finite() || !(self.height > 0.0) || self.height > MAX_COORD {
            return invalid("the tab height must be above 0");
        }
        if self.count == 0 && (!self.spacing.is_finite() || !(self.spacing > 0.0) || self.spacing > MAX_COORD) {
            return invalid("the tab spacing must be above 0 when no tab count is given");
        }
        Ok(())
    }

    /// The tab centres along a loop of `length` mm, and the distance between them.
    pub(crate) fn centres(&self, length: f64) -> Result<(Vec<f64>, f64)> {
        if self.is_empty() {
            return Ok((Vec::new(), 0.0));
        }
        self.validate()?;
        if !length.is_finite() || !(length > 0.0) {
            return invalid("the contour has no length to place tabs on");
        }
        if self.count > 0 {
            let n = self.count as usize;
            let step = length / n as f64;
            Ok(((0..n).map(|k| step * (k as f64 + 0.5)).collect(), step))
        } else {
            let step = self.spacing;
            Ok(((0..((length / step) as usize)).map(|k| step * (k as f64 + 0.5)).collect(), step))
        }
    }

    /// Refuses tabs the loop cannot carry: wider than the gaps between them, or so tall that
    /// they reach the stock's top.
    pub(crate) fn check_fit(&self, step: f64, depth: f64) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.width > step + 1e-9 {
            return invalid("the tab width must be at most the distance between the tabs");
        }
        if !(depth > 0.0) || !(self.height < depth - 1e-9) {
            return invalid("the tab height must be less than the depth of the cut");
        }
        Ok(())
    }
}
