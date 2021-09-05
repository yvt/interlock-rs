//! The thread-safe, blocking implementation that uses [`std`]'s interthread
//! synchronization API.
use std::{
    ops::Range,
    pin::Pin,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
    thread::Thread,
};

use crate::{
    core::{rbtree, IntervalRwLockCore, LockCallback, UnlockCallback},
    raw::{RawBlockingIntervalRwLock, RawIntervalRwLock},
    utils::pinsync::PinMutex,
};

/// Wraps `IntervalRwLockCore` (an internal trait) to provide a thread-safe,
/// blocking readers-writer lock optimized for interval locks with a raw
/// interface. This utilizes [`std`]'s API for inter-thread synchronization.
#[pin_project::pin_project]
#[derive(Debug)]
pub struct SyncRawIntervalRwLock<Core> {
    #[pin]
    core: PinMutex<Core>,
}

/// The structure that describes a pending borrow operation.
#[derive(Debug)]
pub struct InProgress {
    complete: NonNull<AtomicBool>,
    waiter: Thread,
}

unsafe impl Send for InProgress {}
unsafe impl Sync for InProgress {}

macro_rules! borrow_core {
    (let $p:pat = $self:ident.core) => {
        let mut core = $self.project_ref().core.lock();
        let $p = Pin::as_mut(&mut core);
    };
}

unsafe impl<Core: IntervalRwLockCore<InProgress = InProgress>> RawIntervalRwLock
    for SyncRawIntervalRwLock<Core>
{
    type Index = Core::Index;

    type TryReadLockState = Core::TryReadLockState;

    type TryWriteLockState = Core::TryWriteLockState;

    const INIT: Self = Self {
        core: PinMutex::new(Core::INIT),
    };

    fn try_lock_read(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryReadLockState>,
    ) -> bool {
        borrow_core!(let core = self.core);
        core.try_lock_read(range, state)
    }

    fn try_lock_write(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryWriteLockState>,
    ) -> bool {
        borrow_core!(let core = self.core);
        core.try_lock_write(range, state)
    }

    fn unlock_try_read(self: Pin<&Self>, state: Pin<&mut Self::TryReadLockState>) {
        borrow_core!(let core = self.core);
        core.unlock_try_read(state, UnlockCallbackImpl {})
    }

    fn unlock_try_write(self: Pin<&Self>, state: Pin<&mut Self::TryWriteLockState>) {
        borrow_core!(let core = self.core);
        core.unlock_try_write(state, UnlockCallbackImpl {})
    }
}

unsafe impl<Core: IntervalRwLockCore<InProgress = InProgress>> RawBlockingIntervalRwLock
    for SyncRawIntervalRwLock<Core>
{
    type ReadLockState = Core::ReadLockState;

    type WriteLockState = Core::WriteLockState;

    type Priority = Core::Priority;

    fn lock_read(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::ReadLockState>,
    ) {
        let complete = AtomicBool::new(false);

        {
            borrow_core!(let core = self.core);
            if core.lock_read(
                range,
                priority,
                state,
                LockCallbackImpl {
                    complete: NonNull::from(&complete),
                },
            ) {
                // The lock completed instantly
                return;
            }

            // Unlock the mutex before waiting
        }

        // Wait until we are woken up
        while !complete.load(Ordering::Relaxed) {
            std::thread::park();
        }
    }

    fn lock_write(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::WriteLockState>,
    ) {
        let complete = AtomicBool::new(false);

        {
            borrow_core!(let core = self.core);
            if core.lock_write(
                range,
                priority,
                state,
                LockCallbackImpl {
                    complete: NonNull::from(&complete),
                },
            ) {
                // The lock completed instantly
                return;
            }

            // Unlock the mutex before waiting
        }

        // Wait until we are woken up
        while !complete.load(Ordering::Relaxed) {
            std::thread::park();
        }
    }

    fn unlock_read(self: Pin<&Self>, state: Pin<&mut Self::ReadLockState>) {
        borrow_core!(let core = self.core);
        let in_progress = core.unlock_read(state, UnlockCallbackImpl {});
        debug_assert!(
            in_progress.is_none(),
            "unexpectedly cancelled an in-progress borrow"
        )
    }

    fn unlock_write(self: Pin<&Self>, state: Pin<&mut Self::WriteLockState>) {
        borrow_core!(let core = self.core);
        let in_progress = core.unlock_write(state, UnlockCallbackImpl {});
        debug_assert!(
            in_progress.is_none(),
            "unexpectedly cancelled an in-progress borrow"
        )
    }
}

struct LockCallbackImpl {
    complete: NonNull<AtomicBool>,
}

struct UnlockCallbackImpl {}

impl LockCallback<InProgress> for LockCallbackImpl {
    type Output = bool;

    #[inline]
    fn in_progress(self) -> (Self::Output, InProgress) {
        (
            false,
            InProgress {
                // Register the current thread as the waiter
                complete: self.complete,
                waiter: std::thread::current(),
            },
        )
    }

    #[inline]
    fn complete(self) -> Self::Output {
        true
    }
}

impl UnlockCallback<InProgress> for UnlockCallbackImpl {
    fn complete(&mut self, in_progress: InProgress) {
        let complete = unsafe { in_progress.complete.as_ref() };

        // Unblock the waiter
        complete.store(true, Ordering::Relaxed);
        in_progress.waiter.unpark();
    }
}

/// A raw interface to a non-thread-safe readers-writer lock optimized for
/// interval locks, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type SyncRawRbTreeIntervalRwLock<Index, Priority = ()> =
    SyncRawIntervalRwLock<rbtree::RbTreeIntervalRwLockCore<Index, Priority, InProgress>>;
