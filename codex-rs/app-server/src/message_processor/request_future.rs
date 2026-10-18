//! Keep large lifecycle request futures out of the shared dispatcher's poll frame.

use std::future::Future;
use std::pin::Pin;

// Passing a constructor keeps the unboxed temporary in this separate frame. Boxing
// an already constructed future at the await site still reserves its stack space.
#[inline(never)]
pub(super) fn boxed<F: Future>(make_future: impl FnOnce() -> F) -> Pin<Box<F>> {
    Box::pin(make_future())
}
