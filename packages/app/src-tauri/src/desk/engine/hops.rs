// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Local hops: pick the screen and the point to move the pointer to. Pure; the engine moves the cursor.
//!
//! The screen order and the "same place on the next screen" maths are `desk-core` (`reading_order`, `hop`).
//! What DevHop got wrong around them is fixed here:
//! - the pointer in a gap between screens (or on a screen that just went away) made every hop do nothing;
//!   it now counts as being on the nearest screen;
//! - mirrored (duplicate) displays counted as two screens, so "next" seemed to do nothing;
//! - "screen 5" with three screens failed without a word; it is now an error with the count.

use desk_core::{
    denormalize, hop_monitors, normalize, reading_order, Device, DeviceId, HopAction, HopMode, Layout, Monitor,
    MonitorId, NormPos, PhysRect, PositionMemory,
};

use crate::desk::host::DisplayInfo;

pub const LOCAL: &str = "local";

#[derive(Clone, Debug, PartialEq)]
pub struct HopTarget {
    pub x: i32,
    pub y: i32,
    pub screen_id: String,
    /// 1-based position in reading order, the number a "go to screen N" key uses.
    pub screen_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HopError {
    NoScreens,
    NoSuchScreen { wanted: u8, have: usize },
}

impl std::fmt::Display for HopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HopError::NoScreens => write!(f, "No screens found"),
            HopError::NoSuchScreen { wanted, have } => {
                write!(f, "There is no screen {wanted}: this computer has {have}")
            }
        }
    }
}

/// The layout of this computer's screens. Zero-size and mirrored screens are left out.
pub fn layout_of(displays: &[DisplayInfo]) -> Layout {
    let mut monitors: Vec<Monitor> = Vec::new();
    // Primary first, so of two mirrored screens the primary is the one kept.
    let mut sorted: Vec<&DisplayInfo> = displays.iter().filter(|d| d.width > 0 && d.height > 0).collect();
    sorted.sort_by_key(|d| !d.primary);
    for d in sorted {
        let bounds = PhysRect::new(d.x, d.y, d.width, d.height);
        if monitors.iter().any(|m| m.bounds == bounds) {
            continue;
        }
        monitors.push(Monitor {
            id: MonitorId(d.id.clone()),
            device_id: DeviceId(LOCAL.into()),
            name: d.name.clone(),
            bounds,
            phys: None,
            scale: d.scale,
        });
    }
    Layout { devices: vec![Device { id: DeviceId(LOCAL.into()), name: "This computer".into() }], monitors, wrap: false }
}

fn distance_sq(rect: PhysRect, x: i32, y: i32) -> i64 {
    let dx = if x < rect.x {
        i64::from(rect.x) - i64::from(x)
    } else if x >= rect.right() {
        i64::from(x) - i64::from(rect.right() - 1)
    } else {
        0
    };
    let dy = if y < rect.y {
        i64::from(rect.y) - i64::from(y)
    } else if y >= rect.bottom() {
        i64::from(y) - i64::from(rect.bottom() - 1)
    } else {
        0
    };
    dx * dx + dy * dy
}

/// The screen the point is on, or the nearest one when it is in a gap.
pub fn locate<'a>(layout: &'a Layout, x: i32, y: i32) -> Option<&'a Monitor> {
    if let Some(m) = layout.monitor_at(x, y) {
        return Some(m);
    }
    let order = reading_order(layout, &DeviceId(LOCAL.into()));
    order
        .iter()
        .filter_map(|id| layout.monitor(id))
        .min_by_key(|m| distance_sq(m.bounds, x, y))
}

/// Screen ids in reading order.
pub fn order(layout: &Layout) -> Vec<MonitorId> {
    reading_order(layout, &DeviceId(LOCAL.into()))
}

/// 1-based reading-order number of a screen.
pub fn index_of(layout: &Layout, id: &MonitorId) -> Option<usize> {
    order(layout).iter().position(|m| m == id).map(|i| i + 1)
}

pub fn plan(
    layout: &Layout,
    mode: HopMode,
    memory: &mut PositionMemory,
    pointer: (i32, i32),
    action: HopAction,
) -> Result<HopTarget, HopError> {
    let order = order(layout);
    if order.is_empty() {
        return Err(HopError::NoScreens);
    }
    if let HopAction::Number(n) = action {
        if n == 0 || usize::from(n) > order.len() {
            return Err(HopError::NoSuchScreen { wanted: n, have: order.len() });
        }
    }
    let here = locate(layout, pointer.0, pointer.1).ok_or(HopError::NoScreens)?;
    // A pointer in a gap is first moved onto its nearest screen's edge for the "same place" maths.
    let (cx, cy) = (
        pointer.0.clamp(here.bounds.x, here.bounds.right() - 1),
        pointer.1.clamp(here.bounds.y, here.bounds.bottom() - 1),
    );
    let (nx, ny) = normalize(here.bounds, cx, cy);
    let current = NormPos::new(here.id.clone(), nx, ny);
    memory.remember(&current);
    let dest = hop_monitors(layout, &DeviceId(LOCAL.into()), mode, memory, &current, action)
        .ok_or(HopError::NoScreens)?;
    let target = layout.monitor(&dest.monitor).ok_or(HopError::NoScreens)?;
    let (x, y) = denormalize(target.bounds, dest.nx, dest.ny);
    Ok(HopTarget {
        x,
        y,
        screen_id: target.id.0.clone(),
        screen_index: index_of(layout, &target.id).unwrap_or(1),
    })
}
