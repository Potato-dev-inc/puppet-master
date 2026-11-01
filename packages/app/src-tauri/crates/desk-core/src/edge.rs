// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Edge crossing. A point that has left a monitor is mapped onto the neighbor.
//!
//! A single neighbor receives the whole edge proportionally, so a tall monitor
//! beside a short one never traps the cursor on the overhang (R2). Several
//! neighbors split the edge by their real overlap, which is what an L-shaped
//! desk needs.

use crate::layout::{
    horizontal_overlap, vertical_overlap, DeviceId, Layout, Monitor, MonitorId, PhysRect, Side,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossMode {
    Proportional,
    Layout,
}

/// How far apart two facing edges may be and still count as touching.
pub const MAX_GAP_PX: i32 = 80;

/// Where the cursor should appear after leaving a monitor.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrival {
    pub monitor: MonitorId,
    pub device_id: DeviceId,
    pub x: i32,
    pub y: i32,
    /// The side of the *source* monitor that was crossed.
    pub side: Side,
    /// 0..1 along the source edge.
    pub t: f64,
}

/// Map an attempted cursor position that may lie outside `from`.
/// Returns `None` when the point is still inside, or when the edge leads nowhere.
pub fn cross(layout: &Layout, from: &MonitorId, x: i32, y: i32) -> Option<Arrival> {
    cross_with_mode(layout, from, x, y, CrossMode::Proportional)
}

pub fn cross_with_mode(
    layout: &Layout,
    from: &MonitorId,
    x: i32,
    y: i32,
    mode: CrossMode,
) -> Option<Arrival> {
    let source = layout.monitor(from)?;
    let side = outside_side(source.bounds, x, y)?;
    if let Some(arrival) = arrive(layout, source, side, x, y, mode) {
        return Some(arrival);
    }
    if let Some(other) = secondary_side(source.bounds, x, y, side) {
        if let Some(arrival) = arrive(layout, source, other, x, y, mode) {
            return Some(arrival);
        }
    }
    if layout.wrap {
        return wrap_same_device(layout, source, side, x, y);
    }
    None
}

/// Which edge a point outside the rectangle has crossed.
/// On a corner the larger overshoot wins. A tie prefers the vertical edge
/// (left or right), so a diagonal push into a side-by-side seam stays horizontal.
pub fn outside_side(bounds: PhysRect, x: i32, y: i32) -> Option<Side> {
    if bounds.contains(x, y) {
        return None;
    }
    let left = if x < bounds.x { bounds.x - x } else { 0 };
    let right = if x >= bounds.right() {
        x - bounds.right() + 1
    } else {
        0
    };
    let top = if y < bounds.y { bounds.y - y } else { 0 };
    let bottom = if y >= bounds.bottom() {
        y - bounds.bottom() + 1
    } else {
        0
    };
    let horizontal = left.max(right);
    let vertical = top.max(bottom);
    if horizontal == 0 && vertical == 0 {
        return None;
    }
    if horizontal >= vertical && horizontal > 0 {
        if left > right {
            Some(Side::Left)
        } else {
            Some(Side::Right)
        }
    } else if top > bottom {
        Some(Side::Top)
    } else {
        Some(Side::Bottom)
    }
}

fn secondary_side(bounds: PhysRect, x: i32, y: i32, primary: Side) -> Option<Side> {
    let left = x < bounds.x;
    let right = x >= bounds.right();
    let top = y < bounds.y;
    let bottom = y >= bounds.bottom();
    let candidate = if primary.is_vertical() {
        if top {
            Some(Side::Top)
        } else if bottom {
            Some(Side::Bottom)
        } else {
            None
        }
    } else if left {
        Some(Side::Left)
    } else if right {
        Some(Side::Right)
    } else {
        None
    };
    candidate.filter(|side| *side != primary)
}

fn arrive(
    layout: &Layout,
    source: &Monitor,
    side: Side,
    x: i32,
    y: i32,
    mode: CrossMode,
) -> Option<Arrival> {
    let mut neighbors = neighbors(layout, source, side);
    if neighbors.is_empty() {
        return None;
    }
    neighbors.sort_by_key(|m| along_origin(m.bounds, side));
    let coord = along_coord(side, x, y);
    let target = match mode {
        CrossMode::Proportional if neighbors.len() == 1 => neighbors[0],
        CrossMode::Proportional => pick_overlapping(&neighbors, side, coord)?,
        CrossMode::Layout => pick_overlapping(&neighbors, side, coord)?,
    };
    let t = match mode {
        CrossMode::Proportional if neighbors.len() == 1 => {
            fraction_along(source.bounds, side, coord)
        }
        CrossMode::Proportional => fraction_in_overlap(source.bounds, target.bounds, side, coord),
        CrossMode::Layout => fraction_on_target(target.bounds, side, coord),
    };
    let (px, py) = point_on_edge(target.bounds, side.opposite(), t);
    Some(Arrival {
        monitor: target.id.clone(),
        device_id: target.device_id.clone(),
        x: px,
        y: py,
        side,
        t,
    })
}

