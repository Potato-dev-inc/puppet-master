// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Local monitor hops. Relative keeps the fractional position, memory restores
//! the last visit, and center drops the cursor in the middle (R1).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::layout::{vertical_overlap, DeviceId, Layout, Monitor, MonitorId, PhysRect};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HopMode {
    #[default]
    Relative,
    Memory,
    Center,
}

/// Cursor position as a fraction of its monitor, each axis in 0..1.
#[derive(Clone, Debug, PartialEq)]
pub struct NormPos {
    pub monitor: MonitorId,
    pub nx: f64,
    pub ny: f64,
}

impl NormPos {
    pub fn new(monitor: MonitorId, nx: f64, ny: f64) -> Self {
        Self {
            monitor,
            nx: nx.clamp(0.0, 1.0),
            ny: ny.clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HopAction {
    Next,
    Prev,
    /// 1-based index in reading order. `1` is the first monitor.
    Number(u8),
    Center,
}

#[derive(Clone, Debug, Default)]
pub struct PositionMemory {
    last: HashMap<MonitorId, (f64, f64)>,
}

impl PositionMemory {
    pub fn remember(&mut self, pos: &NormPos) {
        self.last.insert(
            pos.monitor.clone(),
            (pos.nx.clamp(0.0, 1.0), pos.ny.clamp(0.0, 1.0)),
        );
    }

    pub fn recall(&self, id: &MonitorId) -> Option<(f64, f64)> {
        self.last.get(id).copied()
    }
}

pub fn normalize(bounds: PhysRect, x: i32, y: i32) -> (f64, f64) {
    let nx = if bounds.width <= 1 {
        0.5
    } else {
        (x - bounds.x) as f64 / (bounds.width - 1) as f64
    };
    let ny = if bounds.height <= 1 {
        0.5
    } else {
        (y - bounds.y) as f64 / (bounds.height - 1) as f64
    };
    (nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0))
}

pub fn denormalize(bounds: PhysRect, nx: f64, ny: f64) -> (i32, i32) {
    let x = if bounds.width <= 1 {
        bounds.x
    } else {
        bounds.x + (nx.clamp(0.0, 1.0) * (bounds.width - 1) as f64).round() as i32
    };
    let y = if bounds.height <= 1 {
        bounds.y
    } else {
        bounds.y + (ny.clamp(0.0, 1.0) * (bounds.height - 1) as f64).round() as i32
    };
    (x, y)
}

/// Left to right along a row, then the next row down.
pub fn reading_order(layout: &Layout, device: &DeviceId) -> Vec<MonitorId> {
    let mut pending = layout.device_monitors(device);
    pending.sort_by(|a, b| {
        a.bounds
            .y
            .cmp(&b.bounds.y)
            .then(a.bounds.x.cmp(&b.bounds.x))
            .then(a.id.0.cmp(&b.id.0))
    });
    let mut rows: Vec<Vec<&Monitor>> = Vec::new();
    for monitor in pending {
        if let Some(row) = rows.iter_mut().find(|row| {
            row.iter()
                .any(|other| vertical_overlap(other.bounds, monitor.bounds))
        }) {
            row.push(monitor);
        } else {
            rows.push(vec![monitor]);
        }
    }
    let mut ids = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| a.bounds.x.cmp(&b.bounds.x).then(a.id.0.cmp(&b.id.0)));
        ids.extend(row.into_iter().map(|m| m.id.clone()));
    }
    ids
}

/// Choose the destination of a hotkey hop. The caller should `remember` the
/// current position before calling this, so memory mode can restore it later.
pub fn hop(
    layout: &Layout,
    device: &DeviceId,
    mode: HopMode,
    memory: &PositionMemory,
    current: &NormPos,
    action: HopAction,
) -> Option<NormPos> {
    let order = reading_order(layout, device);
    if order.is_empty() {
        return None;
    }
    let target_id = match action {
        HopAction::Center => current.monitor.clone(),
        HopAction::Next => step(&order, &current.monitor, 1)?,
        HopAction::Prev => step(&order, &current.monitor, -1)?,
        HopAction::Number(n) => {
            if n == 0 {
                return None;
            }
            order.get((n - 1) as usize)?.clone()
        }
    };
    let target = layout.monitor(&target_id)?;
    Some(place(mode, memory, current, target, action))
}

pub fn place(
    mode: HopMode,
    memory: &PositionMemory,
    from: &NormPos,
    to: &Monitor,
    action: HopAction,
) -> NormPos {
    if matches!(action, HopAction::Center) {
        return NormPos::new(to.id.clone(), 0.5, 0.5);
    }
    let (nx, ny) = match mode {
        HopMode::Center => (0.5, 0.5),
        HopMode::Relative => (from.nx, from.ny),
        HopMode::Memory => memory.recall(&to.id).unwrap_or((from.nx, from.ny)),
    };
    NormPos::new(to.id.clone(), nx, ny)
}

