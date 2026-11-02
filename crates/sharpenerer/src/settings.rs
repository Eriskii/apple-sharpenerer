//! The settings file (`~/.config/sharpenerer/config.toml`) and turning it into the packed config
//! the payload reads.

use std::path::{Path, PathBuf};

use geometry::{Anchor, Config, Corners, Curve};
use toml_edit::{Array, DocumentMut, Item, Value, value};

pub const DEFAULT: &str = r#"# apple-sharpenerer settings. `sharpenerer set <key> <value>` changes them and applies them live;
# after editing this file by hand, run `sharpenerer reload`.

enabled = true

[corners]
# Radius of each corner in points (0-255) when it isn't near a rounded display corner.
top_left = 0
top_right = 0
bottom_left = 0
bottom_right = 0
# "continuous" (Apple's squircle) or "circular"
curve = "continuous"

[screen_aware]
# Round a window corner when it sits near a rounded corner of the display, following its curve.
enabled = true
# "usable" measures from the usable area (below the menu bar, beside the Dock),
# "display" from the physical panel edges.
anchor = "usable"
# "auto" reads the display's own rounding; a number (points) overrides it.
display_radius = "auto"

[sheen]
# true keeps it, false removes it.
top_highlight = false
inner_rim = false
outer_rim = true

[apps]
# Bundle identifiers to leave alone, e.g. ["com.apple.Terminal"].
exclude = []
"#;

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Radius,
    Curve,
    Anchor,
    DisplayRadius,
}

const KEYS: &[(&str, Kind)] = &[
    ("enabled", Kind::Bool),
    ("corners.top_left", Kind::Radius),
    ("corners.top_right", Kind::Radius),
    ("corners.bottom_left", Kind::Radius),
    ("corners.bottom_right", Kind::Radius),
    ("corners.curve", Kind::Curve),
    ("screen_aware.enabled", Kind::Bool),
    ("screen_aware.anchor", Kind::Anchor),
    ("screen_aware.display_radius", Kind::DisplayRadius),
    ("sheen.top_highlight", Kind::Bool),
    ("sheen.inner_rim", Kind::Bool),
    ("sheen.outer_rim", Kind::Bool),
];

pub fn key_names() -> impl Iterator<Item = &'static str> {
    KEYS.iter().map(|(k, _)| *k).chain(["corners"])
}

pub struct Settings {
    doc: DocumentMut,
}

pub fn path_in(home: &Path) -> PathBuf {
    home.join(".config/sharpenerer/config.toml")
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(format!("expected on or off, got {s}")),
    }
}

fn parse_radius(s: &str) -> Result<i64, String> {
    match s.parse::<i64>() {
        Ok(r) if (0..=255).contains(&r) => Ok(r),
        _ => Err(format!("expected a radius from 0 to 255, got {s}")),
    }
}

impl Settings {
    pub fn defaults() -> Self {
        Self { doc: DEFAULT.parse().expect("default settings parse") }
    }

