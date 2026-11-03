// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The crossing rules as pure functions: when a pointer resting on an edge is pushed through it, and where it arrives on
//! the other computer. No host, no link, no clock: every rule is tested on every platform.
//!
//! Both directions use the same geometry (`arrange::depart`): leaving this computer and coming back from the other one
//! are one code path, so the two can never disagree about where the edge is.

use desk_core::Side;

use super::arrange::{self, Departure, Shape};

/// Pixels inside the arriving edge where a crossing puts the pointer: none. The pointer arrives ON the edge pixel of the
/// target screen, on the same row, as the old DevHop app did. Jitter is not guarded by an inset any more: a crossing
/// needs a push outward on the edge (`pushed_through`), and the return needs one push (see `hub`).
pub const ARRIVAL_INSET: i32 = 0;

/// True when the pointer `pos` (in `mine`'s local space) sits on the outer edge pixel of one of `mine`'s screens facing
/// `side`, the pixel just outside that edge is not one of this computer's own screens (a seam between two of its own
/// screens is not a door), and the motion `delta` points out through `side`. A zero or inward delta is false.
///
/// Whether another computer lies beyond that edge is decided by `arrange::depart`, not here.
pub fn pushed_through(mine: &[Shape], pos: (i32, i32), delta: (i32, i32), side: Side) -> bool {
    let outward = match side {
        Side::Left => delta.0 < 0,
        Side::Right => delta.0 > 0,
        Side::Top => delta.1 < 0,
        Side::Bottom => delta.1 > 0,
    };
    if !outward {
        return false;
    }
    let Some(src) = mine.iter().find(|s| s.rect.contains(pos.0, pos.1)) else { return false };
    if !arrange::on_edge(src.rect, pos, side) {
        return false;
    }
    let out = arrange::step(pos, side);
    !mine.iter().any(|s| s.rect.contains(out.0, out.1))
}

