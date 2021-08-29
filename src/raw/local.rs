//! The thread-unsafe (but faster) implementation.
use core::{ops::Range, pin::Pin};
use guard::guard;
use pin_cell::PinCell;

use crate::{
    core::{rbtree, IntervalRwLockCore},
    raw::{RawBlockingIntervalRwLock, RawIntervalRwLock},
};

/// Wraps `IntervalRwLockCore` (an internal trait) to provide a non-thread-safe
/// readers-writer lock optimized for interval locks with a raw interface.
#[pin_project::pin_project]
pub struct LocalRawIntervalRwLock<Core> {
    #[pin]
    core: PinCell<Core>,
}

#[cold]
fn core_is_not_reentrant() -> ! {
    panic!("attempted to recursively call a method of `LocalRawIntervalRwLock`");
}

#[cold]
fn deadlocked() -> ! {
    panic!("deadlocked");
}

macro_rules! borrow_core {
	(let $p:pat = $self:ident.core) => {
		guard!(let Ok(mut core) = $self.project_ref().core.try_borrow_mut()
			else { core_is_not_reentrant(); });
		let $p = pin_cell::PinMut::as_mut(&mut core);
	};
}

impl<Core: IntervalRwLockCore<InProgress = !>> RawIntervalRwLock for LocalRawIntervalRwLock<Core> {
    type Index = Core::Index;

    type TryReadLockState = Core::TryReadLockState;

    type TryWriteLockState = Core::TryWriteLockState;

    const INIT: Self = Self {
        core: PinCell::new(Core::INIT),
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
        core.unlock_try_read(state, ())
    }

    fn unlock_try_write(self: Pin<&Self>, state: Pin<&mut Self::TryWriteLockState>) {
        borrow_core!(let core = self.core);
        core.unlock_try_write(state, ())
    }
}

impl<Core: IntervalRwLockCore<InProgress = !>> RawBlockingIntervalRwLock
    for LocalRawIntervalRwLock<Core>
{
    type ReadLockState = Core::TryReadLockState;

    type WriteLockState = Core::TryWriteLockState;

    type Priority = ();

    fn lock_read(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        _priority: Self::Priority,
        state: Pin<&mut Self::ReadLockState>,
    ) {
        if !self.try_lock_read(range, state) {
            deadlocked();
        }
    }

    fn lock_write(
        self: Pin<&Self>,
        range: Range<Self::Index>,
        _priority: Self::Priority,
        state: Pin<&mut Self::WriteLockState>,
    ) {
        if !self.try_lock_write(range, state) {
            deadlocked();
        }
    }

    fn unlock_read(self: Pin<&Self>, state: Pin<&mut Self::ReadLockState>) {
        self.unlock_try_read(state);
    }

    fn unlock_write(self: Pin<&Self>, state: Pin<&mut Self::WriteLockState>) {
        self.unlock_try_write(state);
    }
}

/// A raw interface to a non-thread-safe readers-writer lock optimized for
/// interval locks, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type LocalRawRbTreeIntervalRwLock<Index> =
    LocalRawIntervalRwLock<rbtree::RbTreeIntervalRwLockCore<Index, !, !>>;
