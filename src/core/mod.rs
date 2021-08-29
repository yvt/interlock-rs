//! The core algorithm implementation to manage interval borrows and ensure
//! mutual exclusion.
use core::{ops::Range, pin::Pin};

pub mod rbtree;

/// A specialized readers-writer lock optimized for interval locks.
///
/// # Drop Validation
///
/// May panic if dropped while a lock is held. This property is important for
/// memory safety. An `*LockState` object, when holding a lock, contains a
/// pointer that points to the originating `IntervalRwLockCore` object. This is
/// crucial for making sure that the `*LockState` object is unlocked with the
/// correct instance of `IntervalRwLockCore`. But if the originating
/// `IntervalRwLockCore` object is dropped, a second `IntervalRwLockCore`
/// instance might be created later at the same location. The previously created
/// `*LockState` object  doesn't belong to this `IntervalRwLockCore` instance,
/// but since it has the location, it could be misidentified as the originating
/// `IntervalRwLockCore`. Panicking in the destructor prevents this issue.
/// `Self: !Unpin` is necessary for this mechanism to really work.
///
/// # Panics
///
/// This trait's methods may (but is not¹ guaranteed to) panic under the
/// following circumstances:
///
///  - `range` is empty or invalid (`start >= end`).
///  - `state` is already occupied (when locking) or associated with an
///    incorrect instance of `Self` (when unlocking).
///
/// ¹ But only to the extent that doesn't put soundness in jeopardy.
///
/// Panicking in user-provided trait methods may be escalated to aborting.
pub trait IntervalRwLockCore {
    /// The type used to represent interval endpoints.
    type Index;

    /// Governs the ordering between pending borrows at the same location.
    type Priority;

    /// The storage for per-lock data. Dropping it while it has an associated
    /// lock will cause a panic.
    type ReadLockState;

    /// Ditto for writer locks.
    type WriteLockState;

    /// Ditto for non-blocking reader locks.
    type TryReadLockState;

    /// Ditto for non-blocking writer locks.
    type TryWriteLockState;

    /// Created by a `LockCallback` implementation when a lock operation is
    /// blocked in the middle. It will later be passed to [`UnlockCallback::
    /// complete`] when the lock completes.
    type InProgress;

    /// Acquire a reader lock.
    fn lock_read<Callback: LockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::ReadLockState>,
        callback: Callback,
    ) -> Callback::Output;

    /// Acquire a writer lock.
    fn lock_write<Callback: LockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::WriteLockState>,
        callback: Callback,
    ) -> Callback::Output;

    /// Attempt to acquire a reader lock. (Non-blocking)
    fn try_lock_read(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryReadLockState>,
    ) -> bool;

    /// Attempt to acquire a writer lock. (Non-blocking)
    fn try_lock_write(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryWriteLockState>,
    ) -> bool;

    /// Release a reader lock.
    fn unlock_read<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::ReadLockState>,
        callback: Callback,
    ) -> Option<Self::InProgress>;

    /// Release a writer lock.
    fn unlock_write<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::WriteLockState>,
        callback: Callback,
    ) -> Option<Self::InProgress>;

    /// Release a non-blocking reader lock.
    fn unlock_try_read<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::TryReadLockState>,
        callback: Callback,
    );

    /// Release a non-blocking writer lock.
    fn unlock_try_write<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::TryWriteLockState>,
        callback: Callback,
    );
}

pub trait LockCallback<InProgress> {
    type Output;

    /// Called if the lock operation did not complete because of a conflicting
    /// lock. The returned `InProgress` will be used to notify the caller by
    /// [`UnlockCallback::complete`].
    fn in_progress(self) -> (Self::Output, InProgress);

    /// Indicates the lock operation's completion.
    fn complete(self) -> Self::Output;
}

pub trait UnlockCallback<InProgress> {
    /// An in-progress lock operation completed because of this unlock
    /// operation.
    fn complete(&mut self, in_progress: InProgress);
}

impl LockCallback<()> for () {
    type Output = ();
    fn in_progress(self) -> (Self::Output, ()) {
        ((), ())
    }
    fn complete(self) -> Self::Output {}
}

impl UnlockCallback<()> for () {
    fn complete(&mut self, _in_progress: ()) {}
}
