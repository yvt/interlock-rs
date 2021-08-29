//! RB-Tree-based implementation
use core::{
    cell::{Cell, UnsafeCell},
    cmp::Ordering,
    fmt,
    marker::PhantomPinned,
    ops::Range,
    pin::Pin,
    ptr::NonNull,
};
use guard::guard;

use super::{IntervalRwLockCore, LockCallback, UnlockCallback};
use crate::utils::{
    panicking::abort_on_unwind,
    pin::{EarlyDrop, EarlyDropGuard},
    rbtree,
};

#[cfg(test)]
mod tests;

// Data types
// -----------------------------------------------------------------------------

/// An implementation of [`IntervalRwLockCore`] based on red-black trees.
pub struct RbTreeIntervalRwLockCore<Index, Priority, InProgress> {
    reads: Option<NonNull<ReadNode<Index>>>,
    writes: Option<NonNull<WriteNode<Index>>>,
    pendings: Option<NonNull<PendingNode<Index, Priority, InProgress>>>,
    /// Ensures the destructor is called.
    _pin: PhantomPinned,
}

// Safety: They are semantically container
unsafe impl<Index: Send, Priority: Send, InProgress: Send> Send
    for RbTreeIntervalRwLockCore<Index, Priority, InProgress>
{
}

// Safety: It can do nothing with `&Self`. But we require `Send` just in case,
// following `std::mutex::Mutex`'s pattern.
unsafe impl<Index: Send, Priority: Send, InProgress: Send> Sync
    for RbTreeIntervalRwLockCore<Index, Priority, InProgress>
{
}

/// A node of [`RbTreeIntervalRwLockCore::reads`]. Represents an endpoint of an
/// immutable borrow.
///
///  - `(index, 1)`: A lower bound (inclusive)
///  - `(index, -1)`: An upper bound (exclusive)
///
type ReadNode<Index> = rbtree::Node<(Index, i8), isize>;

/// A node of [`RbTreeIntervalRwLockCore::writes`]. Represents a mutable borrow.
type WriteNode<Index> = rbtree::Node<Range<Index>, ()>;

/// A node of [`RbTreeIntervalRwLockCore::pendings`]. Represents a pending
/// borrow.
type PendingNode<Index, Priority, InProgress> =
    rbtree::Node<Pending<Index, Priority, InProgress>, ()>;

/// An pending borrow.
#[derive(Debug)]
struct Pending<Index, Priority, InProgress> {
    /// The next index range to borrow.
    range: Range<Index>,
    priority: Priority,
    /// A pointer to the [`ReadLockState`] or [`WriteLockState`] that contains
    /// `Self`.
    parent: LockStatePtr<Index, Priority, InProgress>,
    /// A user-provided object that will be returned when this pending borrow
    /// operation completes.
    in_progress: InProgress,
}

#[derive(Debug)]
enum LockStatePtr<Index, Priority, InProgress> {
    Read(NonNull<ReadLockStateInner<Index, Priority, InProgress>>),
    Write(NonNull<WriteLockStateInner<Index, Priority, InProgress>>),
}

impl<Index, Priority, InProgress> Clone for LockStatePtr<Index, Priority, InProgress> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<Index, Priority, InProgress> Copy for LockStatePtr<Index, Priority, InProgress> {}

macro_rules! impl_lock_state {
    ($(
        $( #[$meta:meta] )*
        pub struct $ty:ident { inner: $inner:ident }
    )*) => {$(
        $( #[$meta] )*

        #[pin_project::pin_project]
        pub struct $ty<Index, Priority, InProgress> {
            #[pin]
            inner: EarlyDropGuard<$inner<Index, Priority, InProgress>>
        }

        impl<Index, Priority, InProgress> $ty<Index, Priority, InProgress> {
            #[inline]
            pub const fn new() -> Self {
                Self { inner: EarlyDropGuard::new() }
            }

            #[inline]
            fn get(self: Pin<&mut Self>) -> Pin<&$inner<Index, Priority, InProgress>> {
                self.project().inner.get_or_insert_default()
            }
        }

        impl<Index, Priority, InProgress> fmt::Debug for $ty<Index, Priority, InProgress> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                // We might be tempted to debug-format the contained nodes, but
                // we should because `Index`, `Priority`, and `InProgress` might
                // not be designed to be accessed in a different thread. We can
                // only show the parent's address.
                f.write_str(stringify!($ty))?;
                if let Some(parent) = self.inner.get().and_then(|inner|inner.parent.get()) {
                    write!(f, "(< borrow data for {:p} >)", parent)
                } else {
                    f.write_str("(< empty >)")
                }
            }
        }

        impl<Index, Priority, InProgress> Default for $ty<Index, Priority, InProgress> {
            #[inline]
            fn default() -> Self {
                Self::new()
            }
        }

        // Safety: `$ty` is just a vessel to contain the data associated with
        // locks. Its content is only accessed by `RbTreeIntervalRwLockCore`'s
        // methods, to which `$ty` is only passed as `Pin<&mut $ty>`. The
        // destructor isn't allowed to run if the slots for any of these generic
        // parameter types are filled.
        unsafe impl<Index, Priority, InProgress> Send for $ty<Index, Priority, InProgress> {}
        unsafe impl<Index, Priority, InProgress> Sync for $ty<Index, Priority, InProgress> {}
    )*};
}

impl_lock_state! {
    /// Provides a storage to store information about a blocking reader lock in
    /// [`RbTreeIntervalRwLockCore`].
    pub struct ReadLockState { inner: ReadLockStateInner }

    /// Provides a storage to store information about a blocking writer lock in
    /// [`RbTreeIntervalRwLockCore`].
    pub struct WriteLockState { inner: WriteLockStateInner }
    /// Provides a storage to store information about a non-blocking reader lock in
    /// [`RbTreeIntervalRwLockCore`].
    pub struct TryReadLockState { inner: TryReadLockStateInner }
    /// Provides a storage to store information about a non-blocking writer lock in
    /// [`RbTreeIntervalRwLockCore`].
    pub struct TryWriteLockState { inner: TryWriteLockStateInner }
}

