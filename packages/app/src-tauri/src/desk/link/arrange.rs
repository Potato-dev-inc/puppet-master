// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (Layout page + layout_store: computers placed in one space; hop-core edge.rs: cross only where screens touch.)
//! The layout of two paired computers in one coordinate space. Pure: no host, no link, no files.
//!
//! Every computer has a **local space**: its screens, moved so the top-left corner of the box around them is (0, 0).
//! A paired computer is *placed* at a [`Spot`]: where its local (0, 0) sits in this computer's local space. Because
//! both spaces are local, the other computer sees the mirror: if B is at (1920, 200) from A, then A is at
//! (-1920, -200) from B. A [`Spot::Side`] is the old "it sits on my right/left/above/below": it stays symbolic, so it
//! keeps touching when either computer changes its screens.
//!
//! The pointer leaves through the edge pixel of a screen only where a screen of the other computer faces it within a
//! small gap and overlaps the pointer's position along the shared edge ([`depart`]); it appears at the same place in
//! the shared space, which is proportional along the shared overlap.

use desk_core::{cross_with_mode, CrossMode, Device, DeviceId, Layout, Monitor, MonitorId, PhysRect, Side};
use serde::{Deserialize, Serialize};

use super::super::host::DisplayInfo;
use super::crossing::ARRIVAL_INSET;
use super::geometry::{inward_by, union};

/// One screen in a computer's local space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub id: String,
    pub name: String,
    pub primary: bool,
    pub rect: PhysRect,
}

/// Where a paired computer sits relative to this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Spot {
    /// Touching on that side, top (or left) edges aligned.
    Side(Side),
    /// Its local origin is at this point of this computer's local space.
    At { x: i32, y: i32 },
}

/// Largest distance from the origin a placement may have (keeps the arithmetic far from overflow).
pub const MAX_COORD: i32 = 200_000;

impl Spot {
    /// How the other computer sees this computer.
    pub fn mirror(self) -> Spot {
        match self {
            Spot::Side(s) => Spot::Side(s.opposite()),
            Spot::At { x, y } => Spot::At { x: -x, y: -y },
        }
    }

    /// The top-left corner of the placed computer, given the size of this computer's box and of theirs.
    pub fn resolve(self, mine: (i32, i32), theirs: (i32, i32)) -> (i32, i32) {
        match self {
            Spot::Side(Side::Right) => (mine.0, 0),
            Spot::Side(Side::Left) => (-theirs.0, 0),
            Spot::Side(Side::Bottom) => (0, mine.1),
            Spot::Side(Side::Top) => (0, -theirs.1),
            Spot::At { x, y } => (x, y),
        }
    }
}

/// The placement two computers agree on before anyone moved anything: the one with the lower id has the other on
/// its right. Both compute it, so they agree without talking.
pub fn default_spot(me: &str, peer: &str) -> Spot {
    if me < peer {
        Spot::Side(Side::Right)
    } else {
        Spot::Side(Side::Left)
    }
}

/// The screens in local space, and where that space's origin is in the operating system's coordinates.
pub fn localize(displays: &[DisplayInfo]) -> (Vec<Shape>, (i32, i32)) {
    let Some(u) = union(displays) else { return (Vec::new(), (0, 0)) };
    let shapes = displays
        .iter()
        .map(|d| Shape {
            id: d.id.clone(),
            name: d.name.clone(),
            primary: d.primary,
            rect: PhysRect::new(d.x - u.left, d.y - u.top, d.width, d.height),
        })
        .collect();
    (shapes, (u.left, u.top))
}

/// Moves a received screen list so the box around it starts at (0, 0).
pub fn normalize(shapes: &[Shape]) -> Vec<Shape> {
    let Some((l, t)) = origin_of(shapes) else { return Vec::new() };
    shapes.iter().map(|s| Shape { rect: PhysRect::new(s.rect.x - l, s.rect.y - t, s.rect.width, s.rect.height), ..s.clone() }).collect()
}

fn origin_of(shapes: &[Shape]) -> Option<(i32, i32)> {
    let l = shapes.iter().map(|s| s.rect.x).min()?;
    let t = shapes.iter().map(|s| s.rect.y).min()?;
    Some((l, t))
}