fn fraction_on_target(target: PhysRect, side: Side, coord: i32) -> f64 {
    let origin = along_origin(target, side);
    let len = along_len(target, side).max(1);
    ((coord - origin) as f64 / len as f64).clamp(0.0, 1.0)
}

fn neighbors<'a>(layout: &'a Layout, source: &Monitor, side: Side) -> Vec<&'a Monitor> {
    layout
        .monitors
        .iter()
        .filter(|m| m.id != source.id)
        .filter(|m| faces(source.bounds, m.bounds, side))
        .collect()
}

fn faces(from: PhysRect, to: PhysRect, side: Side) -> bool {
    let gap = match side {
        Side::Right => to.x - from.right(),
        Side::Left => from.x - to.right(),
        Side::Bottom => to.y - from.bottom(),
        Side::Top => from.y - to.bottom(),
    };
    if !(-8..=MAX_GAP_PX).contains(&gap) {
        return false;
    }
    if side.is_vertical() {
        vertical_overlap(from, to)
    } else {
        horizontal_overlap(from, to)
    }
}

fn pick_overlapping<'a>(neighbors: &[&'a Monitor], side: Side, coord: i32) -> Option<&'a Monitor> {
    neighbors.iter().copied().find(|m| {
        let (lo, hi) = along_span(m.bounds, side);
        coord >= lo && coord < hi
    })
}

fn fraction_along(bounds: PhysRect, side: Side, coord: i32) -> f64 {
    let origin = along_origin(bounds, side);
    let len = along_len(bounds, side).max(1);
    ((coord - origin) as f64 / len as f64).clamp(0.0, 1.0)
}

fn fraction_in_overlap(source: PhysRect, target: PhysRect, side: Side, coord: i32) -> f64 {
    let (s0, s1) = along_span(source, side);
    let (t0, t1) = along_span(target, side);
    let lo = s0.max(t0);
    let hi = s1.min(t1).max(lo.saturating_add(1));
    let clamped = coord.clamp(lo, hi - 1);
    ((clamped - lo) as f64 / (hi - lo) as f64).clamp(0.0, 1.0)
}

fn along_coord(side: Side, x: i32, y: i32) -> i32 {
    if side.is_vertical() {
        y
    } else {
        x
    }
}

fn along_origin(bounds: PhysRect, side: Side) -> i32 {
    if side.is_vertical() {
        bounds.y
    } else {
        bounds.x
    }
}

fn along_len(bounds: PhysRect, side: Side) -> i32 {
    if side.is_vertical() {
        bounds.height as i32
    } else {
        bounds.width as i32
    }
}

fn along_span(bounds: PhysRect, side: Side) -> (i32, i32) {
    let origin = along_origin(bounds, side);
    (origin, origin.saturating_add(along_len(bounds, side)))
}

/// `entry` is the side of the destination we arrive through.
pub fn point_on_edge(bounds: PhysRect, entry: Side, t: f64) -> (i32, i32) {
    let t = t.clamp(0.0, 1.0);
    let along_y = |h: u32| -> i32 {
        if h <= 1 {
            bounds.y
        } else {
            bounds.y + (t * (h - 1) as f64).round() as i32
        }
    };
    let along_x = |w: u32| -> i32 {
        if w <= 1 {
            bounds.x
        } else {
            bounds.x + (t * (w - 1) as f64).round() as i32
        }
    };
    match entry {
        Side::Left => (bounds.x, along_y(bounds.height)),
        Side::Right => (bounds.right().saturating_sub(1), along_y(bounds.height)),
        Side::Top => (along_x(bounds.width), bounds.y),
        Side::Bottom => (along_x(bounds.width), bounds.bottom().saturating_sub(1)),
    }
}