struct ReadLockStateInner<Index, Priority, InProgress> {
    // | State                       | `parent` | `read` | `pending` |
    // | --------------------------- | -------- | ------ | --------- |
    // | Not in use                  | `None`   | `None` | `None`    |
    // | Pending, no borrowed region | `Some`   | `None` | `Some`    |
    // | Pending, partial borrow     | `Some`   | `Some` | `Some`    |
    // | Complete                    | `Some`   | `Some` | `None`    |
    /// The `RbTreeIntervalRwLockCore` `self` is currently used with. This is
    /// set when a lock is associated with `self` and cleared when the lock is
    /// released.
    ///
    /// In a method that takes `self: &mut RbTreeIntervalRwLockCore`, `parent
    /// == self` usually means it's safe to mutably borrow this struct's
    /// `UnsafeCell`s.
    parent: Cell<Option<NonNull<RbTreeIntervalRwLockCore<Index, Priority, InProgress>>>>,
    /// Stores a linked `PendingNode` if the borrow is incomplete.
    pending: UnsafeCell<Option<PendingNode<Index, Priority, InProgress>>>,
    /// Stores linked `ReadNode`s if the borrowed region is non-empty.
    read: UnsafeCell<Option<[ReadNode<Index>; 2]>>,
}

struct WriteLockStateInner<Index, Priority, InProgress> {
    /// See [`ReadLockStateInner::parent`]
    parent: Cell<Option<NonNull<RbTreeIntervalRwLockCore<Index, Priority, InProgress>>>>,
    /// See [`ReadLockStateInner::pending`]
    pending: UnsafeCell<Option<PendingNode<Index, Priority, InProgress>>>,
    /// Stores a linked `WriteNode` if the borrowed region is non-empty.
    write: UnsafeCell<Option<WriteNode<Index>>>,
}

struct TryReadLockStateInner<Index, Priority, InProgress> {
    /// See [`ReadLockStateInner::parent`]
    parent: Cell<Option<NonNull<RbTreeIntervalRwLockCore<Index, Priority, InProgress>>>>,
    /// See [`ReadLockStateInner::read`]
    read: UnsafeCell<Option<[ReadNode<Index>; 2]>>,
}

struct TryWriteLockStateInner<Index, Priority, InProgress> {
    /// See [`ReadLockStateInner::parent`]
    parent: Cell<Option<NonNull<RbTreeIntervalRwLockCore<Index, Priority, InProgress>>>>,
    /// See [`WriteLockStateInner::write`]
    write: UnsafeCell<Option<WriteNode<Index>>>,
}

impl<Index, Priority, InProgress> Default for ReadLockStateInner<Index, Priority, InProgress> {
    #[inline]
    fn default() -> Self {
        Self {
            parent: Cell::new(None),
            read: UnsafeCell::new(None),
            pending: UnsafeCell::new(None),
        }
    }
}

impl<Index, Priority, InProgress> Default for WriteLockStateInner<Index, Priority, InProgress> {
    #[inline]
    fn default() -> Self {
        Self {
            parent: Cell::new(None),
            write: UnsafeCell::new(None),
            pending: UnsafeCell::new(None),
        }
    }
}

impl<Index, Priority, InProgress> Default for TryReadLockStateInner<Index, Priority, InProgress> {
    #[inline]
    fn default() -> Self {
        Self {
            parent: Cell::new(None),
            read: UnsafeCell::new(None),
        }
    }
}

impl<Index, Priority, InProgress> Default for TryWriteLockStateInner<Index, Priority, InProgress> {
    #[inline]
    fn default() -> Self {
        Self {
            parent: Cell::new(None),
            write: UnsafeCell::new(None),
        }
    }
}

// RB tree callbacks
// -----------------------------------------------------------------------------

struct ReadNodeCallback;

impl<Index: Ord> rbtree::Callback<(Index, i8), isize> for ReadNodeCallback {
    // The summary of nodes is the sum of their `element.1` values
    #[inline]
    fn zero_summary(&mut self) -> isize {
        0
    }
    #[inline]
    fn element_to_summary(&mut self, element: &(Index, i8)) -> isize {
        element.1 as isize
    }
    #[inline]
    fn add_assign_summary(&mut self, lhs: &mut isize, rhs: &isize) {
        *lhs = lhs.wrapping_add(*rhs);
    }
    #[inline]
    fn sub_assign_summary(&mut self, lhs: &mut isize, rhs: &isize) {
        *lhs = lhs.wrapping_sub(*rhs);
    }
    #[inline]
    fn cmp_element(&mut self, e1: &(Index, i8), e2: &(Index, i8)) -> Ordering {
        e1.0.cmp(&e2.0)
    }
}

struct WriteNodeCallback;

impl<Index: Ord> rbtree::Callback<Range<Index>, ()> for WriteNodeCallback {
    #[inline]
    fn zero_summary(&mut self) {}
    #[inline]
    fn element_to_summary(&mut self, _element: &Range<Index>) {}
    #[inline]
    fn add_assign_summary(&mut self, _lhs: &mut (), _rhs: &()) {}
    #[inline]
    fn sub_assign_summary(&mut self, _lhs: &mut (), _rhs: &()) {}
    #[inline]
    fn cmp_element(&mut self, e1: &Range<Index>, e2: &Range<Index>) -> Ordering {
        // Since write borrows have no overlaps, either `start` and `end` can be
        // used for ordering
        e1.start.cmp(&e2.start)
    }
}

struct PendingNodeCallback;

