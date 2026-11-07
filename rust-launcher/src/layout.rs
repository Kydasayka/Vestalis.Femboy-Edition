//! Main-screen layout: named zones and the order of movable blocks.
//!
//! The layout is intentionally a list of zones rather than a free canvas: it
//! keeps the UI responsive at the 1080×620 minimum and avoids overlapping
//! panels. The server selector lives in the title bar and is pinned to the
//! header zone.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const ITEMS: [&str; 7] = [
    "server",
    "user",
    "profiles",
    "countdown",
    "sidebar-actions",
    "news",
    "main-actions",
];

pub const ZONES: [&str; 6] = [
    "sidebar-left",
    "sidebar-right",
    "header",
    "content-top",
    "content-bottom",
    "footer",
];

pub const WIDTH_MIN: u32 = 240;
pub const WIDTH_MAX: u32 = 420;
pub const WIDTH_DEFAULT_LEFT: u32 = 316;
pub const WIDTH_DEFAULT_RIGHT: u32 = 300;

/// Default block height, minimum and maximum in logical pixels.
pub const BLOCK_HEIGHTS: [(&str, u32, u32, u32); 6] = [
    ("user", 64, 56, 96),
    ("profiles", 280, 120, 520),
    ("countdown", 124, 96, 240),
    ("sidebar-actions", 44, 40, 72),
    ("news", 220, 120, 600),
    ("main-actions", 52, 44, 80),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct LayoutConfig {
    pub sidebar_left: Vec<String>,
    pub sidebar_right: Vec<String>,
    pub header: Vec<String>,
    pub content_top: Vec<String>,
    pub content_bottom: Vec<String>,
    pub footer: Vec<String>,
    /// Blocks hidden by the user. They stay in their zone so the layout can be
    /// restored without losing the order.
    pub hidden: Vec<String>,
    pub sidebar_left_width: u32,
    pub sidebar_right_width: u32,
    /// Per-block height overrides. Missing entries use the defaults below.
    pub block_heights: BTreeMap<String, u32>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            sidebar_left: vec![
                "user".into(),
                "profiles".into(),
                "countdown".into(),
                "sidebar-actions".into(),
            ],
            sidebar_right: Vec::new(),
            header: vec!["server".into()],
            content_top: vec!["news".into()],
            content_bottom: vec!["main-actions".into()],
            footer: Vec::new(),
            hidden: Vec::new(),
            sidebar_left_width: WIDTH_DEFAULT_LEFT,
            sidebar_right_width: WIDTH_DEFAULT_RIGHT,
            block_heights: BTreeMap::new(),
        }
    }
}

/// Named layout preset stored next to the active layout.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutProfile {
    pub name: String,
    #[serde(default)]
    pub layout: LayoutConfig,
}

impl LayoutConfig {
    pub fn zones(&self) -> [&Vec<String>; 6] {
        [
            &self.sidebar_left,
            &self.sidebar_right,
            &self.header,
            &self.content_top,
            &self.content_bottom,
            &self.footer,
        ]
    }

    fn zones_mut(&mut self) -> [&mut Vec<String>; 6] {
        [
            &mut self.sidebar_left,
            &mut self.sidebar_right,
            &mut self.header,
            &mut self.content_top,
            &mut self.content_bottom,
            &mut self.footer,
        ]
    }

    /// Drops duplicates/unknown items and appends blocks missing from the
    /// saved layout to their default zones. The server selector is pinned to
    /// the header because the title bar owns its dropdown.
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        for zone in self.zones_mut() {
            zone.retain(|item| {
                if !ITEMS.contains(&item.as_str()) || seen.contains(item) || item == "server" {
                    false
                } else {
                    seen.push(item.clone());
                    true
                }
            });
        }

        let defaults = LayoutConfig::default();
        for (name, default_items) in ZONES.iter().zip(defaults.zones()) {
            for item in default_items {
                if item == "server" || seen.contains(item) {
                    continue;
                }
                let target = match *name {
                    "sidebar-left" => &mut self.sidebar_left,
                    "sidebar-right" => &mut self.sidebar_right,
                    "content-top" => &mut self.content_top,
                    "content-bottom" => &mut self.content_bottom,
                    "footer" => &mut self.footer,
                    _ => &mut self.content_top,
                };
                target.push(item.clone());
                seen.push(item.clone());
            }
        }

        self.header.retain(|item| item != "server");
        self.header.insert(0, "server".into());

        let mut hidden = Vec::new();
        for item in self.hidden.drain(..) {
            if item != "server" && ITEMS.contains(&item.as_str()) && !hidden.contains(&item) {
                hidden.push(item);
            }
        }
        self.hidden = hidden;