fn wrap_same_device(
    layout: &Layout,
    source: &Monitor,
    side: Side,
    x: i32,
    y: i32,
) -> Option<Arrival> {
    let mine: Vec<&Monitor> = layout.device_monitors(&source.device_id);
    if mine.len() < 2 {
        return None;
    }
    let target = match side {
        Side::Right => mine.iter().copied().min_by_key(|m| m.bounds.x)?,
        Side::Left => mine.iter().copied().max_by_key(|m| m.bounds.right())?,
        Side::Bottom => mine.iter().copied().min_by_key(|m| m.bounds.y)?,
        Side::Top => mine.iter().copied().max_by_key(|m| m.bounds.bottom())?,
    };
    if target.id == source.id {
        return None;
    }
    let coord = along_coord(side, x, y);
    let t = fraction_along(source.bounds, side, coord);
    let (px, py) = point_on_edge(target.bounds, side.opposite(), t);
    Some(Arrival {
        monitor: target.id.clone(),
        device_id: target.device_id.clone(),
        x: px,
        y: py,
        side,
        t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Device, DeviceId, Layout, Monitor, MonitorId, PhysRect};

    fn mon(id: &str, dev: &str, x: i32, y: i32, w: u32, h: u32, scale: f64) -> Monitor {
        Monitor {
            id: MonitorId(id.into()),
            device_id: DeviceId(dev.into()),
            name: id.into(),
            bounds: PhysRect::new(x, y, w, h),
            phys: None,
            scale,
        }
    }

    fn layout(wrap: bool, monitors: Vec<Monitor>) -> Layout {
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
            wrap,
        }
    }

    #[test]
    fn same_size_edge_keeps_the_fraction() {
        let l = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("b", "local", 100, 0, 100, 100, 1.0),
            ],
        );
        let hit = cross(&l, &MonitorId("a".into()), 100, 40).unwrap();
        assert_eq!(hit.monitor.0, "b");
        assert_eq!(hit.side, Side::Right);
        assert_eq!(hit.x, 100);
        assert!((hit.t - 0.4).abs() < 0.02);
        assert!((hit.y - 40).abs() <= 1);
    }

    #[test]
    fn mismatched_heights_map_proportionally() {
        // 200px beside 100px, top aligned. Leaving at 75% of the tall one
        // must land at 75% of the short one, not fall off the end.
        let l = layout(
            false,
            vec![
                mon("tall", "local", 0, 0, 100, 200, 2.0),
                mon("short", "local", 100, 0, 80, 100, 1.0),
            ],
        );
        let hit = cross(&l, &MonitorId("tall".into()), 100, 150).unwrap();
        assert_eq!(hit.monitor.0, "short");
        assert!((hit.t - 0.75).abs() < 0.02, "t={}", hit.t);
        let expected_y = (0.75_f64 * 99.0).round() as i32;
        assert!((hit.y - expected_y).abs() <= 1, "y={}", hit.y);
        assert_eq!(hit.x, 100);
    }

    #[test]
    fn scale_factor_does_not_change_a_same_pixel_mapping() {
        let a = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("b", "local", 100, 0, 100, 100, 1.0),
            ],
        );
        let b = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.5),
                mon("b", "local", 100, 0, 100, 100, 2.0),
            ],
        );
        let left = cross(&a, &MonitorId("a".into()), 100, 25).unwrap();
        let right = cross(&b, &MonitorId("a".into()), 100, 25).unwrap();
        assert_eq!((left.x, left.y), (right.x, right.y));
    }

    #[test]
    fn gap_within_the_limit_still_crosses() {
        let l = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("b", "local", 140, 0, 100, 100, 1.0),
            ],
        );
        let hit = cross(&l, &MonitorId("a".into()), 100, 50).unwrap();
        assert_eq!(hit.monitor.0, "b");
        assert_eq!(hit.x, 140);
    }

    #[test]
    fn gap_past_the_limit_does_not_cross() {
        let l = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("b", "local", 200, 0, 100, 100, 1.0),
            ],
        );
        assert!(cross(&l, &MonitorId("a".into()), 100, 50).is_none());
    }

    #[test]
    fn corner_uses_the_larger_overshoot() {
        let l = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("right", "local", 100, 0, 100, 100, 1.0),
                mon("below", "local", 0, 100, 100, 100, 1.0),
            ],
        );
        let to_right = cross(&l, &MonitorId("a".into()), 110, 101).unwrap();
        assert_eq!(to_right.monitor.0, "right");
        let to_below = cross(&l, &MonitorId("a".into()), 101, 120).unwrap();
        assert_eq!(to_below.monitor.0, "below");
    }

    #[test]
    fn l_shape_splits_the_edge_between_two_neighbors() {
        let l = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 200, 1.0),
                mon("top", "local", 100, 0, 100, 100, 1.0),
                mon("bot", "local", 100, 100, 100, 100, 1.0),
            ],
        );
        let upper = cross(&l, &MonitorId("a".into()), 100, 20).unwrap();
        assert_eq!(upper.monitor.0, "top");
        assert_eq!(upper.y, 20);
        let lower = cross(&l, &MonitorId("a".into()), 100, 150).unwrap();
        assert_eq!(lower.monitor.0, "bot");
        assert_eq!(lower.y, 150);
    }

    #[test]
    fn inside_a_monitor_does_not_cross() {
        let l = layout(false, vec![mon("a", "local", 0, 0, 100, 100, 1.0)]);
        assert!(cross(&l, &MonitorId("a".into()), 50, 50).is_none());
    }

    #[test]
    fn wrap_sends_the_far_right_to_the_far_left() {
        let l = layout(
            true,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("b", "local", 100, 0, 100, 100, 1.0),
            ],
        );
        let hit = cross(&l, &MonitorId("b".into()), 200, 25).unwrap();
        assert_eq!(hit.monitor.0, "a");
        assert_eq!(hit.side, Side::Right);
        assert_eq!(hit.x, 0);
        assert!((hit.y - 25).abs() <= 1);
    }

    #[test]
    fn wrap_does_not_jump_to_another_device() {
        let l = layout(
            true,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("peer", "other", 500, 0, 100, 100, 1.0),
            ],
        );
        assert!(cross(&l, &MonitorId("a".into()), 100, 40).is_none());
    }

    #[test]
    fn peer_on_the_facing_edge_is_the_arrival_device() {
        let l = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 100, 1.0),
                mon("p", "laptop", 100, 0, 100, 80, 2.0),
            ],
        );
        let hit = cross(&l, &MonitorId("a".into()), 100, 50).unwrap();
        assert_eq!(hit.device_id.0, "laptop");
        assert_eq!(hit.monitor.0, "p");
    }

    #[test]
    fn layout_mode_keeps_absolute_y_on_mismatched_heights() {
        let l = layout(
            false,
            vec![
                mon("pc", "local", 0, 100, 800, 600, 1.0),
                mon("mac", "peer", 800, 0, 800, 400, 1.0),
            ],
        );
        let hit =
            cross_with_mode(&l, &MonitorId("pc".into()), 800, 250, CrossMode::Layout).unwrap();
        assert_eq!(hit.monitor.0, "mac");
        assert!((hit.y - 250).abs() <= 1, "y={}", hit.y);
        assert!(
            cross_with_mode(&l, &MonitorId("pc".into()), 800, 450, CrossMode::Layout).is_none()
        );
        let back =
            cross_with_mode(&l, &MonitorId("mac".into()), 799, 250, CrossMode::Layout).unwrap();
        assert_eq!(back.monitor.0, "pc");
        assert!((back.y - 250).abs() <= 1, "y={}", back.y);
    }

    #[test]
    fn layout_mode_l_shape_still_picks_by_overlap() {
        let l = layout(
            false,
            vec![
                mon("a", "local", 0, 0, 100, 200, 1.0),
                mon("top", "local", 100, 0, 100, 100, 1.0),
                mon("bot", "local", 100, 100, 100, 100, 1.0),
            ],
        );
        let upper =
            cross_with_mode(&l, &MonitorId("a".into()), 100, 20, CrossMode::Layout).unwrap();
        assert_eq!(upper.monitor.0, "top");
        assert_eq!(upper.y, 20);
        let lower =
            cross_with_mode(&l, &MonitorId("a".into()), 100, 150, CrossMode::Layout).unwrap();
        assert_eq!(lower.monitor.0, "bot");
        assert_eq!(lower.y, 150);
    }

    #[test]
    fn proportional_mode_unchanged_for_mismatched_heights() {
        let l = layout(
            false,
            vec![
                mon("tall", "local", 0, 0, 100, 200, 1.0),
                mon("short", "local", 100, 0, 80, 100, 1.0),
            ],
        );
        let hit = cross_with_mode(
            &l,
            &MonitorId("tall".into()),
            100,
            150,
            CrossMode::Proportional,
        )
        .unwrap();
        assert!((hit.t - 0.75).abs() < 0.02);
        let expected_y = (0.75_f64 * 99.0).round() as i32;
        assert!((hit.y - expected_y).abs() <= 1);
    }
}