impl<Index: Ord, Priority: Ord, InProgress>
    rbtree::Callback<Pending<Index, Priority, InProgress>, ()> for PendingNodeCallback
{
    #[inline]
    fn zero_summary(&mut self) {}
    #[inline]
    fn element_to_summary(&mut self, _element: &Pending<Index, Priority, InProgress>) {}
    #[inline]
    fn add_assign_summary(&mut self, _lhs: &mut (), _rhs: &()) {}
    #[inline]
    fn sub_assign_summary(&mut self, _lhs: &mut (), _rhs: &()) {}
    #[inline]
    fn cmp_element(
        &mut self,
        e1: &Pending<Index, Priority, InProgress>,
        e2: &Pending<Index, Priority, InProgress>,
    ) -> Ordering {
        (&e1.range.start, &e1.priority).cmp(&(&e2.range.start, &e2.priority))
    }
}

// Implementation
// -----------------------------------------------------------------------------

impl<Index, Priority, InProgress> RbTreeIntervalRwLockCore<Index, Priority, InProgress> {
    #[inline]
    pub const fn new() -> Self {
        Self {
            reads: None,
            writes: None,
            pendings: None,
            _pin: PhantomPinned,
        }
    }
}

impl<Index, Priority, InProgress> IntervalRwLockCore
    for RbTreeIntervalRwLockCore<Index, Priority, InProgress>
