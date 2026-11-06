//! Main-screen layout: named zones and the order of movable blocks.
//!
//! The layout is intentionally a list of zones rather than a free canvas: it
//! keeps the UI responsive at the 1080×620 minimum and avoids overlapping
//! panels. The server selector lives in the title bar and is pinned to the
//! header zone.

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct LayoutConfig {
    pub sidebar_left: Vec<String>,
    pub sidebar_right: Vec<String>,
    pub header: Vec<String>,
    pub content_top: Vec<String>,
    pub content_bottom: Vec<String>,
    pub footer: Vec<String>,
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
        }
    }
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
