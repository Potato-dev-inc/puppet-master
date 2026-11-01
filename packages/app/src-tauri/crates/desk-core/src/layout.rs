// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Monitors and devices in one global layout, measured in physical pixels.

use serde::{Deserialize, Serialize};

/// Stable id of a machine. Paired peers use the same id forever.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

impl DeviceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable id of one monitor. Unique across the whole layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MonitorId(pub String);

impl MonitorId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Axis-aligned rectangle in physical pixels. The right and bottom edges are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PhysRect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> i32 {
        self.x.saturating_add_unsigned(self.width)
    }

    pub fn bottom(self) -> i32 {
        self.y.saturating_add_unsigned(self.height)
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

/// An edge of a monitor. Left and right are the vertical edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }

    /// True for the left and right edges.
    pub fn is_vertical(self) -> bool {
        match self {
            Side::Left | Side::Right => true,
            Side::Top | Side::Bottom => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    pub id: MonitorId,
    pub device_id: DeviceId,
    pub name: String,
    /// Virtual layout bounds used for crossing and the editor.
    pub bounds: PhysRect,
    /// Real OS bounds when they differ from `bounds`. Omitted when virtual matches physical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phys: Option<PhysRect>,
    /// Physical pixels per OS point. Windows uses dpi/96, macOS uses the backing scale.
    pub scale: f64,
}

impl Monitor {
    pub fn physical_bounds(&self) -> PhysRect {
        self.phys.unwrap_or(self.bounds)
    }

    /// Peer-native arrangement used when resetting remote blocks to OS geometry.
    pub fn arrangement_bounds(&self) -> PhysRect {
        self.phys.unwrap_or(self.bounds)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub devices: Vec<Device>,
    pub monitors: Vec<Monitor>,
    /// When true, leaving the outer edge of a device re-enters on the opposite side.
    pub wrap: bool,
}

impl Layout {
    pub fn monitor(&self, id: &MonitorId) -> Option<&Monitor> {
        self.monitors.iter().find(|m| &m.id == id)
    }

    pub fn device_monitors(&self, device: &DeviceId) -> Vec<&Monitor> {
        self.monitors
            .iter()
            .filter(|m| &m.device_id == device)
            .collect()
    }

    /// The topmost monitor whose rectangle contains the point.
    pub fn monitor_at(&self, x: i32, y: i32) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.bounds.contains(x, y))
    }

    pub fn device(&self, id: &DeviceId) -> Option<&Device> {
        self.devices.iter().find(|d| &d.id == id)
    }
}

/// Vertical overlap of two rectangles, ignoring x.
pub fn vertical_overlap(a: PhysRect, b: PhysRect) -> bool {
    a.y < b.bottom() && b.y < a.bottom()
}

/// Horizontal overlap of two rectangles, ignoring y.
pub fn horizontal_overlap(a: PhysRect, b: PhysRect) -> bool {
    a.x < b.right() && b.x < a.right()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, dev: &str, x: i32, y: i32, w: u32, h: u32) -> Monitor {
        Monitor {
            id: MonitorId(id.into()),
            device_id: DeviceId(dev.into()),
            name: id.into(),
            bounds: PhysRect::new(x, y, w, h),
            phys: None,
            scale: 1.0,
        }
    }

    fn layout(monitors: Vec<Monitor>) -> Layout {
        let mut devices = Vec::new();
        for m in &monitors {
            if !devices.iter().any(|d: &Device| d.id == m.device_id) {
                devices.push(Device {
                    id: m.device_id.clone(),
                    name: m.device_id.0.clone(),
                });
            }
        }
        Layout {
            devices,
            monitors,
            wrap: false,
        }
    }

    #[test]
    fn one_monitor_contains_its_origin_and_not_the_far_edge() {
        let l = layout(vec![mon("a", "local", 0, 0, 100, 80)]);
        assert!(l.monitor_at(0, 0).is_some());
        assert!(l.monitor_at(99, 79).is_some());
        assert!(l.monitor_at(100, 0).is_none());
        assert_eq!(l.device_monitors(&DeviceId("local".into())).len(), 1);
    }

    #[test]
    fn two_monitors_sit_side_by_side() {
        let l = layout(vec![
            mon("a", "local", 0, 0, 1920, 1080),
            mon("b", "local", 1920, 0, 1920, 1080),
        ]);
        assert_eq!(l.monitor_at(10, 10).unwrap().id.0, "a");
        assert_eq!(l.monitor_at(1920, 10).unwrap().id.0, "b");
        assert_eq!(l.monitors.len(), 2);
    }

    #[test]
    fn three_monitors_and_an_l_shape() {
        let l = layout(vec![
            mon("a", "local", 0, 0, 100, 200),
            mon("b", "local", 100, 0, 100, 100),
            mon("c", "local", 100, 100, 100, 100),
        ]);
        assert_eq!(l.monitor_at(50, 150).unwrap().id.0, "a");
        assert_eq!(l.monitor_at(150, 10).unwrap().id.0, "b");
        assert_eq!(l.monitor_at(150, 150).unwrap().id.0, "c");
        assert!(l.monitor_at(50, 250).is_none());
        assert_eq!(l.device_monitors(&DeviceId("local".into())).len(), 3);
    }

    #[test]
    fn rect_round_trips_through_json() {
        let r = PhysRect::new(-20, 5, 10, 12);
        let s = serde_json::to_string(&r).unwrap();
        let back: PhysRect = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
        assert_eq!(r.right(), -10);
        assert_eq!(r.bottom(), 17);
    }
}