where
    Index: Clone + Ord,
    Priority: Ord,
{
    type Index = Index;
    type Priority = Priority;
    type ReadLockState = ReadLockState<Index, Priority, InProgress>;
    type WriteLockState = WriteLockState<Index, Priority, InProgress>;
    type TryReadLockState = TryReadLockState<Index, Priority, InProgress>;
    type TryWriteLockState = TryWriteLockState<Index, Priority, InProgress>;
    type InProgress = InProgress;

    fn lock_read<Callback: LockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::ReadLockState>,
        callback: Callback,
    ) -> Callback::Output {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        debug_assert!(range.start < range.end, "invalid range");

        // Check for conflicting borrows.
        let (output, pending) =
            if let Some(first_conflict_index) = this.first_write_borrow_in_range(&range) {
                let (output, in_progress) = callback.in_progress();
                (
                    output,
                    Some(Pending {
                        range: first_conflict_index..range.end.clone(),
                        priority,
                        parent: LockStatePtr::Read((&*state).into()),
                        in_progress,
                    }),
                )
            } else {
                drop(priority);
                (callback.complete(), None)
            };

        // The about-to-be-successfully-borrowed region
        let borrow_range = if let Some(pending) = &pending {
            (pending.range.start != range.start).then(|| range.start..pending.range.start.clone())
        } else {
            Some(range)
        };

        // Claim `state`. Do this before accessing other fields of `state`
        // because external trait implementations may use `state` with other
        // instances of `RbTreeIntervalRwLockCore`.
        if state.parent.get().is_some() {
            panic_lock_state_in_use();
        }
        state.parent.set(Some(NonNull::from(&*this)));

        abort_on_unwind(|| {
            // The rest steps are indivisible - we must complete, rollback, or
            // abort the program.

            // Borrow the region that we can borrow now
            if let Some(borrow_range) = borrow_range {
                // Initialize the read borrow nodes
                let read_nodes: [NonNull<_>; 2] = {
                    // Safety: Since `state` is still claimed by `self`, we have
                    // exclusive access to the contained `UnsafeCell`s.
                    let read_nodes = unsafe { &mut *state.read.get() };
                    debug_assert!(read_nodes.is_none());
                    let [n0, n1] = read_nodes.insert([
                        rbtree::Node::new((borrow_range.start, 1), 1),
                        rbtree::Node::new((borrow_range.end, -1), -1),
                    ]);
                    [n0.into(), n1.into()]
                };

                // Insert the read borrow nodes
                // Safety: The tree is sound, these nodes are new, and there are no
                //         conflicting borrows of the existing nodes.
                unsafe {
                    rbtree::Node::insert(ReadNodeCallback, &mut this.reads, read_nodes[0]);
                    rbtree::Node::insert(ReadNodeCallback, &mut this.reads, read_nodes[1]);
                };
            }

            // Remember the pending borrow
            if let Some(pending) = pending {
                // Initialize the pending borrow node
                let pending_node: NonNull<_> = {
                    // Safety: Since `state` is still claimed by `self`, we have
                    // exclusive access to the contained `UnsafeCell`s.
                    let pending_node = unsafe { &mut *state.pending.get() };
                    debug_assert!(pending_node.is_none());
                    pending_node.insert(rbtree::Node::new(pending, ())).into()
                };

                // Insert the pending borrow node
                // Safety: The tree is sound, this nodes is new, and there are no
                //         conflicting borrows of the existing nodes.
                unsafe {
                    rbtree::Node::insert(PendingNodeCallback, &mut this.pendings, pending_node)
                };
            }

            true
        });

        output
    }

    fn lock_write<Callback: LockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        priority: Self::Priority,
        state: Pin<&mut Self::WriteLockState>,
        callback: Callback,
    ) -> Callback::Output {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        debug_assert!(range.start < range.end, "invalid range");

        // Check for conflicting borrows.
        let first_conflict_index = match (
            this.first_write_borrow_in_range(&range),
            this.first_read_borrow_in_range(&range),
        ) {
            (None, None) => None,
            (one @ Some(_), None) | (None, one @ Some(_)) => one,
            (Some(i0), Some(i1)) => Some(i0.min(i1)),
        };

        // Check for conflicting borrows.
        let (output, pending) = if let Some(first_conflict_index) = first_conflict_index {
            let (output, in_progress) = callback.in_progress();
            (
                output,
                Some(Pending {
                    range: first_conflict_index..range.end.clone(),
                    priority,
                    parent: LockStatePtr::Write((&*state).into()),
                    in_progress,
                }),
            )
        } else {
            drop(priority);
            (callback.complete(), None)
        };

        // The about-to-be-successfully-borrowed region
        let borrow_range = if let Some(pending) = &pending {
            (pending.range.start != range.start).then(|| range.start..pending.range.start.clone())
        } else {
            Some(range)
        };

        // Claim `state`. Do this before accessing other fields of `state`
        // because external trait implementations may use `state` with other
        // instances of `RbTreeIntervalRwLockCore`.
        if state.parent.get().is_some() {
            panic_lock_state_in_use();
        }
        state.parent.set(Some(NonNull::from(&*this)));

        abort_on_unwind(|| {
            // The rest steps are indivisible - we must complete, rollback, or
            // abort the program.

            if let Some(borrow_range) = borrow_range {
                // Initialize the write borrow node
                let write_node: NonNull<_> = {
                    // Safety: Since `state` is still claimed by `self`, we have
                    // exclusive access to the contained `UnsafeCell`s.
                    let write_node = unsafe { &mut *state.write.get() };
                    debug_assert!(write_node.is_none());
                    write_node
                        .insert(rbtree::Node::new(borrow_range, ()))
                        .into()
                };

                // Insert the write borrow node
                // Safety: The tree is sound, this nodes is new, and there are no
                //         conflicting borrows of the existing nodes.
                unsafe { rbtree::Node::insert(WriteNodeCallback, &mut this.writes, write_node) };
            }

            // Remember the pending borrow
            if let Some(pending) = pending {
                // Initialize the pending borrow node
                let pending_node: NonNull<_> = {
                    // Safety: Since `state` is still claimed by `self`, we have
                    // exclusive access to the contained `UnsafeCell`s.
                    let pending_node = unsafe { &mut *state.pending.get() };
                    debug_assert!(pending_node.is_none());
                    pending_node.insert(rbtree::Node::new(pending, ())).into()
                };

                // Insert the pending borrow node
                // Safety: The tree is sound, this nodes is new, and there are no
                //         conflicting borrows of the existing nodes.
                unsafe {
                    rbtree::Node::insert(PendingNodeCallback, &mut this.pendings, pending_node)
                };
            }

            output
        })
    }

    fn try_lock_read(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryReadLockState>,
    ) -> bool {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        debug_assert!(range.start < range.end, "invalid range");

        // Check for conflicting borrows.
        if this.first_write_borrow_in_range(&range).is_some() {
            // Failure - write borrow conflict
            return false;
        }

        // Claim `state`. Do this before accessing other fields of `state`
        // because external trait implementations may use `state` with other
        // instances of `RbTreeIntervalRwLockCore`.
        if state.parent.get().is_some() {
            panic_lock_state_in_use();
        }
        state.parent.set(Some(NonNull::from(&*this)));

        abort_on_unwind(|| {
            // The rest steps are indivisible - we must complete, rollback, or
            // abort the program.

            // Initialize the read borrow nodes
            let read_nodes: [NonNull<_>; 2] = {
                // Safety: Since `state` is still claimed by `self`, we have
                // exclusive access to the contained `UnsafeCell`s.
                let read_nodes = unsafe { &mut *state.read.get() };
                debug_assert!(read_nodes.is_none());
                let [n0, n1] = read_nodes.insert([
                    ReadNode::new((range.start, 1), 1),
                    ReadNode::new((range.end, -1), -1),
                ]);
                [n0.into(), n1.into()]
            };

            // Insert the read borrow nodes
            // Safety: The tree is sound, these nodes are new, and there are no
            //         conflicting borrows of the existing nodes.
            unsafe {
                rbtree::Node::insert(ReadNodeCallback, &mut this.reads, read_nodes[0]);
                rbtree::Node::insert(ReadNodeCallback, &mut this.reads, read_nodes[1]);
            };

            true
        })
    }

    fn try_lock_write(
        self: Pin<&mut Self>,
        range: Range<Self::Index>,
        state: Pin<&mut Self::TryWriteLockState>,
    ) -> bool {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        debug_assert!(range.start < range.end, "invalid range");

        // Check for conflicting borrows.
        if this.first_write_borrow_in_range(&range).is_some()
            || this.first_read_borrow_in_range(&range).is_some()
        {
            // Failure - borrow conflict
            return false;
        }

        // Claim `state`. Do this before accessing other fields of `state`
        // because external trait implementations may use `state` with other
        // instances of `RbTreeIntervalRwLockCore`.
        if state.parent.get().is_some() {
            panic_lock_state_in_use();
        }
        state.parent.set(Some(NonNull::from(&*this)));

        abort_on_unwind(|| {
            // The rest steps are indivisible - we must complete, rollback, or
            // abort the program.

            // Initialize the write borrow node
            let write_node: NonNull<_> = {
                // Safety: Since `state` is still claimed by `self`, we have
                // exclusive access to the contained `UnsafeCell`s.
                let write_node = unsafe { &mut *state.write.get() };
                debug_assert!(write_node.is_none());
                NonNull::from(write_node.insert(WriteNode::new(range, ())))
            };

            // Insert the write borrow node
            // Safety: The tree is sound, this nodes is new, and there are no
            //         conflicting borrows of the existing nodes.
            unsafe { rbtree::Node::insert(WriteNodeCallback, &mut this.writes, write_node) };

            true
        })
    }

    fn unlock_read<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::ReadLockState>,
        callback: Callback,
    ) -> Option<Self::InProgress> {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        // Check `state`'s ownership before touching its `UnsafeCell`s
        if state.parent.get() != Some(NonNull::from(&*this)) {
            panic_lock_state_incorrect_parent();
        }

        let removed_pending_node = abort_on_unwind(|| {
            // If `state` contains a pending borrow, cancel it by removing it
            // from `self.pendings`.
            //
            // Safety: Since `state` is still claimed by `self`, we have
            // exclusive access to the contained `UnsafeCell`s.
            let removed_pending_node =
                unsafe { this.cancel_borrow(NonNull::new(state.pending.get()).unwrap()) };

            // Unborrow the (already-) borrowed region
            unsafe { this.unlock_read_inner(NonNull::new(state.read.get()).unwrap(), callback) };

            removed_pending_node
        });

        // Unclaim `state`
        state.parent.set(None);

        removed_pending_node.map(|node| node.element.in_progress)
    }

    fn unlock_write<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::WriteLockState>,
        callback: Callback,
    ) -> Option<Self::InProgress> {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        // Check `state`'s ownership before touching its `UnsafeCell`s
        if state.parent.get() != Some(NonNull::from(&*this)) {
            panic_lock_state_incorrect_parent();
        }

        let removed_pending_node = abort_on_unwind(|| {
            // If `state` contains a pending borrow, cancel it by removing it
            // from `self.pendings`.
            //
            // Safety: Since `state` is still claimed by `self`, we have
            // exclusive access to the contained `UnsafeCell`s.
            let removed_pending_node =
                unsafe { this.cancel_borrow(NonNull::new(state.pending.get()).unwrap()) };

            // Unborrow the (already-) borrowed region
            unsafe { this.unlock_write_inner(NonNull::new(state.write.get()).unwrap(), callback) };

            removed_pending_node
        });

        // Unclaim `state`
        state.parent.set(None);

        removed_pending_node.map(|node| node.element.in_progress)
    }

    /// Release a non-blocking reader lock.
    fn unlock_try_read<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::TryReadLockState>,
        callback: Callback,
    ) {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        // Check `state`'s ownership before touching its `UnsafeCell`s
        if state.parent.get() != Some(NonNull::from(&*this)) {
            panic_lock_state_incorrect_parent();
        }

        abort_on_unwind(|| {
            // Unborrow the borrowed region
            unsafe { this.unlock_read_inner(NonNull::new(state.read.get()).unwrap(), callback) };
        });

        // Unclaim `state`
        state.parent.set(None);
    }

    /// Release a non-blocking writer lock.
    fn unlock_try_write<Callback: UnlockCallback<Self::InProgress>>(
        self: Pin<&mut Self>,
        state: Pin<&mut Self::TryWriteLockState>,
        callback: Callback,
    ) {
        // Safety: We won't violate the pinning invariant
        let this = unsafe { self.get_unchecked_mut() };

        let state = state.get();

        // Check `state`'s ownership before touching its `UnsafeCell`s
        if state.parent.get() != Some(NonNull::from(&*this)) {
            panic_lock_state_incorrect_parent();
        }

        abort_on_unwind(|| {
            // Unborrow the borrowed region
            unsafe { this.unlock_write_inner(NonNull::new(state.write.get()).unwrap(), callback) };
        });

        // Unclaim `state`
        state.parent.set(None);
    }
}

