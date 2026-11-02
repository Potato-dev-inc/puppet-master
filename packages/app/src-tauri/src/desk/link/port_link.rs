//! The link's side of `desk::port`: hotkeys on this computer ask for "jump to computer N" and "return home", and the
//! screen engine says whether the pointer is locked to a screen. Kept in its own file so `mod.rs` only holds the field.

use std::sync::{Arc, Weak};

use super::hub::Msg;
use super::{lock, DeskLink, Inner};
use crate::desk::port::{LinkAction, LinkPort};

impl Inner {
    pub(super) fn port(&self) -> Option<Arc<LinkPort>> {
        lock(&self.port).clone()
    }

    /// The person locked the pointer to one screen: it must not cross to another computer.
    pub(super) fn pointer_locked(&self) -> bool {
        self.port().is_some_and(|p| p.is_locked())
    }
}

impl DeskLink {
    /// Connect this link to the screen engine's port. The handler holds the link weakly, so it keeps nothing alive.
    pub fn attach_port(&self, port: Arc<LinkPort>) {
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        port.set_handler(Some(Arc::new(move |action| {
            let inner = weak.upgrade().ok_or_else(|| "My computers is stopped".to_string())?;
            if !inner.labs() {
                return Err(crate::desk::FEATURE_OFF.into());
            }
            let tx = inner.hub_tx().ok_or_else(|| "No other computer is connected".to_string())?;
            let msg = match action {
                LinkAction::Jump(n) => Msg::Jump(n),
                LinkAction::ReturnHome => Msg::ReturnHome,
            };
            tx.send(msg).map_err(|_| "My computers is stopped".to_string())
        })));
        *lock(&self.inner.port) = Some(port);
    }
}
