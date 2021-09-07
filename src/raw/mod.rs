//! Raw synchronization primitives.
//!
//!  - [`RawIntervalRwLock`] provides a non-blocking (fallible) interface,
//!    similar to `std::sync::RwLock::try_write`.
//!
//!  - [`RawBlockingIntervalRwLock`] provides a blocking (infallible) interface,
//!    similar to `std::sync::RwLock::write`.
//!
//!  - [`RawAsyncIntervalRwLock`] provides a `Future`-based interface, something
//!    that could be used to build a high-level interface like
//!    `futures::lock::Mutex`.
//!
//! # Notes
//!
//!  - These traits' methods take `Pin<&Self>` as the receiver. This means that
//!    the trait implementations of client-provided types such as `Index` are
//!    not prevented to recursively call into these methods. This does not lead
//!    to an undefined behavior but may cause a panic or abort.
//!
use core::{
    ops::Range,
    pin::Pin,
    task::{Context, Poll},
};

#[cfg(feature = "async")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "async")))]
pub mod future; // not naming it `async` as it clashes with the keyword
pub mod local;
#[cfg(feature = "std")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "std")))]
pub mod sync;

/// A non-blocking interface to a specialized readers-writer lock optimized for
/// interval locks.
pub unsafe trait RawIntervalRwLock {
    /// The type used to represent interval endpoints.
    type Index;

    /// The storage for per-lock data. Dropping it while it has an associated
    /// lock will cause a panic.
    type TryReadLockState: Default;

    /// Ditto for non-blocking writer locks.
    type TryWriteLockState: Default;

    /// The initializer.
    const INIT: Self;

    /// Attempt to acquire a reader lock. (Non-blocking)
    fn try_lock_read(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryReadLockState>,
    ) -> bool;

    /// Attempt to acquire a writer lock. (Non-blocking)
    fn try_lock_write(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryWriteLockState>,
    ) -> bool;

    /// Release a non-blocking reader lock.
    fn unlock_try_read(self: Pin<&Self>, state: Pin<&mut Self::TryReadLockState>);

    /// Release a non-blocking writer lock.
    fn unlock_try_write(self: Pin<&Self>, state: Pin<&mut Self::TryWriteLockState>);
}

/// A blocking interface to a specialized readers-writer lock optimized for
/// interval locks.
pub unsafe trait RawBlockingIntervalRwLock: RawIntervalRwLock {
    /// The storage for per-lock data. Dropping it while it has an associated
    /// lock will cause a panic.
    type ReadLockState: Default;

    /// Ditto for writer locks.
    type WriteLockState: Default;

    /// Governs the ordering between pending borrows at the same location.
    type Priority;

    /// Acquire a reader lock, blocking the current thread until being able to
    /// do so.
    fn lock_read(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::ReadLockState>,
    );

    /// Acquire a writer lock, blocking the current thread until being able to
    /// do so.
    fn lock_write(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::WriteLockState>,
    );

    /// Release a reader lock.
    fn unlock_read(self: Pin<&Self>, state: Pin<&mut Self::ReadLockState>);

    /// Release a writer lock.
    fn unlock_write(self: Pin<&Self>, state: Pin<&mut Self::WriteLockState>);
}

/// A [`Future`]-based interface to a specialized readers-writer lock optimized
/// for interval locks.
///
/// [`Future`]: core::future::Future
pub unsafe trait RawAsyncIntervalRwLock: RawIntervalRwLock {
    /// The storage for per-lock data. Dropping it while it has an associated
    /// (possibly pending) lock will cause a panic.
    type ReadLockState: Default;

    /// Ditto for writer locks.
    type WriteLockState: Default;

    /// Governs the ordering between pending borrows at the same location.
    type Priority;

    /// Start acquiring a reader lock.
    ///
    /// This method associates `state` with a pending reader lock. It's an error
    /// to pass a `ReadLockState` that is already associated with a lock.
    fn start_lock_read(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::ReadLockState>,
    );

    /// Start acquiring a writer lock.
    ///
    /// See [`Self::start_lock_read`] for details.
    fn start_lock_write(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::WriteLockState>,
    );

    /// Poll the status of a possibly-pending reader lock.
    ///
    /// `state` must be associated with a possibly-pending lock. This method
    /// returns `Poll::Ready` if the lock is complete. Otherwise, it will
    /// return `Poll::Pending`, and `cx.waker()` will be used to notify the
    /// client when a progress can be made.
    fn poll_lock_read(
        self: Pin<&Self>,
        state: Pin<&mut Self::ReadLockState>,
        cx: &mut Context<'_>,
    ) -> Poll<()>;

    /// Poll the status of a possibly-pending writer lock.
    ///
    /// See [`Self::poll_lock_read`] for details.
    fn poll_lock_write(
        self: Pin<&Self>,
        state: Pin<&mut Self::WriteLockState>,
        cx: &mut Context<'_>,
    ) -> Poll<()>;

    /// Release a (possibly pending) reader lock.
    fn unlock_read(self: Pin<&Self>, state: Pin<&mut Self::ReadLockState>);

    /// Release a (possibly pending) writer lock.
    fn unlock_write(self: Pin<&Self>, state: Pin<&mut Self::WriteLockState>);
}