impl<Index, Priority, InProgress> RbTreeIntervalRwLockCore<Index, Priority, InProgress>
where
    Index: Clone + Ord,
    Priority: Ord,
{
    /// Find the lowest mutably borrowed index in the specified range.
    fn first_write_borrow_in_range(&self, range: &Range<Index>) -> Option<Index> {
        // Find the first writing borrow whose end address is > range.start.
        let write_node = unsafe {
            rbtree::Node::lower_bound(&self.writes, |e| {
                range.start.cmp(&e.end).then(Ordering::Greater)
            })
            .map(|node| node.as_ref())
        };

        // ... and whose start address if < range.end.
        write_node
            .filter(|node| node.element.start < range.end)
            .map(|node| (&node.element.start).max(&range.start).clone())
    }

    /// Find the lowest immutably borrowed index in the specified range.
    fn first_read_borrow_in_range(&self, range: &Range<Index>) -> Option<Index> {
        // Find the first reading borrow endpoint whose
        // address is > range.start.
        //
        // Safety: The tree is sound, and there are no conflicting borrows of
        // the existing nodes.
        let read_node = unsafe {
            rbtree::Node::lower_bound(&self.reads, |e| {
                range.start.cmp(&e.0).then(Ordering::Greater)
            })
            .map(|node| node.as_ref())
        };
        if let Some(read_node) = read_node {
            debug_assert!(read_node.element.0 > range.start);

            // The summary of read nodes in the range `..=range.start`, i.e.,
            // the number of read borrows that overlap with `range.start..
            // range.start + ε`.
            //
            // Safety: The tree is sound, there are no conflicting borrows of
            // the existing nodes, and `read_node` is included in the tree.
            let num_reads_at_start = unsafe {
                rbtree::Node::prefix_sum(ReadNodeCallback, &self.reads, Some(read_node.into()))
            };

            if num_reads_at_start > 0 {
                // There's at least one read borrow in `range.start..range.start
                // + ε`,
                return Some(range.start.clone());
            }

            // Is there a read borrow that starts in the range `range.start + ε
            // ..range.end`?
            let read_starts_in_middle = read_node.element.0 < range.end;

            // the first node in `range.start + ε..` must be a lower bound of
            // some read borrow range
            debug_assert_eq!(
                read_node.element.1, 1,
                "found a read borrow upper bound point without a matching lower \
                bound point"
            );

            if read_starts_in_middle {
                // A read borrow starts here
                return Some(read_node.element.0.clone());
            }
        } else {
            // There are no read borrows in the range `range.start + ε..`.
        }
        None
    }

    /// Cancel a pending borrow. This is an internal function that assumes the
    /// following:
    ///
    ///  - Called inside `abort_on_unwind`. (This method is not unwind safe).
    ///  - If `pending_node` is `Some(_)`, it contains a node that is included
    ///    in the tree `self.pendings`.
    ///  - Neither `*pending_node` nor any of `self.pendings`'s nodes are
    ///    currently borrowed.
    ///
    unsafe fn cancel_borrow(
        &mut self,
        mut pending_node: NonNull<Option<PendingNode<Index, Priority, InProgress>>>,
    ) -> Option<PendingNode<Index, Priority, InProgress>> {
        // If `pending_node` contains a node, remove it from `self.pendings`.
        // First, do the conversion `NonNull<Option<T>>` → `Option<NonNull<T>>`.
        // If the result is `None`, return early.
        //
        // Safety: `*pending_node` is safe to borrow
        let pending_node_ptr = unsafe { option_as_ptr(pending_node)? };

        // Safety: The tree is sound, this nodes is new, and there are no
        //         conflicting borrows of the existing nodes.
        unsafe { rbtree::Node::remove(PendingNodeCallback, &mut self.pendings, pending_node_ptr) };

        // Move the node out of `state.pending` for safer deletion
        //
        // Safety: There are no conflicting borrows of `*pending_node`
        unsafe { pending_node.as_mut() }.take()
    }

    /// Release a reader lock. This is an internal function that assumes the
    /// following:
    ///
    ///  - Called inside `abort_on_unwind`. (This method is not unwind safe).
    ///  - If `read_nodes` contains nodes, they are included in the tree
    ///    `self.reads`.
    ///  - The pending borrow corresponding to `read_nodes` is absent from
    ///    `self.pendings`.
    ///  - Neither `*read_nodes` nor any of `self`'s trees' nodes are
    ///    currently borrowed.
    ///
    unsafe fn unlock_read_inner<Callback: UnlockCallback<InProgress>>(
        &mut self,
        mut read_nodes_ptr: NonNull<Option<[ReadNode<Index>; 2]>>,
        mut callback: Callback,
    ) {
        // First, do the conversion `NonNull<Option<[T; 2]>>` →
        // `Option<[NonNull<T>; 2]>`. If the result is `None`, return early.
        //
        // Safety: `*read_nodes` is safe to borrow
        guard!(let Some(mut read_nodes) = unsafe { option_array2_as_ptr(read_nodes_ptr) }
            else { return; });

        // The range we are about to unlock
        // Safety: It's still safe to borrow
        let [start, mut end] = unsafe { read_nodes.map(|n| n.as_ref().element.0.clone()) };

        // Look for pending borrows that can be resumed. Find the maximum node
        // that is < (end, -∞)
        //
        // Safety: The tree is sound, this nodes is new, and there are no
        //         conflicting borrows of the existing nodes.
        let mut maybe_pending_node = unsafe {
            if let Some(node) =
                rbtree::Node::lower_bound(&self.pendings, |e| end.cmp(&e.range.start))
            {
                rbtree::Node::predecessor(node)
            } else {
                self.pendings.map(|root| rbtree::Node::max(root))
            }
            .filter(|node| node.as_ref().element.range.start >= start)
        };

        while let Some(pending_node) = maybe_pending_node {
            // Find the next pending borrow before removing this one
            //
            // Safety: `pending_node` is in the tree and safe to borrow. Note
            // that `resume_borrow` only removes the provided node.
            let next_maybe_pending_node = unsafe {
                rbtree::Node::predecessor(pending_node)
                    .filter(|node| node.as_ref().element.range.start >= start)
            };

            // Unborrow `index..end` before resuming this pending borrow
            let index = unsafe { pending_node.as_ref().element.range.start.clone() };
            debug_assert!((&start..=&end).contains(&&index));

            let complete = if index == end {
                // Although we searched for pending nodes that are `< end`,
                // since `end` is moved to the pending node's index on each
                // iteration, we might come here if there are multiple pending
                // nodes at the same index.
                false
            } else {
                unsafe { rbtree::Node::remove(ReadNodeCallback, &mut self.reads, read_nodes[1]) };
                if index == start {
                    // The unborrowing is complete.
                    unsafe {
                        rbtree::Node::remove(ReadNodeCallback, &mut self.reads, read_nodes[0]);
                        *read_nodes_ptr.as_mut() = None;
                    }
                    true
                } else {
                    // The unborrowing is incomplete. Truncate the borrow to
                    // `start..index`.
                    unsafe {
                        read_nodes[1].as_mut().element.0 = index.clone();
                        read_nodes[1].as_mut().summary = -1;
                        rbtree::Node::insert(ReadNodeCallback, &mut self.reads, read_nodes[1]);
                    }
                    end = index;
                    false
                }
            };

            // Resume this pending borrow.
            unsafe { self.resume_borrow(pending_node, &mut callback) };

            if complete {
                return;
            }

            maybe_pending_node = next_maybe_pending_node;
        } // while

        // Unborrow `start..end` completely
        unsafe {
            rbtree::Node::remove(ReadNodeCallback, &mut self.reads, read_nodes[0]);
            rbtree::Node::remove(ReadNodeCallback, &mut self.reads, read_nodes[1]);
            *read_nodes_ptr.as_mut() = None;
        }
    }

    /// Release a writer lock. This is an internal function that assumes the
    /// following:
    ///
    ///  - Called inside `abort_on_unwind`. (This method is not unwind safe).
    ///  - If `write_node` contains a node, it is included in the tree
    ///    `self.writes`.
    ///  - The pending borrow corresponding to `write_node` is absent from
    ///    `self.pendings`.
    ///  - Neither `*write_node` nor any of `self`'s trees' nodes are
    ///    currently borrowed.
    ///
    unsafe fn unlock_write_inner<Callback: UnlockCallback<InProgress>>(
        &mut self,
        mut write_node_ptr: NonNull<Option<WriteNode<Index>>>,
        mut callback: Callback,
    ) {
        // First, do the conversion `NonNull<Option<T>>` → `Option<NonNull<T>>`.
        // If the result is `None`, return early.
        //
        // Safety: `*pending_node` is safe to borrow
        guard!(let Some(mut write_node) = unsafe { option_as_ptr(write_node_ptr) } else { return; });

        let Range { start, mut end } = unsafe { write_node.as_ref().element.clone() };

        // Look for pending borrows that can be resumed. Find the maximum node
        // that is < (end, -∞)
        //
        // Safety: The tree is sound, this nodes is new, and there are no
        //         conflicting borrows of the existing nodes.
        let mut maybe_pending_node = unsafe {
            if let Some(node) =
                rbtree::Node::lower_bound(&self.pendings, |e| end.cmp(&e.range.start))
            {
                rbtree::Node::predecessor(node)
            } else {
                self.pendings.map(|root| rbtree::Node::max(root))
            }
            .filter(|node| node.as_ref().element.range.start >= start)
        };

        while let Some(pending_node) = maybe_pending_node {
            // Find the next pending borrow before removing this one
            //
            // Safety: `pending_node` is in the tree and safe to borrow. Note
            // that `resume_borrow` only removes the provided node.
            let next_maybe_pending_node = unsafe {
                rbtree::Node::predecessor(pending_node)
                    .filter(|node| node.as_ref().element.range.start >= start)
            };

            // Unborrow `index..end` before resuming this pending borrow
            let index = unsafe { pending_node.as_ref().element.range.start.clone() };
            debug_assert!((&start..=&end).contains(&&index));

            let complete = if index == start {
                // The unborrowing is complete.
                unsafe { rbtree::Node::remove(WriteNodeCallback, &mut self.writes, write_node) };
                unsafe { *write_node_ptr.as_mut() = None };
                true
            } else {
                // The unborrowing is incomplete. Truncate the borrow to
                // `start..index`.
                //
                // Although we searched for pending nodes that are `< end`,
                // since `end` is moved to the pending node's index on each
                // iteration, we might observe `index == end` here if there are
                // multiple pending nodes at the same index.
                unsafe { write_node.as_mut().element.end = index.clone() };
                end = index;
                false
            };

            // Resume this pending borrow.
            unsafe { self.resume_borrow(pending_node, &mut callback) };

            if complete {
                return;
            }

            maybe_pending_node = next_maybe_pending_node;
        } // while

        // Unborrow `start..end` completely.
        unsafe { rbtree::Node::remove(WriteNodeCallback, &mut self.writes, write_node) };
        unsafe { *write_node_ptr.as_mut() = None };
    }

    /// Attempt to resume and potentially complete a pending borrow.
    ///
    /// This is an internal function that assumes the following:
    ///
    ///  - Called inside `abort_on_unwind`. (This method is not unwind safe).
    ///  - `pending_node` is included in the tree `self.pendings`.
    ///  - None of `self`'s trees' nodes are currently borrowed.
    ///
    /// This method may remove `pending_node` but does not remove or modify any
    /// other pending borrows.
    unsafe fn resume_borrow(
        &mut self,
        mut pending_node_ptr: NonNull<PendingNode<Index, Priority, InProgress>>,
        callback: &mut impl UnlockCallback<InProgress>,
    ) {
        let pending: &Pending<_, _, _> = unsafe { &pending_node_ptr.as_ref().element };
        let parent = pending.parent;

        // The borrow resumes here
        let range = pending.range.clone();

        // Check for conflicting borrows.
        let first_conflict_write_index = self.first_write_borrow_in_range(&range);
        let first_conflict_read_index = if let LockStatePtr::Write(_) = parent {
            self.first_read_borrow_in_range(&range)
        } else {
            None // read borrows can overlap
        };
        let first_conflict_index = match (first_conflict_write_index, first_conflict_read_index) {
            (None, None) => None,
            (one @ Some(_), None) | (None, one @ Some(_)) => one,
            (Some(i0), Some(i1)) => Some(i0.min(i1)),
        };

        if first_conflict_index.as_ref() == Some(&range.start) {
            // No progress
            return;
        }

        // Move `pending_node_ptr` to the new conflict position
        // (Warning: This invalidates `pending` according to Stacked Borrows)
        unsafe { rbtree::Node::remove(PendingNodeCallback, &mut self.pendings, pending_node_ptr) };
        if let Some(first_conflict_index) = first_conflict_index.clone() {
            // Reinsert `pending_node_ptr` at the new position
            unsafe { pending_node_ptr.as_mut() }.element.range.start = first_conflict_index;

            unsafe {
                rbtree::Node::insert(PendingNodeCallback, &mut self.pendings, pending_node_ptr);
            }
        } else {
            // This borrow is complete. Move out `in_progress` from `parent`
            // and assign `None` to `(Read|Write)LockState::pending`.
            // Safety: `parent`'s target still exists and is safe to borrow
            let pending_cell: *mut Option<PendingNode<_, _, _>> = match parent {
                LockStatePtr::Write(state) => unsafe { state.as_ref() }.pending.get(),
                LockStatePtr::Read(state) => unsafe { state.as_ref() }.pending.get(),
            };

            debug_assert_eq!(Some(pending_node_ptr), unsafe {
                option_as_ptr(NonNull::new(pending_cell).unwrap())
            });

            // Notify the client
            // (Warning: This `take` invalidates `pending_node_ptr`.)
            let in_progress = unsafe { (*pending_cell).take() }
                .unwrap()
                .element
                .in_progress;
            callback.complete(in_progress);
        }

        // Expand the existing borrow
        let new_end = first_conflict_index.unwrap_or(range.end);
        match parent {
            LockStatePtr::Write(state) => {
                // Safety: `state` is still claimed
                let state = unsafe { state.as_ref() };

                // ... it's still claimed, right?
                debug_assert_eq!(state.parent.get(), Some(NonNull::from(&*self)));

                // Safety: `state` is still claimed
                let write_node_cell = unsafe { &mut *state.write.get() };
                if let Some(write_node) = write_node_cell.as_mut() {
                    // Yes, there's indeed an existing borrow
                    write_node.element.end = new_end;
                } else {
                    // Actually, the previous borrowed region was empty. Create
                    // a `WriteNode` now.
                    let write_node =
                        write_node_cell.insert(WriteNode::new(range.start..new_end, ()));
                    unsafe {
                        rbtree::Node::insert(
                            WriteNodeCallback,
                            &mut self.writes,
                            NonNull::from(write_node),
                        );
                    }
                }
            }
            LockStatePtr::Read(state) => {
                // Safety: `state` is still claimed
                let state = unsafe { state.as_ref() };

                // ... it's still claimed, right?
                debug_assert_eq!(state.parent.get(), Some(NonNull::from(&*self)));

                // Safety: `state` is still claimed
                let read_node_cells = unsafe { &mut *state.read.get() };
                if let Some([_, read_node1]) = read_node_cells.as_mut() {
                    // Yes, there's indeed an existing borrow; reinsert the second node
                    unsafe {
                        rbtree::Node::remove(
                            ReadNodeCallback,
                            &mut self.reads,
                            NonNull::from(&mut *read_node1),
                        );
                    }
                    read_node1.element.0 = new_end;
                    read_node1.summary = -1;
                    unsafe {
                        rbtree::Node::insert(
                            ReadNodeCallback,
                            &mut self.reads,
                            NonNull::from(&mut *read_node1),
                        );
                    }
                } else {
                    // Actually, the previous borrowed region was empty. Create
                    // `ReadNode`s now.
                    let read_nodes = read_node_cells.insert([
                        ReadNode::new((range.start, 1), 1),
                        ReadNode::new((new_end, -1), -1),
                    ]);
                    unsafe {
                        rbtree::Node::insert(
                            ReadNodeCallback,
                            &mut self.reads,
                            NonNull::from(&mut read_nodes[0]),
                        );
                        rbtree::Node::insert(
                            ReadNodeCallback,
                            &mut self.reads,
                            NonNull::from(&mut read_nodes[1]),
                        );
                    }
                }
            }
        }
    }
}

