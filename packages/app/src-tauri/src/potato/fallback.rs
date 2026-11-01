//! The fallback brains of the talk brain. The talk brain is the first choice. The fallbacks are the brains to
//! try next, in order, when the brain in use says its model is out of balance or busy (`PROVIDER_LIMIT`) or is not
//! answering (`PROVIDER_DOWN`). Auth and internal errors do not switch: they need the person to act.
//!
//! This file only decides what comes next. The switch itself is in `service.rs`: the brain that failed is stopped,
//! the message it failed on goes back to the queue, and the next brain starts for it. The next brain is started with
//! its own environment (its own key, or its own sign-in), so no key is ever handed to a brain that does not use it.
//!
//! `agents` is never a fallback (it runs no brain process). A DeepSeek fallback needs the DeepSeek key, the same as the
//! main brain. Every new run starts on the main brain again.

use super::protocol::ErrorCode;
use super::store::BrainKind;

/// The fallbacks as they are kept: in the given order, without `agents`, without repeats and without the main brain.
pub fn clean(primary: BrainKind, list: &[BrainKind]) -> Vec<BrainKind> {
    let mut out: Vec<BrainKind> = Vec::new();
    for &kind in list {
        if kind != BrainKind::Agents && kind != primary && !out.contains(&kind) {
            out.push(kind);
        }
    }
    out
}

/// The brains a run may use, in order: the main brain, then the fallbacks that can run now (a DeepSeek one needs its key).
pub fn chain(primary: BrainKind, list: &[BrainKind], has_deepseek_key: bool) -> Vec<BrainKind> {
    let mut out = vec![primary];
    out.extend(clean(primary, list).into_iter().filter(|kind| *kind != BrainKind::Deepseek || has_deepseek_key));
    out
}

/// The step of the chain to switch to after `code`, or `None` when the error is not one a fallback answers
/// or there is no next brain. `step` is the place in the chain of the brain that failed.
pub fn next_step(chain_len: usize, step: usize, code: ErrorCode) -> Option<usize> {
    let answers = matches!(code, ErrorCode::ProviderLimit | ErrorCode::ProviderDown);
    (answers && step + 1 < chain_len).then_some(step + 1)
}
