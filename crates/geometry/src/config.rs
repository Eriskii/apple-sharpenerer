//! Everything the payload acts on, packed into 64 bits: small enough to patch straight into a
//! freshly loaded payload and to carry as a Darwin notification's state for live updates, so
//! sandboxed apps never have to read a file.

use crate::{Anchor, Corners, Curve, Settings};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub shape: Settings,
    /// Radius for the rounded corners of a display instead of the measured one; 0 keeps the
    /// measurement.
    pub display_radius: f64,
    /// Keep the bright line along the top edge of windows.
    pub top_highlight: bool,
    /// Keep the faint white line just inside the window edge.
    pub inner_rim: bool,
    /// Keep the dark outline around the window.
    pub outer_rim: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: Settings { base: Corners::all(0.0), curve: Curve::Continuous, screen_aware: Some(Anchor::Usable) },
            display_radius: 0.0,
            top_highlight: false,
            inner_rim: false,
            outer_rim: true,
        }
    }
}

/// Marks a packed value as ours, so an unset notification state (0) never parses.
const TAG: u64 = 0xA5;

// Layout: bits 0-31 base radii (top-left, top-right, bottom-left, bottom-right; one byte each),
// 32-39 display radius, 40-46 flags, 56-63 tag. Radii are whole points, 0-255.
impl Config {
    pub fn pack(&self) -> u64 {
        let byte = |r: f64| (r.max(0.0).min(255.0) + 0.5) as u64;
        let b = self.shape.base;
        let flags = [
            self.enabled,
            self.shape.screen_aware.is_some(),
            self.shape.screen_aware == Some(Anchor::Display),
            self.shape.curve == Curve::Circular,
            self.top_highlight,
            self.inner_rim,
            self.outer_rim,
        ]
        .iter()
        .enumerate()
        .fold(0u64, |acc, (bit, &on)| acc | (on as u64) << bit);
        byte(b.top_left)
            | byte(b.top_right) << 8
            | byte(b.bottom_left) << 16
            | byte(b.bottom_right) << 24
            | byte(self.display_radius) << 32
            | flags << 40
            | TAG << 56
    }

    pub fn unpack(v: u64) -> Option<Self> {
        if v >> 56 != TAG {
            return None;
        }
        let byte = |shift: u32| ((v >> shift) & 0xFF) as f64;
        let flag = |bit: u32| (v >> (40 + bit)) & 1 == 1;
        Some(Self {
            enabled: flag(0),
            shape: Settings {
                base: Corners { top_left: byte(0), top_right: byte(8), bottom_left: byte(16), bottom_right: byte(24) },
                curve: if flag(3) { Curve::Circular } else { Curve::Continuous },
                screen_aware: flag(1).then(|| if flag(2) { Anchor::Display } else { Anchor::Usable }),
            },
            display_radius: byte(32),
            top_highlight: flag(4),
            inner_rim: flag(5),
            outer_rim: flag(6),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let custom = Config {
            enabled: false,
            shape: Settings {
                base: Corners { top_left: 12.0, top_right: 0.0, bottom_left: 255.0, bottom_right: 3.0 },
                curve: Curve::Circular,
                screen_aware: Some(Anchor::Display),
            },
            display_radius: 40.0,
            top_highlight: true,
            inner_rim: false,
            outer_rim: true,
        };
        for config in [Config::default(), custom] {
            assert_eq!(Config::unpack(config.pack()), Some(config));
        }
    }

    #[test]
    fn unset_state_is_not_a_config() {
        assert_eq!(Config::unpack(0), None);
    }
}