/// Width and height of the box around the screens.
pub fn size_of(shapes: &[Shape]) -> (i32, i32) {
    let Some((l, t)) = origin_of(shapes) else { return (0, 0) };
    let r = shapes.iter().map(|s| s.rect.right()).max().unwrap_or(l);
    let b = shapes.iter().map(|s| s.rect.bottom()).max().unwrap_or(t);
    (r - l, b - t)
}

/// The screens moved by (dx, dy).
pub fn shifted(shapes: &[Shape], by: (i32, i32)) -> Vec<Shape> {
    shapes.iter().map(|s| Shape { rect: PhysRect::new(s.rect.x + by.0, s.rect.y + by.1, s.rect.width, s.rect.height), ..s.clone() }).collect()
}

fn overlaps(a: PhysRect, b: PhysRect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// Whether any screen of `a` lies on top of any screen of `b`.
pub fn collide(a: &[Shape], b: &[Shape]) -> bool {
    a.iter().any(|x| b.iter().any(|y| overlaps(x.rect, y.rect)))
}

/// Which side of this computer the other one is on, for the plain "On the right" words: by where its middle is.
pub fn dominant_side(mine: (i32, i32), theirs: (i32, i32), at: (i32, i32)) -> Side {
    let cx = i64::from(at.0) + i64::from(theirs.0) / 2 - i64::from(mine.0) / 2;
    let cy = i64::from(at.1) + i64::from(theirs.1) / 2 - i64::from(mine.1) / 2;
    // Compare as shares of the combined box so a wide pair and a tall pair both read naturally.
    let across = i64::from(mine.0 + theirs.0).max(1);
    let down = i64::from(mine.1 + theirs.1).max(1);
    if cx.abs() * down >= cy.abs() * across {
        if cx >= 0 { Side::Right } else { Side::Left }
    } else if cy >= 0 {
        Side::Bottom
    } else {
        Side::Top
    }
}

const ME: &str = "me";
const THEM: &str = "them";

/// Both computers' screens in one [`Layout`] (this computer's local space). Monitor ids are `me:<index>` and
/// `them:<index>`.
pub fn combined(mine: &[Shape], theirs: &[Shape], at: (i32, i32)) -> Layout {
    let mut monitors = Vec::new();
    for (device, shapes, by) in [(ME, mine, (0, 0)), (THEM, theirs, at)] {
        for (i, s) in shapes.iter().enumerate() {
            monitors.push(Monitor {
                id: MonitorId(format!("{device}:{i}")),
                device_id: DeviceId(device.into()),
                name: s.name.clone(),
                bounds: PhysRect::new(s.rect.x + by.0, s.rect.y + by.1, s.rect.width, s.rect.height),
                phys: None,
                scale: 1.0,
            });
        }
    }
    let devices = [ME, THEM].iter().map(|d| Device { id: DeviceId((*d).into()), name: (*d).into() }).collect();
    Layout { devices, monitors, wrap: false }
}

/// Where the pointer goes after leaving through an edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Departure {
    /// The edge of the screen it left through (on the computer it left).
    pub side: Side,
    /// The screen it arrives on (an id from the other computer's list).
    pub screen: String,
    /// The arrival point in the other computer's local space, a few pixels inside the edge.
    pub to: (i32, i32),
}

pub(super) fn on_edge(r: PhysRect, p: (i32, i32), side: Side) -> bool {
    match side {
        Side::Left => p.0 == r.x,
        Side::Right => p.0 == r.right() - 1,
        Side::Top => p.1 == r.y,
        Side::Bottom => p.1 == r.bottom() - 1,
    }
}

pub(super) fn step(p: (i32, i32), side: Side) -> (i32, i32) {
    match side {
        Side::Left => (p.0 - 1, p.1),
        Side::Right => (p.0 + 1, p.1),
        Side::Top => (p.0, p.1 - 1),
        Side::Bottom => (p.0, p.1 + 1),
    }
}

fn clamp_into(r: PhysRect, p: (i32, i32)) -> (i32, i32) {
    (p.0.clamp(r.x, r.right() - 1), p.1.clamp(r.y, r.bottom() - 1))
}

