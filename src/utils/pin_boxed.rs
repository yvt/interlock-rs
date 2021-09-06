//! An alternate implementation of `./pin.rs` for the Miri interpreter, whose
//! Stacked Borrows memory model is [not adjusted][1] to support `async` blocks
//! yet.
//!
//! [1]: https://github.com/rust-lang/unsafe-code-guidelines/issues/148
use core::{fmt, marker::PhantomPinned, pin::Pin};

extern crate alloc;
use alloc::boxed::Box;

#[cfg(test)]
#[path = "pin/tests.rs"]
mod tests;

pub trait EarlyDrop {
    unsafe fn early_drop(self: Pin<&Self>) {}
}

/// See `pin.rs`
#[pin_project::pin_project]
pub struct EarlyDropGuard<T: EarlyDrop> {
    storage: Option<EarlyDroppingBox<T>>,
    _pinned: PhantomPinned,
}

unsafe impl<T: Send + EarlyDrop> Send for EarlyDropGuard<T> {}
unsafe impl<T: Sync + EarlyDrop> Sync for EarlyDropGuard<T> {}

impl<T: EarlyDrop> EarlyDropGuard<T> {
    /// Construct an `EarlyDropGuard`. The created `EarlyDropGuard` initially
    /// doesn't contain the inner object.
    pub const fn new() -> Self {
        Self {
            storage: None,
            _pinned: PhantomPinned,
        }
    }

    #[inline]
    pub fn get(&self) -> Option<Pin<&T>> {
        self.storage.as_ref().map(|storage| Pin::as_ref(&storage.0))
    }
}

impl<T: EarlyDrop> EarlyDropGuard<T> {
    /// Get a reference to the inner object, creating one with `init` if it
    /// hasn't been created yet.
    pub fn get_or_insert_with(self: Pin<&mut Self>, init: impl FnOnce() -> T) -> Pin<&T> {
        let this = self.project();
        let storage = this
            .storage
            .get_or_insert_with(|| EarlyDroppingBox(Box::pin(init())));
        Pin::as_ref(&storage.0)
    }

    /// Get a reference to the inner object, creating one with
    /// [`Default::default`] if it hasn't been created yet.
    pub fn get_or_insert_default(self: Pin<&mut Self>) -> Pin<&T>
    where
        T: Default,
    {
        self.get_or_insert_with(T::default)
    }
}

struct EarlyDroppingBox<T: EarlyDrop>(Pin<Box<T>>);

impl<T: EarlyDrop> Drop for EarlyDroppingBox<T> {
    #[inline]
    fn drop(&mut self) {
        unsafe { Pin::as_ref(&self.0).early_drop() };
    }
}

impl<T: fmt::Debug + EarlyDrop> fmt::Debug for EarlyDropGuard<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(inner) = self.get() {
            fmt::Debug::fmt(&inner, f)
        } else {
            f.write_str("< uninitialized >")
        }
    }
}