/// Panic because of [`WriteLockState::parent`] being still set.
#[cold]
#[track_caller]
fn panic_lock_state_in_use() -> ! {
    panic!("attempted to occupy a `*LockState` that is still in use")
}

/// Panic because of [`WriteLockState::parent`] being incorrect.
#[cold]
#[track_caller]
fn panic_lock_state_incorrect_parent() -> ! {
    panic!("attempted to release a lock with an incorrect origin")
}

/// Convert `NonNull<Option<T>>` to `Option<NonNull<T>>`.
///
/// # Safety
///
/// This function temporarily borrows the target to check its variant.
/// (In terms of the Stacked Borrows model, this is considered a *use* of `p`,
/// meaning there must be SharedRW in the stack for the location, and that this
/// function will temporarily put a Unique on top.)
///
/// That this produces SharedRW means the target must be included in
/// `UnsafeCell`. (There are other ways in which this can be safe, though.)
#[inline]
unsafe fn option_as_ptr<T>(mut p: NonNull<Option<T>>) -> Option<NonNull<T>> {
    Some(unsafe { p.as_mut() }.as_mut()?.into())
}

/// Convert `NonNull<Option<[T; 2]>>` to `Option<[NonNull<T>; 2]>`.
///
/// # Safety
///
/// See [`option_as_ptr`].
#[inline]
unsafe fn option_array2_as_ptr<T>(mut p: NonNull<Option<[T; 2]>>) -> Option<[NonNull<T>; 2]> {
    Some(
        unsafe { p.as_mut() }
            .as_mut()?
            .each_mut()
            .map(NonNull::from),
    )
}