/// The pointer rests at `pos` (in `mine`'s local space) and wants to go out through one of `sides`. `theirs` is the
/// other computer, placed at `at`. `None` when `pos` is not on an edge pixel of a screen, when the screen next to
/// that edge is one of this computer's own, when nothing of theirs faces the edge within the gap, or when the
/// pointer is beside the overlap (the gap in the middle of a staggered pair is a wall).
pub fn depart(mine: &[Shape], theirs: &[Shape], at: (i32, i32), pos: (i32, i32), sides: &[Side]) -> Option<Departure> {
    depart_inset(mine, theirs, at, pos, sides, ARRIVAL_INSET)
}

/// `depart` with the arrival `inset` (pixels inside the arriving edge) given: a crossing uses zero.
pub fn depart_inset(mine: &[Shape], theirs: &[Shape], at: (i32, i32), pos: (i32, i32), sides: &[Side], inset: i32) -> Option<Departure> {
    let (index, src) = mine.iter().enumerate().find(|(_, s)| s.rect.contains(pos.0, pos.1))?;
    let mut layout = None;
    let from = MonitorId(format!("{ME}:{index}"));
    for &side in sides {
        if !on_edge(src.rect, pos, side) {
            continue;
        }
        let layout = layout.get_or_insert_with(|| combined(mine, theirs, at));
        let out = step(pos, side);
        let Some(arrival) = cross_with_mode(layout, &from, out.0, out.1, CrossMode::Layout) else { continue };
        if arrival.device_id.as_str() != THEM {
            continue;
        }
        let Some(j) = arrival.monitor.as_str().strip_prefix("them:").and_then(|n| n.parse::<usize>().ok()) else { continue };
        let Some(target) = theirs.get(j) else { continue };
        // The same place in the shared space: the position along the edge is kept exactly, the other coordinate is
        // the facing edge of the target.
        let r = target.rect;
        let local = match side {
            Side::Right => (r.x, pos.1 - at.1),
            Side::Left => (r.right() - 1, pos.1 - at.1),
            Side::Bottom => (pos.0 - at.0, r.y),
            Side::Top => (pos.0 - at.0, r.bottom() - 1),
        };
        let to = clamp_into(r, inward_by(clamp_into(r, local), side.opposite(), inset));
        return Some(Departure { side, screen: target.id.clone(), to });
    }
    None
}

/// The sides a motion `(dx, dy)` points out of.
pub fn outward(delta: (i32, i32)) -> Vec<Side> {
    let mut v = Vec::new();
    if delta.0 > 0 {
        v.push(Side::Right);
    } else if delta.0 < 0 {
        v.push(Side::Left);
    }
    if delta.1 > 0 {
        v.push(Side::Bottom);
    } else if delta.1 < 0 {
        v.push(Side::Top);
    }
    v
}

pub const ALL_SIDES: [Side; 4] = [Side::Right, Side::Left, Side::Bottom, Side::Top];

// ---- the wire -------------------------------------------------------------------------------------------------

/// `StreamMsg::Layout` carries this as JSON: the sender's screens and where the *sender* sits relative to the
/// receiver, with the revision of that decision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutMsg {
    pub v: u8,
    pub screens: Vec<Shape>,
    pub spot: Spot,
    pub rev: u64,
}

pub const LAYOUT_V: u8 = 1;
const MAX_SCREENS: usize = 64;
const MAX_MSG_BYTES: usize = 64 * 1024;

pub fn encode_msg(m: &LayoutMsg) -> Vec<u8> {
    serde_json::to_vec(m).unwrap_or_default()
}