/// The crossing itself: the pointer was pushed through `side` (`pushed_through`) and a computer of `theirs`, placed at
/// `at` in `mine`'s local space, faces that edge. Where it arrives is `arrange::depart`'s answer, with `ARRIVAL_INSET`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn crossing(mine: &[Shape], theirs: &[Shape], at: (i32, i32), pos: (i32, i32), delta: (i32, i32), side: Side) -> Option<Departure> {
    if !pushed_through(mine, pos, delta, side) {
        return None;
    }
    arrange::depart(mine, theirs, at, pos, &[side])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk::link::arrange::{size_of, Spot};
    use desk_core::PhysRect;

    fn s(id: &str, x: i32, y: i32, w: u32, h: u32) -> Shape {
        Shape { id: id.into(), name: id.into(), primary: false, rect: PhysRect::new(x, y, w, h) }
    }

    fn one(w: u32, h: u32) -> Vec<Shape> {
        vec![s("a", 0, 0, w, h)]
    }

    /// The founder's layout: the PC (1920x1080) holds the mouse; the Mac (1280x800) sits on its LEFT, tops aligned.
    fn founder_layout() -> (Vec<Shape>, Vec<Shape>, (i32, i32)) {
        let pc = one(1920, 1080);
        let mac = one(1280, 800);
        let at = Spot::Side(Side::Left).resolve(size_of(&pc), size_of(&mac));
        (pc, mac, at)
    }

    #[test]
    fn the_mac_on_the_left_is_arrived_at_on_its_right_edge_pixel_on_the_same_row() {
        let (pc, mac, at) = founder_layout();
        assert_eq!(at, (-1280, 0));
        assert!(pushed_through(&pc, (0, 300), (-30, 0), Side::Left));
        let d = crossing(&pc, &mac, at, (0, 300), (-30, 0), Side::Left).unwrap();
        assert_eq!(d.side, Side::Left);
        assert_eq!(d.to, (1279, 300), "the Mac's right edge pixel, row 300");
    }

    #[test]
    fn a_shorter_mac_keeps_the_row_inside_its_own_screen() {
        // The Mac is 800 high and sits 100 px down the PC's left edge: it covers rows 100..=899 of the PC.
        let pc = one(1920, 1080);
        let mac = one(1280, 800);
        let at = (-1280, 100);
        let d = crossing(&pc, &mac, at, (0, 899), (-5, 0), Side::Left).unwrap();
        assert_eq!(d.to, (1279, 799), "row 899 of the PC is row 799 of the Mac");
        assert_eq!(crossing(&pc, &mac, at, (0, 900), (-5, 0), Side::Left), None, "below the Mac is a wall");
        assert_eq!(crossing(&pc, &mac, at, (0, 99), (-5, 0), Side::Left), None, "above the Mac is a wall");
    }

    #[test]
    fn a_gap_of_20_crosses_and_a_gap_of_280_is_a_wall() {
        let pc = one(1920, 1080);
        let mac = one(1280, 800);
        // The Mac's right edge is 20 px left of the PC's left edge.
        let near = crossing(&pc, &mac, (-1300, 0), (0, 300), (-30, 0), Side::Left).unwrap();
        assert_eq!(near.to, (1279, 300));
        // 280 px: beyond the 80 px the layout allows.
        assert_eq!(crossing(&pc, &mac, (-1560, 0), (0, 300), (-30, 0), Side::Left), None);
    }

    #[test]
    fn an_inward_or_zero_delta_on_the_edge_does_not_cross() {
        let (pc, mac, at) = founder_layout();
        assert!(!pushed_through(&pc, (0, 300), (0, 0), Side::Left), "resting on the edge");
        assert!(!pushed_through(&pc, (0, 300), (30, 0), Side::Left), "a move back into the screen");
        assert!(!pushed_through(&pc, (0, 300), (0, -30), Side::Left), "sideways along the edge");
        assert_eq!(crossing(&pc, &mac, at, (0, 300), (0, 0), Side::Left), None);
        assert_eq!(crossing(&pc, &mac, at, (0, 300), (30, 0), Side::Left), None);
    }

    #[test]
    fn only_the_edge_pixel_itself_is_a_push() {
        let (pc, _, _) = founder_layout();
        assert!(!pushed_through(&pc, (1, 300), (-30, 0), Side::Left), "one pixel inside");
        assert!(!pushed_through(&pc, (1919, 300), (-30, 0), Side::Left), "the far edge is not the left one");
        assert!(!pushed_through(&pc, (0, 300), (-30, 0), Side::Right), "the wrong side");
    }

    #[test]
    fn a_seam_between_two_of_the_computers_own_screens_is_not_a_door() {
        let pc = vec![s("a1", 0, 0, 1920, 1080), s("a2", 1920, 0, 1920, 1080)];
        let mac = one(1280, 800);
        let at = Spot::Side(Side::Right).resolve(size_of(&pc), size_of(&mac));
        assert!(!pushed_through(&pc, (1919, 500), (30, 0), Side::Right), "the pixel beyond is the second screen");
        assert_eq!(crossing(&pc, &mac, at, (1919, 500), (30, 0), Side::Right), None);
        // The outer edge of the second screen is a door.
        assert!(pushed_through(&pc, (3839, 500), (30, 0), Side::Right));
        assert_eq!(crossing(&pc, &mac, at, (3839, 500), (30, 0), Side::Right).unwrap().to, (0, 500));
    }

    #[test]
    fn the_way_back_arrives_on_the_pcs_left_edge_pixel() {
        // The Mac, pushing right through its right edge, comes back to the PC's left edge pixel on the same row.
        let (pc, mac, _) = founder_layout();
        let at = Spot::Side(Side::Right).resolve(size_of(&mac), size_of(&pc));
        assert_eq!(at, (1280, 0));
        let back = crossing(&mac, &pc, at, (1279, 300), (30, 0), Side::Right).unwrap();
        assert_eq!(back.to, (0, 300), "the PC's left edge pixel");
    }

    #[test]
    fn the_arrival_inset_is_zero_and_the_inset_is_a_parameter() {
        assert_eq!(ARRIVAL_INSET, 0);
        let (pc, mac, at) = founder_layout();
        let d = arrange::depart_inset(&pc, &mac, at, (0, 300), &[Side::Left], 6).unwrap();
        assert_eq!(d.to, (1279 - 6, 300));
    }
}
