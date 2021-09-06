//! The thread-safe, `Future`-oriented implementation.
use core::{
    fmt,
    ops::Range,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use lock_api::RawMutex as RawMutexTrait;

use crate::{
    core::{rbtree, IntervalRwLockCore, LockCallback, LockState, UnlockCallback},
    raw::{RawAsyncIntervalRwLock, RawIntervalRwLock},
    utils::pinlock::PinMutex,
};

/// Wraps `IntervalRwLockCore` (an internal trait) to provide a thread-safe,
/// `Future`-oriented readers-writer lock optimized for interval locks with a
/// raw interface. `RawMutex: `[`lock_api::RawMutex`] is used to protect `Core`
/// from concurrent accesses.
#[pin_project::pin_project]
pub struct AsyncRawIntervalRwLock<RawMutex, Core> {
    #[pin]
    core: PinMutex<RawMutex, Core>,
}

impl<RawMutex: RawMutexTrait, Core: fmt::Debug> fmt::Debug
    for AsyncRawIntervalRwLock<RawMutex, Core>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.core.fmt(f)
    }
}

/// The structure that describes a pending borrow operation.
#[derive(Debug)]
pub struct InProgress {
    waker: Option<Waker>,
}

impl InProgress {
    #[inline]
    fn set_waker(&mut self, cx: &mut Context<'_>) {
        let new_waker = cx.waker();
        if let Some(old_waker) = &self.waker {
            if old_waker.will_wake(new_waker) {
                return;
            }
        }
        self.waker = Some(new_waker.clone());
    }
}

macro_rules! borrow_core {
    (let $p:pat = $self:ident.core) => {
        let this = $self.project_ref();
        let mut core = this.core.lock();
        let $p = Pin::as_mut(&mut core);
    };
}

unsafe impl<RawMutex: RawMutexTrait, Core: IntervalRwLockCore<InProgress = InProgress>>
    RawIntervalRwLock for AsyncRawIntervalRwLock<RawMutex, Core>
{
    type Index = Core::Index;

    type TryReadLockState = Core::TryReadLockState;

    type TryWriteLockState = Core::TryWriteLockState;

    const INIT: Self = Self {
        core: PinMutex::const_new(RawMutex::INIT, Core::INIT),
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

unsafe impl<RawMutex: RawMutexTrait, Core: IntervalRwLockCore<InProgress = InProgress>>
    RawAsyncIntervalRwLock for AsyncRawIntervalRwLock<RawMutex, Core>
{
    type ReadLockState = Core::ReadLockState;

    type WriteLockState = Core::WriteLockState;

    type Priority = Core::Priority;

    fn start_lock_read(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::ReadLockState>,
    ) {
        borrow_core!(let core = self.core);
        core.lock_read(range, priority, state, LockCallbackImpl {});
    }

    fn start_lock_write(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::WriteLockState>,
    ) {
        borrow_core!(let core = self.core);
        core.lock_write(range, priority, state, LockCallbackImpl {});
    }

    fn poll_lock_read(
        self: Pin<&Self>,
        state: Pin<&mut Self::ReadLockState>,
        cx: &mut Context<'_>,
    ) -> Poll<()> {
        borrow_core!(let core = self.core);
        match core.inspect_read_mut(state) {
            LockState::Complete => Poll::Ready(()),
            LockState::InProgress(in_progress) => {
                in_progress.set_waker(cx);
                Poll::Pending
            }
        }
    }

    fn poll_lock_write(
        self: Pin<&Self>,
        state: Pin<&mut Self::WriteLockState>,
        cx: &mut Context<'_>,
    ) -> Poll<()> {
        borrow_core!(let core = self.core);
        match core.inspect_write_mut(state) {
            LockState::Complete => Poll::Ready(()),
            LockState::InProgress(in_progress) => {
                in_progress.set_waker(cx);
                Poll::Pending
            }
        }
    }

    fn unlock_read(self: Pin<&Self>, state: Pin<&mut Self::ReadLockState>) {
        borrow_core!(let core = self.core);
        core.unlock_read(state, UnlockCallbackImpl {});
    }

    fn unlock_write(self: Pin<&Self>, state: Pin<&mut Self::WriteLockState>) {
        borrow_core!(let core = self.core);
        core.unlock_write(state, UnlockCallbackImpl {});
    }
}

struct LockCallbackImpl {}

struct UnlockCallbackImpl {}

impl LockCallback<InProgress> for LockCallbackImpl {
    type Output = ();

    #[inline]
    fn in_progress(self) -> (Self::Output, InProgress) {
        ((), InProgress { waker: None })
    }

    #[inline]
    fn complete(self) -> Self::Output {}
}

impl UnlockCallback<InProgress> for UnlockCallbackImpl {
    #[inline]
    fn complete(&mut self, in_progress: InProgress) {
        if let Some(waker) = in_progress.waker {
            waker.wake();
        }
    }
}

/// A raw interface to a thread-safe, `Future`-oriented readers-writer lock
/// optimized for interval locks, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type AsyncRawRbTreeIntervalRwLock<RawMutex, Index, Priority = ()> =
    AsyncRawIntervalRwLock<RawMutex, rbtree::RbTreeIntervalRwLockCore<Index, Priority, InProgress>>;
