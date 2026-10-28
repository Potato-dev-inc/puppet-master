//! Night Shift: a list of up to 10 tasks that run while the person sleeps, each in its own copy of
//! the project, on the plans the person picked. Invariants (spec section 4): the runner never answers
//! a trust, permission or login question; nothing is merged without the person; a stop never deletes
//! a copy; every stop time is required.

pub mod model;
pub mod plan;
