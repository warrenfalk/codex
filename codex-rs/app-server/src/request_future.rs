//! Construct large RPC futures outside the shared dispatcher's poll frame.

use std::future::Future;
use std::pin::Pin;

// Taking a constructor matters: boxing an already-created future still reserves
// stack space for that future's temporary in the calling poll frame.
#[inline(never)]
pub(crate) fn box_request_future<F: Future + Send>(create: impl FnOnce() -> F) -> Pin<Box<F>> {
    Box::pin(create())
}