/// A message from the network: checked, and the screens made local.
pub fn decode_msg(bytes: &[u8]) -> Option<LayoutMsg> {
    if bytes.len() > MAX_MSG_BYTES {
        return None;
    }
    let mut m: LayoutMsg = serde_json::from_slice(bytes).ok()?;
    if m.v != LAYOUT_V || m.screens.is_empty() || m.screens.len() > MAX_SCREENS {
        return None;
    }
    let sane = |v: i32| v.abs() <= MAX_COORD;
    for s in &mut m.screens {
        if s.rect.width == 0 || s.rect.height == 0 || s.rect.width > MAX_COORD as u32 || s.rect.height > MAX_COORD as u32 || !sane(s.rect.x) || !sane(s.rect.y) {
            return None;
        }
        s.name.truncate(80);
        s.id.truncate(120);
    }
    if let Spot::At { x, y } = m.spot {
        if !sane(x) || !sane(y) {
            return None;
        }
    }
    m.screens = normalize(&m.screens);
    Some(m)
}

/// `Enter.monitor_id` for a point: the edge it comes in through and where, in the receiver's local space.
pub fn enter_tag(entry: Side, p: (i32, i32)) -> String {
    format!("{}@{},{}", super::geometry::side_name(entry), p.0, p.1)
}

