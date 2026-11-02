// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Which modifier keys to release when remote control ends. The Windows side reads two states per key: what the system
//! thinks is down (`GetKeyState`) and what the person is holding (`GetAsyncKeyState`). A modifier that is logically
//! down but not physically down is stuck: while it stays down every click here is a Ctrl-click or an Alt-click. This
//! file decides which ones to release; the Windows host sends the releases. It holds no OS calls, so it is testable.

use std::collections::BTreeSet;

/// The eleven Windows virtual-key codes of the modifier keys: Shift, Ctrl, Alt (generic and left/right), Win (left/right).
#[cfg_attr(not(windows), allow(dead_code))]
pub const MODIFIER_VKS: [u16; 11] = [
    0x10, // VK_SHIFT
    0x11, // VK_CONTROL
    0x12, // VK_MENU (Alt)
    0xA0, // VK_LSHIFT
    0xA1, // VK_RSHIFT
    0xA2, // VK_LCONTROL
    0xA3, // VK_RCONTROL
    0xA4, // VK_LMENU
    0xA5, // VK_RMENU
    0x5B, // VK_LWIN
    0x5C, // VK_RWIN
];

/// The modifier keys in `logical_down` that are not in `physical_down`, sorted and without duplicates. Keys that are
/// not modifiers are ignored, and so are modifiers that are only physically down.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn modifiers_to_release(logical_down: &[u16], physical_down: &[u16]) -> Vec<u16> {
    logical_down
        .iter()
        .copied()
        .filter(|vk| MODIFIER_VKS.contains(vk) && !physical_down.contains(vk))
        .collect::<BTreeSet<u16>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modifier_set_has_eleven_distinct_keys() {
        let set: BTreeSet<u16> = MODIFIER_VKS.iter().copied().collect();
        assert_eq!(set.len(), 11);
    }

    #[test]
    fn nothing_down_releases_nothing() {
        assert!(modifiers_to_release(&[], &[]).is_empty());
    }

    #[test]
    fn modifiers_that_are_physically_down_are_not_released() {
        assert!(modifiers_to_release(&[0x11, 0xA2], &[0x11, 0xA2]).is_empty());
        assert!(modifiers_to_release(&[0x10, 0xA0, 0xA1], &[0x10, 0xA0, 0xA1, 0x12]).is_empty());
    }

    #[test]
    fn a_stuck_control_key_is_released() {
        assert_eq!(modifiers_to_release(&[0x11], &[]), vec![0x11]);
        assert_eq!(modifiers_to_release(&[0xA2], &[]), vec![0xA2]);
    }

    #[test]
    fn only_the_stuck_side_is_released_when_the_other_is_held() {
        assert_eq!(modifiers_to_release(&[0x11, 0xA2], &[0xA2]), vec![0x11]);
        assert_eq!(modifiers_to_release(&[0xA0, 0xA1], &[0xA1]), vec![0xA0]);
    }

    #[test]
    fn a_stuck_win_key_is_released_left_or_right() {
        assert_eq!(modifiers_to_release(&[0x5B], &[]), vec![0x5B]);
        assert_eq!(modifiers_to_release(&[0x5C], &[]), vec![0x5C]);
        assert_eq!(modifiers_to_release(&[0x5C, 0x5B], &[0x5B]), vec![0x5C]);
    }

    #[test]
    fn a_stuck_alt_key_is_released() {
        assert_eq!(modifiers_to_release(&[0x12, 0xA4, 0xA5], &[0xA5]), vec![0x12, 0xA4]);
    }

    #[test]
    fn duplicates_are_released_once_and_sorted() {
        assert_eq!(modifiers_to_release(&[0xA2, 0xA2, 0x11, 0x11], &[]), vec![0x11, 0xA2]);
    }

    #[test]
    fn the_result_is_sorted_whatever_the_input_order() {
        assert_eq!(modifiers_to_release(&[0xA5, 0x5B, 0x10], &[]), vec![0x10, 0x5B, 0xA5]);
    }

    #[test]
    fn every_modifier_stuck_releases_all_eleven_in_order() {
        let all = MODIFIER_VKS.to_vec();
        let mut sorted = all.clone();
        sorted.sort_unstable();
        assert_eq!(modifiers_to_release(&all, &[]), sorted);
        assert_eq!(modifiers_to_release(&all, &[]).len(), 11);
    }

    #[test]
    fn ordinary_keys_are_ignored() {
        // A, Enter, F1, Space: a stuck letter is not this rule's business (the swallowed-up releases handle it).
        assert!(modifiers_to_release(&[0x41, 0x0D, 0x70, 0x20], &[]).is_empty());
        assert_eq!(modifiers_to_release(&[0x41, 0xA0], &[]), vec![0xA0]);
    }

    #[test]
    fn a_modifier_only_physically_down_is_not_released() {
        assert!(modifiers_to_release(&[], &[0x11, 0x5B]).is_empty());
    }
}