// Destructors
// -----------------------------------------------------------------------------

impl<Index, Priority, InProgress> Drop for RbTreeIntervalRwLockCore<Index, Priority, InProgress> {
    #[inline]
    #[track_caller]
    fn drop(&mut self) {
        if self.reads.is_some() || self.writes.is_some() || self.pendings.is_some() {
            panic_drop_when_still_locked();
        }
    }
}

/// Panic because of [`RbTreeIntervalRwLockCore`] still being locked.
#[cold]
#[track_caller]
fn panic_drop_when_still_locked() -> ! {
    panic!("attempted to drop an `RbTreeIntervalRwLockCore` while it's locked")
}

impl<Index, Priority, InProgress> EarlyDrop for ReadLockStateInner<Index, Priority, InProgress> {
    #[inline]
    #[track_caller]
    unsafe fn early_drop(self: Pin<&Self>) {
        if self.parent.get().is_some() {
            panic_drop_when_still_linked();
        }
    }
}

impl<Index, Priority, InProgress> EarlyDrop for WriteLockStateInner<Index, Priority, InProgress> {
    #[inline]
    #[track_caller]
    unsafe fn early_drop(self: Pin<&Self>) {
        if self.parent.get().is_some() {
            panic_drop_when_still_linked();
        }
    }
}

impl<Index, Priority, InProgress> EarlyDrop for TryReadLockStateInner<Index, Priority, InProgress> {
    #[inline]
    #[track_caller]
    unsafe fn early_drop(self: Pin<&Self>) {
        if self.parent.get().is_some() {
            panic_drop_when_still_linked();
        }
    }
}

impl<Index, Priority, InProgress> EarlyDrop
    for TryWriteLockStateInner<Index, Priority, InProgress>
{
    #[inline]
    #[track_caller]
    unsafe fn early_drop(self: Pin<&Self>) {
        if self.parent.get().is_some() {
            panic_drop_when_still_linked();
        }
    }
}

/// Panic because of [`WriteLockState::parent`] being still set.
#[cold]
#[track_caller]
fn panic_drop_when_still_linked() -> ! {
    panic!("attempted to early-drop a `*LockState` while it's holding a lock")
}