        self.sidebar_left_width = self.sidebar_left_width.clamp(WIDTH_MIN, WIDTH_MAX);
        self.sidebar_right_width = self.sidebar_right_width.clamp(WIDTH_MIN, WIDTH_MAX);

        self.block_heights
            .retain(|item, _| ITEMS.contains(&item.as_str()) && item != "server");
        for (item, default, min, max) in BLOCK_HEIGHTS {
            let mut height = self
                .block_heights
                .get(item)
                .copied()
                .unwrap_or(default)
                .clamp(min, max);
            // 200 px was the old default and left the search field + three
            // profile rows clipped; migrate it to the new default.
            if item == "profiles" && height == 200 {
                height = default;
            }
            self.block_heights.insert(item.to_string(), height);
        }
    }

    pub fn block_height(&self, item: &str) -> u32 {
        BLOCK_HEIGHTS
            .iter()
            .find(|(name, _, _, _)| *name == item)
            .map(|(_, default, min, max)| {
                self.block_heights
                    .get(item)
                    .copied()
                    .unwrap_or(*default)
                    .clamp(*min, *max)
            })
            .unwrap_or(0)
    }

    pub fn set_block_height(&mut self, item: &str, height: u32) {
        let Some((_, _, min, max)) = BLOCK_HEIGHTS.iter().find(|(name, _, _, _)| *name == item)
        else {
            return;
        };
        self.block_heights
            .insert(item.to_string(), height.clamp(*min, *max));
    }

    /// Moves an item into `zone`, optionally before another item. Used by
    /// drag-and-drop; the arrow buttons keep working through `move_item`.
    pub fn move_to_zone(&mut self, item: &str, zone: &str, before: Option<&str>) {
        self.normalize();
        if item == "server" || !ITEMS.contains(&item) || !ZONES.contains(&zone) {
            return;
        }
        if before == Some(item) {
            return;
        }

        for list in self.zones_mut() {
            list.retain(|value| value != item);
        }

        let before = before.filter(|value| *value != item && *value != "server");
        let list = match zone {
            "sidebar-left" => &mut self.sidebar_left,
            "sidebar-right" => &mut self.sidebar_right,
            "header" => &mut self.header,
            "content-top" => &mut self.content_top,
            "content-bottom" => &mut self.content_bottom,
            _ => &mut self.footer,
        };

        let index = before
            .and_then(|value| list.iter().position(|current| current == value))
            .unwrap_or(list.len());
        let index = if zone == "header" {
            index.max(1)
        } else {
            index
        };
        list.insert(index.min(list.len()), item.to_string());
    }

    pub fn set_hidden(&mut self, item: &str, hidden: bool) {
        self.normalize();
        if item == "server" || !ITEMS.contains(&item) {
            return;
        }

        self.hidden.retain(|value| value != item);
        if hidden {
            self.hidden.push(item.to_string());
        }
    }

    pub fn is_hidden(&self, item: &str) -> bool {
        self.hidden.iter().any(|value| value == item)
    }

    pub fn set_widths(&mut self, left: u32, right: u32) {
        self.sidebar_left_width = left.clamp(WIDTH_MIN, WIDTH_MAX);
        self.sidebar_right_width = right.clamp(WIDTH_MIN, WIDTH_MAX);
    }

    /// Moves an item up/down inside its zone or to the previous/next zone.
    pub fn move_item(&mut self, item: &str, direction: &str) {
        self.normalize();
        if item == "server" {
            return;
        }

        let zone_index = self
            .zones()
            .iter()
            .position(|zone| zone.iter().any(|value| value == item));
        let Some(zone_index) = zone_index else {
            return;
        };

        let zones = self.zones_mut();
        let index = zones[zone_index]
            .iter()
            .position(|value| value == item)
            .expect("item is in the zone");

        match direction {
            "up" if index > 0 => {
                zones[zone_index].swap(index, index - 1);
            }
            "down" if index + 1 < zones[zone_index].len() => {
                zones[zone_index].swap(index, index + 1);
            }
            "left" if zone_index > 0 => {
                zones[zone_index].remove(index);
                let target = zone_index - 1;
                zones[target].push(item.to_string());
            }
            "right" if zone_index + 1 < zones.len() => {
                zones[zone_index].remove(index);
                let target = zone_index + 1;
                zones[target].push(item.to_string());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_contains_every_item_once() {
        let mut layout = LayoutConfig::default();
        layout.normalize();
        let mut all: Vec<&String> = layout.zones().iter().flat_map(|zone| zone.iter()).collect();
        all.sort();
        let mut expected: Vec<String> = ITEMS.iter().map(|item| item.to_string()).collect();
        expected.sort();
        assert_eq!(all, expected.iter().collect::<Vec<_>>());
    }

    #[test]
    fn moving_between_zones_keeps_items_unique() {
        let mut layout = LayoutConfig::default();
        layout.move_item("news", "left");
        assert!(layout.header.iter().any(|item| item == "news"));
        layout.move_item("news", "left");
        layout.normalize();

        let count = layout
            .zones()
            .iter()
            .flat_map(|zone| zone.iter())
            .filter(|item| item.as_str() == "news")
            .count();
        assert_eq!(count, 1);
        assert!(layout.sidebar_right.iter().any(|item| item == "news"));
    }

    #[test]
    fn server_stays_in_header() {
        let mut layout = LayoutConfig::default();
        layout.move_item("server", "left");
        layout.normalize();
        assert_eq!(layout.header.first().map(String::as_str), Some("server"));
    }

    #[test]
    fn drag_and_drop_moves_and_inserts_without_duplicates() {
        let mut layout = LayoutConfig::default();
        layout.move_to_zone("news", "sidebar-right", None);
        assert!(layout.sidebar_right.contains(&"news".to_string()));

        layout.move_to_zone("countdown", "sidebar-right", Some("news"));
        let right = &layout.sidebar_right;
        let countdown = right.iter().position(|item| item == "countdown").unwrap();
        let news = right.iter().position(|item| item == "news").unwrap();
        assert!(countdown < news);

        layout.move_to_zone("news", "sidebar-right", Some("news"));
        layout.normalize();
        let count = layout
            .zones()
            .iter()
            .flat_map(|zone| zone.iter())
            .filter(|item| item.as_str() == "news")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn hidden_blocks_stay_in_place_and_ignore_unknown_items() {
        let mut layout = LayoutConfig::default();
        layout.set_hidden("countdown", true);
        layout.set_hidden("bogus", true);
        layout.set_hidden("server", true);
        layout.normalize();

        assert!(layout.is_hidden("countdown"));
        assert!(!layout.is_hidden("bogus"));
        assert!(!layout.is_hidden("server"));
        assert!(layout.sidebar_left.contains(&"countdown".to_string()));

        layout.set_hidden("countdown", false);
        assert!(!layout.is_hidden("countdown"));
    }

    #[test]
    fn block_heights_are_clamped_and_defaulted() {
        let mut layout = LayoutConfig::default();
        layout.normalize();
        assert_eq!(layout.block_height("countdown"), 124);
        assert_eq!(layout.block_height("profiles"), 280);
        assert_eq!(layout.block_height("server"), 0);

        layout.set_block_height("countdown", 10);
        layout.set_block_height("profiles", 10_000);
        layout.set_block_height("news", 300);
        layout.normalize();

        assert_eq!(layout.block_height("countdown"), 96);
        assert_eq!(layout.block_height("profiles"), 520);
        assert_eq!(layout.block_height("news"), 300);

        layout.block_heights.insert("bogus".into(), 500);
        layout.normalize();
        assert!(!layout.block_heights.contains_key("bogus"));
    }

    #[test]
    fn hidden_block_keeps_its_size_for_restore() {
        let mut layout = LayoutConfig::default();
        layout.set_block_height("countdown", 210);
        layout.set_hidden("countdown", true);
        layout.normalize();

        assert!(layout.is_hidden("countdown"));
        assert_eq!(layout.block_height("countdown"), 210);

        layout.set_hidden("countdown", false);
        assert_eq!(layout.block_height("countdown"), 210);
    }

    #[test]
    fn sidebar_widths_are_clamped() {
        let mut layout = LayoutConfig::default();
        layout.set_widths(10, 10_000);
        assert_eq!(layout.sidebar_left_width, WIDTH_MIN);
        assert_eq!(layout.sidebar_right_width, WIDTH_MAX);
    }

    #[test]
    fn unknown_and_duplicate_items_are_removed() {
        let mut layout = LayoutConfig::default();
        layout.sidebar_left.push("news".into());
        layout.sidebar_left.push("bogus".into());
        layout.normalize();

        let count = layout
            .zones()
            .iter()
            .flat_map(|zone| zone.iter())
            .filter(|item| item.as_str() == "news")
            .count();
        assert_eq!(count, 1);
        assert!(!layout.sidebar_left.iter().any(|item| item == "bogus"));
    }
}