    /// The file at `path`, or the defaults if there isn't one.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => text.parse().map(|doc| Self { doc }).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::defaults()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, self.doc.to_string()).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn item(&self, key: &str) -> Option<&Item> {
        key.split('.').try_fold(self.doc.as_item(), |item, part| item.get(part))
    }

    fn item_mut(&mut self, key: &str) -> &mut Item {
        key.split('.').fold(self.doc.as_item_mut(), |item, part| &mut item[part])
    }

    fn bool(&self, key: &str, default: bool) -> bool {
        self.item(key).and_then(Item::as_bool).unwrap_or(default)
    }

    fn radius(&self, key: &str) -> f64 {
        self.item(key).and_then(Item::as_integer).unwrap_or(0).clamp(0, 255) as f64
    }

    fn str(&self, key: &str) -> Option<&str> {
        self.item(key).and_then(Item::as_str)
    }

    pub fn config(&self) -> Config {
        let defaults = Config::default();
        let curve = match self.str("corners.curve") {
            Some("circular") => Curve::Circular,
            _ => Curve::Continuous,
        };
        let anchor = match self.str("screen_aware.anchor") {
            Some("display") => Anchor::Display,
            _ => Anchor::Usable,
        };
        Config {
            enabled: self.bool("enabled", defaults.enabled),
            shape: geometry::Settings {
                base: Corners {
                    top_left: self.radius("corners.top_left"),
                    top_right: self.radius("corners.top_right"),
                    bottom_left: self.radius("corners.bottom_left"),
                    bottom_right: self.radius("corners.bottom_right"),
                },
                curve,
                screen_aware: self.bool("screen_aware.enabled", true).then_some(anchor),
            },
            display_radius: self.radius("screen_aware.display_radius"),
            top_highlight: self.bool("sheen.top_highlight", defaults.top_highlight),
            inner_rim: self.bool("sheen.inner_rim", defaults.inner_rim),
            outer_rim: self.bool("sheen.outer_rim", defaults.outer_rim),
        }
    }

    pub fn excluded(&self) -> Vec<String> {
        self.item("apps.exclude")
            .and_then(Item::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    pub fn set_excluded(&mut self, bundle_id: &str, exclude: bool) {
        let mut list = self.excluded();
        list.retain(|id| id != bundle_id);
        if exclude {
            list.push(bundle_id.to_string());
        }
        *self.item_mut("apps.exclude") = value(list.into_iter().collect::<Array>());
    }

    /// Sets `key` (dots or dashes, e.g. `sheen.top-highlight`). `corners` takes one radius for
    /// all four or four radii (top-left, top-right, bottom-left, bottom-right).
    pub fn set(&mut self, key: &str, values: &[String]) -> Result<(), String> {
        let key = key.replace('-', "_");
        if key == "corners" {
            let radii = match values {
                [one] => vec![parse_radius(one)?; 4],
                [_, _, _, _] => values.iter().map(|v| parse_radius(v)).collect::<Result<_, _>>()?,
                _ => return Err("corners takes one radius or four".into()),
            };
            for (corner, r) in ["top_left", "top_right", "bottom_left", "bottom_right"].iter().zip(radii) {
                *self.item_mut(&format!("corners.{corner}")) = value(r);
            }
            return Ok(());
        }
        let Some(&(full, kind)) = KEYS.iter().find(|(k, _)| *k == key || k.rsplit('.').next() == Some(key.as_str())) else {
            return Err(format!("unknown setting {key}; try one of: {}", key_names().collect::<Vec<_>>().join(", ")));
        };
        let [v] = values else {
            return Err(format!("{full} takes one value"));
        };
        let new: Value = match kind {
            Kind::Bool => parse_bool(v)?.into(),
            Kind::Radius => parse_radius(v)?.into(),
            Kind::Curve if v == "continuous" || v == "circular" => v.as_str().into(),
            Kind::Anchor if v == "usable" || v == "display" => v.as_str().into(),
            Kind::DisplayRadius if v == "auto" => "auto".into(),
            Kind::DisplayRadius => parse_radius(v)?.into(),
            Kind::Curve => return Err("curve is continuous or circular".into()),
            Kind::Anchor => return Err("anchor is usable or display".into()),
        };
        *self.item_mut(full) = value(new);
        Ok(())
    }
}

impl std::fmt::Display for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_payload_defaults() {
        assert_eq!(Settings::defaults().config(), Config::default());
    }

    #[test]
    fn set_keeps_comments_and_applies() {
        let mut s = Settings::defaults();
        s.set("top-highlight", &["on".into()]).unwrap();
        s.set("corners", &["12".into(), "0".into(), "0".into(), "4".into()]).unwrap();
        s.set("screen_aware.anchor", &["display".into()]).unwrap();
        s.set("display-radius", &["30".into()]).unwrap();
        let c = s.config();
        assert!(c.top_highlight);
        assert_eq!(c.shape.base, Corners { top_left: 12.0, top_right: 0.0, bottom_left: 0.0, bottom_right: 4.0 });
        assert_eq!(c.shape.screen_aware, Some(Anchor::Display));
        assert_eq!(c.display_radius, 30.0);
        assert!(s.to_string().contains("# true keeps it, false removes it."));
    }

    #[test]
    fn rejects_bad_values() {
        let mut s = Settings::defaults();
        assert!(s.set("corners", &["300".into()]).is_err());
        assert!(s.set("curve", &["wobbly".into()]).is_err());
        assert!(s.set("nonsense", &["1".into()]).is_err());
    }

    #[test]
    fn exclude_list_round_trips() {
        let mut s = Settings::defaults();
        s.set_excluded("com.apple.Terminal", true);
        s.set_excluded("com.example.App", true);
        s.set_excluded("com.apple.Terminal", false);
        assert_eq!(s.excluded(), ["com.example.App"]);
    }
}