fn step(order: &[MonitorId], current: &MonitorId, dir: i32) -> Option<MonitorId> {
    let idx = order.iter().position(|id| id == current)?;
    let len = order.len() as i32;
    let next = (idx as i32 + dir).rem_euclid(len) as usize;
    Some(order[next].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Device, DeviceId, Layout, Monitor, MonitorId, PhysRect};

    fn mon(id: &str, x: i32, y: i32, w: u32, h: u32) -> Monitor {
        Monitor {
            id: MonitorId(id.into()),
            device_id: DeviceId("local".into()),
            name: id.into(),
            bounds: PhysRect::new(x, y, w, h),
            phys: None,
            scale: 1.0,
        }
    }

    fn desk(monitors: Vec<Monitor>) -> Layout {
        Layout {
            devices: vec![Device {
                id: DeviceId("local".into()),
                name: "local".into(),
            }],
            monitors,
            wrap: false,
        }
    }

    fn at(id: &str, nx: f64, ny: f64) -> NormPos {
        NormPos::new(MonitorId(id.into()), nx, ny)
    }

    #[test]
    fn reading_order_is_rows_then_columns() {
        let l = desk(vec![
            mon("bl", 0, 100, 100, 100),
            mon("tr", 100, 0, 100, 100),
            mon("tl", 0, 0, 100, 100),
        ]);
        let order = reading_order(&l, &DeviceId("local".into()));
        let names: Vec<&str> = order.iter().map(|id| id.0.as_str()).collect();
        assert_eq!(names, vec!["tl", "tr", "bl"]);
    }

    #[test]
    fn relative_hop_keeps_thirty_percent() {
        let l = desk(vec![mon("a", 0, 0, 200, 100), mon("b", 200, 0, 100, 400)]);
        let dest = hop(
            &l,
            &DeviceId("local".into()),
            HopMode::Relative,
            &PositionMemory::default(),
            &at("a", 0.3, 0.3),
            HopAction::Next,
        )
        .unwrap();
        assert_eq!(dest.monitor.0, "b");
        assert!((dest.nx - 0.3).abs() < f64::EPSILON);
        assert!((dest.ny - 0.3).abs() < f64::EPSILON);
        let bounds = l.monitor(&dest.monitor).unwrap().bounds;
        let (x, y) = denormalize(bounds, dest.nx, dest.ny);
        assert_eq!(x, 200 + (0.3_f64 * 99.0).round() as i32);
        assert_eq!(y, (0.3_f64 * 399.0).round() as i32);
    }

    #[test]
    fn memory_restores_the_previous_visit() {
        let l = desk(vec![mon("a", 0, 0, 100, 100), mon("b", 100, 0, 100, 100)]);
        let mut memory = PositionMemory::default();
        memory.remember(&at("b", 0.8, 0.2));
        let dest = hop(
            &l,
            &DeviceId("local".into()),
            HopMode::Memory,
            &memory,
            &at("a", 0.1, 0.1),
            HopAction::Next,
        )
        .unwrap();
        assert_eq!(dest.monitor.0, "b");
        assert!((dest.nx - 0.8).abs() < f64::EPSILON);
        assert!((dest.ny - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn memory_falls_back_to_relative_on_a_first_visit() {
        let l = desk(vec![mon("a", 0, 0, 100, 100), mon("b", 100, 0, 100, 100)]);
        let dest = hop(
            &l,
            &DeviceId("local".into()),
            HopMode::Memory,
            &PositionMemory::default(),
            &at("a", 0.25, 0.75),
            HopAction::Number(2),
        )
        .unwrap();
        assert!((dest.nx - 0.25).abs() < f64::EPSILON);
        assert!((dest.ny - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn center_mode_and_center_action() {
        let l = desk(vec![mon("a", 0, 0, 100, 100), mon("b", 100, 0, 100, 100)]);
        let by_mode = hop(
            &l,
            &DeviceId("local".into()),
            HopMode::Center,
            &PositionMemory::default(),
            &at("a", 0.1, 0.2),
            HopAction::Next,
        )
        .unwrap();
        assert!((by_mode.nx - 0.5).abs() < f64::EPSILON);
        let by_action = hop(
            &l,
            &DeviceId("local".into()),
            HopMode::Relative,
            &PositionMemory::default(),
            &at("a", 0.1, 0.2),
            HopAction::Center,
        )
        .unwrap();
        assert_eq!(by_action.monitor.0, "a");
        assert!((by_action.nx - 0.5).abs() < f64::EPSILON);
        assert!((by_action.ny - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn next_and_prev_wrap_and_number_is_one_based() {
        let l = desk(vec![
            mon("a", 0, 0, 10, 10),
            mon("b", 10, 0, 10, 10),
            mon("c", 20, 0, 10, 10),
        ]);
        let device = DeviceId("local".into());
        let memory = PositionMemory::default();
        let from_c = hop(
            &l,
            &device,
            HopMode::Relative,
            &memory,
            &at("c", 0.0, 0.0),
            HopAction::Next,
        )
        .unwrap();
        assert_eq!(from_c.monitor.0, "a");
        let from_a = hop(
            &l,
            &device,
            HopMode::Relative,
            &memory,
            &at("a", 0.0, 0.0),
            HopAction::Prev,
        )
        .unwrap();
        assert_eq!(from_a.monitor.0, "c");
        let third = hop(
            &l,
            &device,
            HopMode::Relative,
            &memory,
            &at("a", 0.0, 0.0),
            HopAction::Number(3),
        )
        .unwrap();
        assert_eq!(third.monitor.0, "c");
        assert!(hop(
            &l,
            &device,
            HopMode::Relative,
            &memory,
            &at("a", 0.0, 0.0),
            HopAction::Number(9)
        )
        .is_none());
        assert!(hop(
            &l,
            &device,
            HopMode::Relative,
            &memory,
            &at("a", 0.0, 0.0),
            HopAction::Number(0)
        )
        .is_none());
    }

    #[test]
    fn normalize_round_trip_on_a_corner() {
        let bounds = PhysRect::new(10, 20, 101, 51);
        let (nx, ny) = normalize(bounds, 10, 20);
        assert!(nx.abs() < f64::EPSILON && ny.abs() < f64::EPSILON);
        assert_eq!(denormalize(bounds, nx, ny), (10, 20));
        let (nx, ny) = normalize(bounds, 110, 70);
        assert!((nx - 1.0).abs() < f64::EPSILON);
        assert_eq!(denormalize(bounds, nx, ny), (110, 70));
    }
}
