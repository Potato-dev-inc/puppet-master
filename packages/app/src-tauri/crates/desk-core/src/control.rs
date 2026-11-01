// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Who owns the keyboard and mouse.
//!
//! `Local` is this machine. `Remote` means we are driving a peer: the local
//! cursor is hidden and every injected key is remembered so a drop, a panic,
//! or three missed heartbeats can release them (R3, R5, R6).

use std::collections::BTreeSet;

use crate::layout::DeviceId;

pub const HEARTBEAT_MISS_LIMIT: u8 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Local,
    Remote { peer: DeviceId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlEvent {
    /// The cursor crossed into `peer`. `button_held` starts a file drag.
    CrossTo {
        peer: DeviceId,
        button_held: bool,
    },
    /// The cursor crossed back onto this machine.
    CrossHome,
    Heartbeat,
    /// One heartbeat interval passed with no packet.
    HeartbeatMiss,
    Disconnect {
        peer: DeviceId,
    },
    Panic,
    KeyDown(u16),
    KeyUp(u16),
    ButtonDown(u8),
    ButtonUp(u8),
    /// Escape, or an explicit cancel, while a drag is in progress.
    CancelDrag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlCommand {
    HideCursor,
    ShowCursor,
    ParkCursor,
    Forward { peer: DeviceId },
    StopForward,
    ReleaseAll { peer: DeviceId },
    BeginDrag { peer: DeviceId },
    FinishDrag { peer: DeviceId },
    CancelDrag { peer: DeviceId },
}

#[derive(Clone, Debug)]
pub struct ControlMachine {
    phase: Phase,
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u8>,
    misses: u8,
    dragging: bool,
}

impl Default for ControlMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlMachine {
    pub fn new() -> Self {
        Self {
            phase: Phase::Local,
            keys: BTreeSet::new(),
            buttons: BTreeSet::new(),
            misses: 0,
            dragging: false,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn held_keys(&self) -> &BTreeSet<u16> {
        &self.keys
    }

    pub fn held_buttons(&self) -> &BTreeSet<u8> {
        &self.buttons
    }

    pub fn dragging(&self) -> bool {
        self.dragging
    }

    pub fn misses(&self) -> u8 {
        self.misses
    }

    pub fn handle(&mut self, event: ControlEvent) -> Vec<ControlCommand> {
        match event {
            ControlEvent::CrossTo { peer, button_held } => self.enter(peer, button_held),
            ControlEvent::CrossHome => self.leave(),
            ControlEvent::Heartbeat => {
                self.misses = 0;
                Vec::new()
            }
            ControlEvent::HeartbeatMiss => self.miss(),
            ControlEvent::Disconnect { peer } => self.disconnect(peer),
            ControlEvent::Panic => self.panic(),
            ControlEvent::KeyDown(hid) => {
                if self.is_remote() {
                    self.keys.insert(hid);
                }
                Vec::new()
            }
            ControlEvent::KeyUp(hid) => {
                self.keys.remove(&hid);
                Vec::new()
            }
            ControlEvent::ButtonDown(button) => {
                if self.is_remote() {
                    self.buttons.insert(button);
                }
                Vec::new()
            }
            ControlEvent::ButtonUp(button) => self.button_up(button),
            ControlEvent::CancelDrag => self.cancel_drag(),
        }
    }

    fn is_remote(&self) -> bool {
        matches!(self.phase, Phase::Remote { .. })
    }

    fn enter(&mut self, peer: DeviceId, button_held: bool) -> Vec<ControlCommand> {
        if let Phase::Remote { peer: current } = &self.phase {
            if current == &peer {
                return Vec::new();
            }
            let previous = current.clone();
            let mut commands = self.release_remote(previous);
            commands.extend(self.begin_remote(peer, button_held));
            return commands;
        }
        self.begin_remote(peer, button_held)
    }

    fn begin_remote(&mut self, peer: DeviceId, button_held: bool) -> Vec<ControlCommand> {
        self.phase = Phase::Remote { peer: peer.clone() };
        self.misses = 0;
        self.keys.clear();
        self.buttons.clear();
        let mut commands = vec![
            ControlCommand::HideCursor,
            ControlCommand::ParkCursor,
            ControlCommand::Forward { peer: peer.clone() },
        ];
        if button_held {
            self.dragging = true;
            self.buttons.insert(0);
            commands.push(ControlCommand::BeginDrag { peer });
        } else {
            self.dragging = false;
        }
        commands
    }

    fn leave(&mut self) -> Vec<ControlCommand> {
        let Phase::Remote { peer } = &self.phase else {
            return Vec::new();
        };
        self.release_remote(peer.clone())
    }

    fn release_remote(&mut self, peer: DeviceId) -> Vec<ControlCommand> {
        let mut commands = Vec::new();
        if self.dragging {
            commands.push(ControlCommand::CancelDrag { peer: peer.clone() });
            self.dragging = false;
        }
        commands.push(ControlCommand::ReleaseAll { peer });
        commands.push(ControlCommand::StopForward);
        commands.push(ControlCommand::ShowCursor);
        self.phase = Phase::Local;
        self.keys.clear();
        self.buttons.clear();
        self.misses = 0;
        commands
    }

    fn miss(&mut self) -> Vec<ControlCommand> {
        let Phase::Remote { peer } = &self.phase else {
            return Vec::new();
        };
        self.misses = self.misses.saturating_add(1);
        if self.misses >= HEARTBEAT_MISS_LIMIT {
            self.release_remote(peer.clone())
        } else {
            Vec::new()
        }
    }

    fn disconnect(&mut self, peer: DeviceId) -> Vec<ControlCommand> {
        match &self.phase {
            Phase::Remote { peer: current } if current == &peer => self.release_remote(peer),
            Phase::Local | Phase::Remote { .. } => Vec::new(),
        }
    }

    fn panic(&mut self) -> Vec<ControlCommand> {
        match &self.phase {
            Phase::Remote { peer } => self.release_remote(peer.clone()),
            Phase::Local => vec![ControlCommand::ShowCursor, ControlCommand::StopForward],
        }
    }

    fn button_up(&mut self, button: u8) -> Vec<ControlCommand> {
        self.buttons.remove(&button);
        if self.dragging && button == 0 {
            if let Phase::Remote { peer } = &self.phase {
                self.dragging = false;
                return vec![ControlCommand::FinishDrag { peer: peer.clone() }];
            }
        }
        Vec::new()
    }

    fn cancel_drag(&mut self) -> Vec<ControlCommand> {
        if !self.dragging {
            return Vec::new();
        }
        let Phase::Remote { peer } = &self.phase else {
            self.dragging = false;
            return Vec::new();
        };
        self.dragging = false;
        vec![ControlCommand::CancelDrag { peer: peer.clone() }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(name: &str) -> DeviceId {
        DeviceId(name.into())
    }

    #[test]
    fn crossing_hides_the_cursor_and_forwards() {
        let mut m = ControlMachine::new();
        let cmds = m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: false,
        });
        assert_eq!(
            cmds,
            vec![
                ControlCommand::HideCursor,
                ControlCommand::ParkCursor,
                ControlCommand::Forward { peer: peer("mac") },
            ]
        );
        assert_eq!(m.phase(), &Phase::Remote { peer: peer("mac") });
    }

    #[test]
    fn reentry_to_the_same_peer_is_a_no_op() {
        let mut m = ControlMachine::new();
        m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: false,
        });
        let again = m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: false,
        });
        assert!(again.is_empty());
        assert!(matches!(m.phase(), Phase::Remote { .. }));
    }

    #[test]
    fn switching_peers_releases_the_first() {
        let mut m = ControlMachine::new();
        m.handle(ControlEvent::CrossTo {
            peer: peer("a"),
            button_held: false,
        });
        m.handle(ControlEvent::KeyDown(0x04));
        let cmds = m.handle(ControlEvent::CrossTo {
            peer: peer("b"),
            button_held: false,
        });
        assert!(cmds.contains(&ControlCommand::ReleaseAll { peer: peer("a") }));
        assert!(cmds.contains(&ControlCommand::Forward { peer: peer("b") }));
        assert!(m.held_keys().is_empty());
        assert_eq!(m.phase(), &Phase::Remote { peer: peer("b") });
    }

    #[test]
    fn three_misses_release_everything() {
        let mut m = ControlMachine::new();
        m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: false,
        });
        m.handle(ControlEvent::KeyDown(0x04));
        m.handle(ControlEvent::ButtonDown(0));
        assert!(m.handle(ControlEvent::HeartbeatMiss).is_empty());
        assert!(m.handle(ControlEvent::HeartbeatMiss).is_empty());
        assert_eq!(m.misses(), 2);
        let cmds = m.handle(ControlEvent::HeartbeatMiss);
        assert!(cmds.contains(&ControlCommand::ReleaseAll { peer: peer("mac") }));
        assert!(cmds.contains(&ControlCommand::ShowCursor));
        assert_eq!(m.phase(), &Phase::Local);
        assert!(m.held_keys().is_empty());
        assert!(m.held_buttons().is_empty());
    }

    #[test]
    fn a_heartbeat_clears_the_miss_count() {
        let mut m = ControlMachine::new();
        m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: false,
        });
        m.handle(ControlEvent::HeartbeatMiss);
        m.handle(ControlEvent::HeartbeatMiss);
        m.handle(ControlEvent::Heartbeat);
        assert_eq!(m.misses(), 0);
        assert!(m.handle(ControlEvent::HeartbeatMiss).is_empty());
        assert!(matches!(m.phase(), Phase::Remote { .. }));
    }

    #[test]
    fn disconnect_and_panic_both_return_home() {
        let mut m = ControlMachine::new();
        m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: false,
        });
        m.handle(ControlEvent::KeyDown(0x1B));
        let cmds = m.handle(ControlEvent::Disconnect { peer: peer("mac") });
        assert!(cmds.contains(&ControlCommand::ReleaseAll { peer: peer("mac") }));
        assert_eq!(m.phase(), &Phase::Local);

        m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: false,
        });
        let cmds = m.handle(ControlEvent::Panic);
        assert!(cmds.contains(&ControlCommand::ReleaseAll { peer: peer("mac") }));
        assert_eq!(m.phase(), &Phase::Local);

        let local_panic = m.handle(ControlEvent::Panic);
        assert_eq!(
            local_panic,
            vec![ControlCommand::ShowCursor, ControlCommand::StopForward]
        );
        assert!(m
            .handle(ControlEvent::Disconnect {
                peer: peer("other")
            })
            .is_empty());
    }

    #[test]
    fn crossing_home_cancels_a_drag_and_shows_the_cursor() {
        let mut m = ControlMachine::new();
        let cmds = m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: true,
        });
        assert!(cmds.contains(&ControlCommand::BeginDrag { peer: peer("mac") }));
        assert!(m.dragging());
        let cmds = m.handle(ControlEvent::CrossHome);
        assert!(cmds.contains(&ControlCommand::CancelDrag { peer: peer("mac") }));
        assert!(cmds.contains(&ControlCommand::ReleaseAll { peer: peer("mac") }));
        assert!(!m.dragging());
        assert_eq!(m.phase(), &Phase::Local);
    }

    #[test]
    fn button_up_finishes_the_drag_and_escape_cancels_it() {
        let mut m = ControlMachine::new();
        m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: true,
        });
        let cmds = m.handle(ControlEvent::ButtonUp(0));
        assert_eq!(cmds, vec![ControlCommand::FinishDrag { peer: peer("mac") }]);
        assert!(!m.dragging());
        assert!(matches!(m.phase(), Phase::Remote { .. }));

        m.handle(ControlEvent::CrossHome);
        m.handle(ControlEvent::CrossTo {
            peer: peer("mac"),
            button_held: true,
        });
        let cmds = m.handle(ControlEvent::CancelDrag);
        assert_eq!(cmds, vec![ControlCommand::CancelDrag { peer: peer("mac") }]);
        assert!(matches!(m.phase(), Phase::Remote { .. }));
        assert!(m.handle(ControlEvent::CancelDrag).is_empty());
    }
}