pub fn parse_enter_tag(tag: &str) -> Option<(Side, (i32, i32))> {
    let (side, xy) = tag.split_once('@')?;
    let (x, y) = xy.split_once(',')?;
    Some((super::geometry::parse_side(side)?, (x.parse().ok()?, y.parse().ok()?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, x: i32, y: i32, w: u32, h: u32) -> Shape {
        Shape { id: id.into(), name: id.into(), primary: false, rect: PhysRect::new(x, y, w, h) }
    }

    fn one(w: u32, h: u32) -> Vec<Shape> {
        vec![s("a", 0, 0, w, h)]
    }

    #[test]
    fn it_crosses_where_screens_touch_and_appears_at_the_same_height() {
        let a = one(1920, 1080);
        let b = one(1920, 1080);
        let d = depart(&a, &b, (1920, 0), (1919, 300), &[Side::Right]).unwrap();
        assert_eq!(d.side, Side::Right);
        assert_eq!(d.screen, "a");
        assert_eq!(d.to, (0, 300), "the edge pixel of the other computer, same row");
    }

    #[test]
    fn it_does_not_cross_when_the_pointer_is_not_on_the_edge() {
        let a = one(1920, 1080);
        assert_eq!(depart(&a, &a, (1920, 0), (1918, 300), &ALL_SIDES), None);
        assert_eq!(depart(&a, &a, (1920, 0), (960, 540), &ALL_SIDES), None);
        // The wrong edge for where the other computer is.
        assert_eq!(depart(&a, &a, (1920, 0), (0, 300), &ALL_SIDES), None);
    }

    #[test]
    fn it_does_not_cross_across_a_gap() {
        let a = one(1920, 1080);
        assert!(depart(&a, &a, (1940, 0), (1919, 300), &ALL_SIDES).is_some(), "20 px apart still counts as touching");
        assert_eq!(depart(&a, &a, (2200, 0), (1919, 300), &ALL_SIDES), None, "280 px apart is a wall");
        // Diagonal neighbours do not touch.
        assert_eq!(depart(&a, &a, (1920, 1080), (1919, 1079), &ALL_SIDES), None);
    }

    #[test]
    fn the_part_of_an_edge_beside_the_overlap_is_a_wall() {
        // B is 800 high and sits 200 px down: only y 200..1000 of A touches it.
        let a = one(1920, 1080);
        let b = one(1280, 800);
        assert_eq!(depart(&a, &b, (1920, 200), (1919, 100), &ALL_SIDES), None, "above B");
        assert_eq!(depart(&a, &b, (1920, 200), (1919, 1050), &ALL_SIDES), None, "below B");
        let d = depart(&a, &b, (1920, 200), (1919, 500), &ALL_SIDES).unwrap();
        assert_eq!(d.to, (0, 300), "the same place in the shared space: 500 down A is 300 down B");
        assert_eq!(depart(&a, &b, (1920, 200), (1919, 200), &ALL_SIDES).unwrap().to, (0, 0));
    }

    #[test]
    fn the_way_back_lands_at_the_matching_spot() {
        let a = one(1920, 1080);
        let b = one(1280, 800);
        let at = (1920, 200);
        let there = depart(&a, &b, at, (1919, 500), &[Side::Right]).unwrap();
        // B's pointer pushes left from where it arrived (the edge pixel at the same height).
        let back = depart(&b, &a, Spot::At { x: at.0, y: at.1 }.mirror().resolve(size_of(&b), size_of(&a)), (0, there.to.1), &[Side::Left]).unwrap();
        assert_eq!(back.side, Side::Left);
        assert_eq!(back.to, (1920 - 1, 500));
    }

    #[test]
    fn mirror_placement_is_the_opposite_offset() {
        let at = Spot::At { x: 1920, y: 200 };
        assert_eq!(at.mirror(), Spot::At { x: -1920, y: -200 });
        assert_eq!(at.mirror().mirror(), at);
        assert_eq!(Spot::Side(Side::Right).mirror(), Spot::Side(Side::Left));
        assert_eq!(Spot::Side(Side::Top).mirror(), Spot::Side(Side::Bottom));
        // Resolved: B at the right of A means A at the left of B, and the offsets are exact negatives.
        let (wa, ha) = (1920, 1080);
        let (wb, hb) = (1280, 800);
        let ab = Spot::Side(Side::Right).resolve((wa, ha), (wb, hb));
        let ba = Spot::Side(Side::Right).mirror().resolve((wb, hb), (wa, ha));
        assert_eq!(ab, (1920, 0));
        assert_eq!(ba, (-1920, 0));
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            let p = Spot::Side(side).resolve((wa, ha), (wb, hb));
            let q = Spot::Side(side).mirror().resolve((wb, hb), (wa, ha));
            assert_eq!((p.0, p.1), (-q.0, -q.1), "{side:?}");
        }
    }

    #[test]
    fn both_computers_cross_at_the_same_seam_from_either_side() {
        let a = vec![s("a1", 0, 0, 1000, 800), s("a2", 1000, 0, 1000, 800)];
        let b = vec![s("b1", 0, 0, 600, 1200)];
        // B is at the right of A, 300 px up (negative y).
        let at = Spot::At { x: 2000, y: -300 };
        let pa = at.resolve(size_of(&a), size_of(&b));
        let pb = at.mirror().resolve(size_of(&b), size_of(&a));
        assert_eq!(pa, (2000, -300));
        assert_eq!(pb, (-2000, 300));
        let out = depart(&a, &b, pa, (1999, 100), &ALL_SIDES).unwrap();
        assert_eq!(out.to, (0, 400));
        let back = depart(&b, &a, pb, (0, 400), &ALL_SIDES).unwrap();
        assert_eq!(back.screen, "a2");
        assert_eq!(back.to, (1999, 100), "the right edge pixel of a2");
    }

    #[test]
    fn a_screen_of_my_own_next_to_the_edge_is_not_a_door() {
        let a = vec![s("a1", 0, 0, 1000, 800), s("a2", 1000, 0, 1000, 800)];
        let b = one(500, 800);
        // The seam between A's own two screens.
        assert_eq!(depart(&a, &b, (2000, 0), (999, 400), &ALL_SIDES), None);
        assert!(depart(&a, &b, (2000, 0), (1999, 400), &ALL_SIDES).is_some());
    }

    #[test]
    fn negative_origins_work() {
        // A has a screen to the left of its main one (a monitor at negative OS x).
        let displays = [
            DisplayInfo { id: "l".into(), name: "l".into(), x: -1280, y: 100, width: 1280, height: 1024, scale: 1.0, primary: false },
            DisplayInfo { id: "m".into(), name: "m".into(), x: 0, y: 0, width: 1920, height: 1080, scale: 1.0, primary: true },
        ];
        let (a, origin) = localize(&displays);
        assert_eq!(origin, (-1280, 0));
        assert_eq!(a[0].rect, PhysRect::new(0, 100, 1280, 1024));
        assert_eq!(a[1].rect, PhysRect::new(1280, 0, 1920, 1080));
        assert_eq!(size_of(&a), (3200, 1124));
        // B sits to the left of everything, aligned at the top: leaving through the far left screen's left edge.
        let b = one(800, 600);
        let at = Spot::Side(Side::Left).resolve(size_of(&a), size_of(&b));
        assert_eq!(at, (-800, 0));
        let pos = (0, 400); // OS (-1280, 400)
        let d = depart(&a, &b, at, pos, &ALL_SIDES).unwrap();
        assert_eq!((d.side, d.to), (Side::Left, (800 - 1, 400)));
        // The top of that screen is above where B's screen ends only if y < 600: y 700 is below B.
        assert_eq!(depart(&a, &b, at, (0, 700), &ALL_SIDES), None);
        // Negative placement of the other computer.
        let far = Spot::At { x: -2000, y: -500 };
        let at = far.resolve(size_of(&a), size_of(&b));
        assert_eq!(at, (-2000, -500));
    }

    #[test]
    fn a_tall_pair_above_and_below_crosses_on_the_horizontal_edges() {
        let a = one(1920, 1080);
        let b = one(1000, 1000);
        // B below A, 500 px to the right.
        let d = depart(&a, &b, (500, 1080), (1000, 1079), &[Side::Bottom]).unwrap();
        assert_eq!(d.to, (500, 0));
        assert_eq!(depart(&a, &b, (500, 1080), (100, 1079), &[Side::Bottom]), None);
    }

    #[test]
    fn a_corner_pixel_tries_both_edges() {
        let a = one(1000, 1000);
        let b = one(1000, 1000);
        let d = depart(&a, &b, (0, 1000), (999, 999), &ALL_SIDES).unwrap();
        assert_eq!(d.side, Side::Bottom);
    }

    #[test]
    fn dominant_side_reads_the_layout_as_plain_words() {
        let m = (1920, 1080);
        let t = (1280, 800);
        assert_eq!(dominant_side(m, t, (1920, 0)), Side::Right);
        assert_eq!(dominant_side(m, t, (-1280, 100)), Side::Left);
        assert_eq!(dominant_side(m, t, (300, 1080)), Side::Bottom);
        assert_eq!(dominant_side(m, t, (300, -800)), Side::Top);
    }

    #[test]
    fn collisions_are_overlaps_not_touching() {
        let a = one(100, 100);
        assert!(!collide(&a, &shifted(&one(100, 100), (100, 0))));
        assert!(collide(&a, &shifted(&one(100, 100), (99, 0))));
        assert!(!collide(&a, &[]));
    }

    #[test]
    fn the_default_is_the_same_decision_on_both_computers() {
        assert_eq!(default_spot("aa", "bb"), Spot::Side(Side::Right));
        assert_eq!(default_spot("bb", "aa"), Spot::Side(Side::Left));
        assert_eq!(default_spot("aa", "bb").mirror(), default_spot("bb", "aa"));
    }

    #[test]
    fn the_wire_message_round_trips_and_refuses_nonsense() {
        let m = LayoutMsg { v: LAYOUT_V, screens: vec![s("a", 100, 50, 800, 600)], spot: Spot::At { x: -5, y: 7 }, rev: 9 };
        let back = decode_msg(&encode_msg(&m)).unwrap();
        assert_eq!(back.screens[0].rect, PhysRect::new(0, 0, 800, 600), "made local");
        assert_eq!((back.spot, back.rev), (m.spot, 9));
        assert!(decode_msg(b"not json").is_none());
        assert!(decode_msg(&vec![b' '; 70_000]).is_none());
        let mut bad = m.clone();
        bad.v = 9;
        assert!(decode_msg(&encode_msg(&bad)).is_none());
        let mut bad = m.clone();
        bad.screens[0].rect.width = 0;
        assert!(decode_msg(&encode_msg(&bad)).is_none());
        let mut bad = m.clone();
        bad.spot = Spot::At { x: 900_000, y: 0 };
        assert!(decode_msg(&encode_msg(&bad)).is_none());
        let mut bad = m;
        bad.screens.clear();
        assert!(decode_msg(&encode_msg(&bad)).is_none());
    }

    #[test]
    fn the_entry_tag_round_trips() {
        assert_eq!(parse_enter_tag(&enter_tag(Side::Left, (3, -40))), Some((Side::Left, (3, -40))));
        assert_eq!(parse_enter_tag("left"), None);
        assert_eq!(parse_enter_tag("diagonal@1,2"), None);
        assert_eq!(parse_enter_tag("left@x,2"), None);
    }
}
