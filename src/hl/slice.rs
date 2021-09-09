//! Provides specialized readers-writer locks for borrowing subslices efficiently.
use core::{
    fmt,
    future::Future,
    mem,
    ops::{Deref, DerefMut, Range},
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll},
};
use futures::{future::FusedFuture, ready};
use stable_deref_trait::StableDeref;

use crate::raw::{self, RawAsyncIntervalRwLock, RawBlockingIntervalRwLock, RawIntervalRwLock};

#[cfg(test)]
mod tests;

// Lock guards associated with state data
// ----------------------------------------------------------------------------

if_alloc! {
    use alloc::boxed::Box;
    use crate::utils::PinDerefMut;

    /// Associates a lock guard with state data.
    ///
    /// The `*_boxed` method family wraps a lock guard with this type instead of
    /// having their own lock guard types. While this is less efficient (because
    /// of an extra pointer to `State`) and requires lifetime transmutation,
    /// this approach lets us minimize code duplication.
    #[pin_project::pin_project]
    struct WithState<LockGuard, State> {
        /// `guard` must precede `state` because `guard` might include a
        /// reference to `state`.
        guard: Option<LockGuard>,
        #[pin]
        state: State,
        /// `!Unpin` because `guard` might reference `state`.
        _phantom: core::marker::PhantomPinned,
    }

    impl<LockGuard, State: Default> Default for WithState<LockGuard, State> {
        #[inline]
        fn default() -> Self {
            Self {
                guard: None,
                state: State::default(),
                _phantom: core::marker::PhantomPinned,
            }
        }
    }

    impl<LockGuard: Deref, State> Deref for WithState<LockGuard, State> {
        type Target = LockGuard::Target;

        #[inline]
        fn deref(&self) -> &Self::Target {
            self.guard.as_ref().unwrap()
        }
    }

    impl<LockGuard: DerefMut, State> PinDerefMut for WithState<LockGuard, State> {
        #[inline]
        fn pin_deref_mut(self: Pin<&mut Self>) -> &mut Self::Target {
            self.project().guard.as_mut().unwrap()
        }
    }

    /// Erase the pinned mutable reference's lifetime.
    ///
    /// This is used to create a self-referential object of type [`WithState`],
    #[inline]
    unsafe fn transmute_lifetime_mut<'a, T: ?Sized>(x: Pin<&mut T>) -> Pin<&'a mut T> {
        unsafe { Pin::new_unchecked(&mut *( x.get_unchecked_mut() as *mut T)) }
    }
}

// `SliceIntervalRwLock` and its methods
// ----------------------------------------------------------------------------

/// A specialized readers-writer lock for borrowing subslices of `Container:
/// DerefMut<Target = [Element]>`.
#[pin_project::pin_project]
pub struct SliceIntervalRwLock<Container, Element, RawLock> {
    container: Container,
    #[pin]
    raw: RawLock,
    /// A pointer to the elements, acquired ahead-of-time by `deref_mut`.
    /// This is guaranteed to be up-to-date because of `Container: StableDeref`.
    ptr: NonNull<[Element]>,
}

// Safety: `ptr` is just a cached value of `container.as_mut_ptr()` and
//		   therefore can be ignored when determining this type's thread safety.
unsafe impl<Container: Send, RawLock: Send, Element: Send> Send
    for SliceIntervalRwLock<Container, Element, RawLock>
{
}

unsafe impl<Container: Sync, RawLock: Sync, Element: Send + Sync> Sync
    for SliceIntervalRwLock<Container, Element, RawLock>
{
}

impl<Container, Element, RawLock> SliceIntervalRwLock<Container, Element, RawLock>
where
    Container: Deref<Target = [Element]> + DerefMut + StableDeref,
    RawLock: RawIntervalRwLock<Index = usize>,
{
    #[inline]
    pub fn new(mut container: Container) -> Self {
        Self {
            ptr: NonNull::from(&mut *container),
            container,
            raw: RawLock::INIT,
        }
    }

    /// Replace the contained `Container` with another one, returning the old
    /// one.
    #[inline]
    pub fn replace_container(self: Pin<&mut Self>, mut container: Container) -> Container {
        let this = self.project();

        // `deref_mut` can panic, hence we must do it before updating our fields
        let ptr = NonNull::from(&mut *container);
        let old_container = mem::replace(this.container, container);
        *this.ptr = ptr;

        old_container
    }

    /// Replace the contained `Container` with [`Default::default`], returning
    /// the old one.
    #[inline]
    pub fn take_container(self: Pin<&mut Self>) -> Container
    where
        Container: Default,
    {
        self.replace_container(Container::default())
    }

    /// Get the number of elements in the underlying slice.
    #[inline]
    pub fn len(&self) -> usize {
        self.ptr.len()
    }

    /// Get a flag indicating whether the underlying slice has no elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get a raw pointer to the underlying slice.
    #[inline]
    pub fn as_ptr(&self) -> *mut [Element] {
        self.ptr.as_ptr()
    }

    /// Get a raw pointer to the underlying slice.
    #[inline]
    pub fn as_non_null_ptr(&self) -> NonNull<[Element]> {
        self.ptr
    }

    /// Return a mutable reference to the underlying slice.
    #[inline]
    pub fn get_mut(self: Pin<&mut Self>) -> &mut [Element] {
        // Safety: `self.ptr`'s value is literally `&mut *self.container`, which
        // should be still true because of `Container: StableDeref`.
        // Nevertheless we are using the cached value in case `deref_mut` does
        // something nasty to the memory model.
        unsafe { self.project().ptr.as_mut() }
    }

    /// Get a slice pointer to subelements.
    #[inline]
    fn slice(&self, range: Range<usize>) -> NonNull<[Element]> {
        let ptr = self.ptr;
        assert!(
            range.start <= range.end && range.end <= ptr.len(),
            "out of bounds"
        );

        // Safety: We just checked that `range` is in-bounds
        unsafe { ptr.get_unchecked_mut(range) }
    }

    /// Attempt to acquire a reader lock on the specified range.
    pub fn try_read<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        mut lock_state: Pin<&'a mut RawLock::TryReadLockState>,
    ) -> Result<TryReadLockGuard<'a, Element, RawLock, RawLock::TryReadLockState>, TryLockError>
    {
        let this = self.project_ref();
        let ptr = self.slice(range.clone()); // may panic
        if this.raw.try_lock_read(range, Pin::as_mut(&mut lock_state)) {
            Ok(TryReadLockGuard {
                lock_state,
                raw: this.raw,
                ptr,
            })
        } else {
            Err(TryLockError::WouldBlock)
        }
    }

    /// Attempt to acquire a writer lock on the specified range.
    pub fn try_write<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        mut lock_state: Pin<&'a mut RawLock::TryWriteLockState>,
    ) -> Result<TryWriteLockGuard<'a, Element, RawLock, RawLock::TryWriteLockState>, TryLockError>
    {
        let this = self.project_ref();
        let ptr = self.slice(range.clone()); // may panic
        if this.raw.try_lock_write(range, Pin::as_mut(&mut lock_state)) {
            Ok(TryWriteLockGuard {
                lock_state,
                raw: this.raw,
                ptr,
            })
        } else {
            Err(TryLockError::WouldBlock)
        }
    }

    if_alloc! {
        #[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
        /// The variant of [`Self::try_read`] that dynamically allocates a
        /// storage for borrow state data.
        ///
        /// # Example
        ///
        /// ```rust
        /// use interlock::hl::slice::SyncRbTreeVecIntervalRwLock;
        ///
        /// let vec = Box::pin(SyncRbTreeVecIntervalRwLock::<_, ()>::new(vec![42u8; 64]));
        /// let vec = vec.as_ref();
        ///
        /// let guard = vec.try_read_boxed(0..4).unwrap();
        /// assert_eq!(guard[..], [42u8; 4][..]);
        /// ```
        pub fn try_read_boxed(
            self: Pin<&Self>,
            range: Range<usize>,
        ) -> Result<Pin<Box<impl Deref<Target = [Element]> + '_>>, TryLockError>
        {
            let mut guard_with_state = Box::pin(WithState::default());
            {
                let guard_with_state = WithState::project(Pin::as_mut(&mut guard_with_state));
                let state = unsafe { transmute_lifetime_mut(guard_with_state.state) };
                *guard_with_state.guard = Some(self.try_read(range, state)?);
            }
            Ok(guard_with_state)
        }

        #[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
        /// The variant of [`Self::try_write`] that dynamically allocates a
        /// storage for borrow state data.
        ///
        /// # Example
        ///
        /// ```rust
        /// use interlock::{hl::slice::SyncRbTreeVecIntervalRwLock, utils::PinDerefMut};
        /// use std::pin::Pin;
        ///
        /// let vec = Box::pin(SyncRbTreeVecIntervalRwLock::<_, ()>::new(vec![42u8; 64]));
        /// let vec = vec.as_ref();
        ///
        /// let mut guard = vec.try_write_boxed(0..4).unwrap();
        /// Pin::as_mut(&mut guard).pin_deref_mut().copy_from_slice(&[56; 4]);
        /// ```
        pub fn try_write_boxed(
            self: Pin<&Self>,
            range: Range<usize>,
        ) -> Result<Pin<Box<impl PinDerefMut<Target = [Element]> + '_>>, TryLockError>
        {
            let mut guard_with_state = Box::pin(WithState::default());
            {
                let guard_with_state = WithState::project(Pin::as_mut(&mut guard_with_state));
                let state = unsafe { transmute_lifetime_mut(guard_with_state.state) };
                *guard_with_state.guard = Some(self.try_write(range, state)?);
            }
            Ok(guard_with_state)
        }
    }
}

/// Indicates a failure of a non-blocking lock operation.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum TryLockError {
    /// The lock could not be acquired at this time because the operation would
    /// otherwise block.
    #[cfg_attr(
        feature = "std",
        error("lock failed because the operation would block")
    )]
    WouldBlock,
}

/// # Blocking Lock Operations
///
/// These methods require `RawLock: `[`RawBlockingIntervalRwLock`].
impl<Container, Element, RawLock> SliceIntervalRwLock<Container, Element, RawLock>
where
    Container: Deref<Target = [Element]> + DerefMut + StableDeref,
    RawLock: RawIntervalRwLock<Index = usize> + RawBlockingIntervalRwLock,
{
    /// Acquire a reader lock on the specified range, blocking the current
    /// thread until it can be acquired.
    pub fn read<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: RawLock::Priority,
        mut lock_state: Pin<&'a mut RawLock::ReadLockState>,
    ) -> ReadLockGuard<'a, Element, RawLock, RawLock::ReadLockState> {
        let this = self.project_ref();
        let ptr = self.slice(range.clone()); // may panic
        this.raw
            .lock_read(range, priority, Pin::as_mut(&mut lock_state));
        ReadLockGuard {
            lock_state,
            raw: this.raw,
            ptr,
        }
    }

    /// Acquire a writer lock on the specified range, blocking the current
    /// thread until it can be acquired.
    pub fn write<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: RawLock::Priority,
        mut lock_state: Pin<&'a mut RawLock::WriteLockState>,
    ) -> WriteLockGuard<'a, Element, RawLock, RawLock::WriteLockState> {
        let this = self.project_ref();
        let ptr = self.slice(range.clone()); // may panic
        this.raw
            .lock_write(range, priority, Pin::as_mut(&mut lock_state));
        WriteLockGuard {
            lock_state,
            raw: this.raw,
            ptr,
        }
    }

    if_alloc! {
        #[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
        /// The variant of [`Self::read`] that dynamically allocates a
        /// storage for borrow state data.
        ///
        /// # Example
        ///
        /// ```rust
        /// use interlock::hl::slice::SyncRbTreeVecIntervalRwLock;
        ///
        /// let vec = Box::pin(SyncRbTreeVecIntervalRwLock::new(vec![42u8; 64]));
        /// let vec = vec.as_ref();
        ///
        /// let guard = vec.read_boxed(0..4, ());
        /// assert_eq!(guard[..], [42u8; 4][..]);
        /// ```
        pub fn read_boxed(
            self: Pin<&Self>,
            range: Range<usize>,
            priority: RawLock::Priority,
        ) -> Pin<Box<impl Deref<Target = [Element]> + '_>>
        {
            let mut guard_with_state = Box::pin(WithState::default());
            {
                let guard_with_state = WithState::project(Pin::as_mut(&mut guard_with_state));
                let state = unsafe { transmute_lifetime_mut(guard_with_state.state) };
                *guard_with_state.guard = Some(self.read(range, priority, state));
            }
            guard_with_state
        }

        #[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
        /// The variant of [`Self::write`] that dynamically allocates a
        /// storage for borrow state data.
        ///
        /// # Example
        ///
        /// ```rust
        /// use interlock::{hl::slice::SyncRbTreeVecIntervalRwLock, utils::PinDerefMut};
        /// use std::pin::Pin;
        ///
        /// let vec = Box::pin(SyncRbTreeVecIntervalRwLock::new(vec![42u8; 64]));
        /// let vec = vec.as_ref();
        ///
        /// let mut guard = vec.write_boxed(0..4, ());
        /// Pin::as_mut(&mut guard).pin_deref_mut().copy_from_slice(&[56; 4]);
        /// ```
        pub fn write_boxed(
            self: Pin<&Self>,
            range: Range<usize>,
            priority: RawLock::Priority,
        ) -> Pin<Box<impl PinDerefMut<Target = [Element]> + '_>>
        {
            let mut guard_with_state = Box::pin(WithState::default());
            {
                let guard_with_state = WithState::project(Pin::as_mut(&mut guard_with_state));
                let state = unsafe { transmute_lifetime_mut(guard_with_state.state) };
                *guard_with_state.guard = Some(self.write(range, priority, state));
            }
            guard_with_state
        }
    }
}

/// # `Future`-based Lock Operations
///
/// These methods require `RawLock: `[`RawAsyncIntervalRwLock`].
impl<Container, Element, RawLock> SliceIntervalRwLock<Container, Element, RawLock>
where
    Container: Deref<Target = [Element]> + DerefMut + StableDeref,
    RawLock: RawIntervalRwLock<Index = usize> + RawAsyncIntervalRwLock,
{
    /// Acquire a reader lock on the specified range asynchronously.
    pub fn async_read<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: RawLock::Priority,
        mut lock_state: Pin<&'a mut RawLock::ReadLockState>,
    ) -> ReadLockFuture<'a, Element, RawLock, RawLock::ReadLockState> {
        let this = self.project_ref();
        let ptr = self.slice(range.clone()); // may panic
        this.raw
            .start_lock_read(range, priority, Pin::as_mut(&mut lock_state));
        ReadLockFuture {
            guard: Some(AsyncReadLockGuard {
                lock_state,
                raw: this.raw,
                ptr,
            }),
        }
    }

    /// Acquire a writer lock on the specified range asynchronously.
    pub fn async_write<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: RawLock::Priority,
        mut lock_state: Pin<&'a mut RawLock::WriteLockState>,
    ) -> WriteLockFuture<'a, Element, RawLock, RawLock::WriteLockState> {
        let this = self.project_ref();
        let ptr = self.slice(range.clone()); // may panic
        this.raw
            .start_lock_write(range, priority, Pin::as_mut(&mut lock_state));
        WriteLockFuture {
            guard: Some(AsyncWriteLockGuard {
                lock_state,
                raw: this.raw,
                ptr,
            }),
        }
    }

    if_alloc! {
        #[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
        /// The variant of [`Self::async_read`] that dynamically allocates a
        /// storage for borrow state data.
        ///
        /// # Example
        ///
        /// ```rust
        /// # #[tokio::main] async fn main() {
        /// use interlock::hl::slice::AsyncRbTreeVecIntervalRwLock;
        /// use parking_lot::RawMutex;
        ///
        /// let vec = Box::pin(AsyncRbTreeVecIntervalRwLock::<RawMutex, _>::new(vec![42u8; 64]));
        /// let vec = vec.as_ref();
        ///
        /// let guard = vec.async_read_boxed(0..4, ()).await;
        /// assert_eq!(guard[..], [42u8; 4][..]);
        /// # }
        /// ```
        pub async fn async_read_boxed(
            self: Pin<&Self>,
            range: Range<usize>,
            priority: RawLock::Priority,
        ) -> Pin<Box<impl Deref<Target = [Element]> + '_>>
        {
            let mut guard_with_state = Box::pin(WithState::default());
            {
                let guard_with_state = WithState::project(Pin::as_mut(&mut guard_with_state));
                let state = unsafe { transmute_lifetime_mut(guard_with_state.state) };
                *guard_with_state.guard = Some(self.async_read(range, priority, state).await);
            }
            guard_with_state
        }

        #[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
        /// The variant of [`Self::async_write`] that dynamically allocates a
        /// storage for borrow state data.
        ///
        /// # Example
        ///
        /// ```rust
        /// # #[tokio::main] async fn main() {
        /// use interlock::{hl::slice::AsyncRbTreeVecIntervalRwLock, utils::PinDerefMut};
        /// use parking_lot::RawMutex;
        /// use std::pin::Pin;
        ///
        /// let vec = Box::pin(AsyncRbTreeVecIntervalRwLock::<RawMutex, _>::new(vec![42u8; 64]));
        /// let vec = vec.as_ref();
        ///
        /// let mut guard = vec.async_write_boxed(0..4, ()).await;
        /// Pin::as_mut(&mut guard).pin_deref_mut().copy_from_slice(&[56; 4]);
        /// # }
        /// ```
        pub async fn async_write_boxed(
            self: Pin<&Self>,
            range: Range<usize>,
            priority: RawLock::Priority,
        ) -> Pin<Box<impl PinDerefMut<Target = [Element]> + '_>>
        {
            let mut guard_with_state = Box::pin(WithState::default());
            {
                let guard_with_state = WithState::project(Pin::as_mut(&mut guard_with_state));
                let state = unsafe { transmute_lifetime_mut(guard_with_state.state) };
                *guard_with_state.guard = Some(self.async_write(range, priority, state).await);
            }
            guard_with_state
        }
    }
}

macro_rules! define_lock_future {
	(
		$( #[$meta:meta] )*
		pub struct $ident:ident<'_, Element, RawLock, LockState>
		where
			RawLock: [$($raw_lock_bounds:tt)*]
		{
			guard: $guard_ty:ident,
		}

		impl Future for _ { => $poll_method:ident }
	) => {
		$( #[$meta] )*
		///
		/// # Notes
		///
		/// It's probably a bad idea to [`forget`] a value of this type. The
		/// underlying `LockState` will remain associated with a pending lock
		/// (and dropping it will abort the program), and you can't dissociate
		/// it.
		///
		/// [`forget`]: core::mem::forget
		pub struct $ident<'a, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
			/// Stores a lock guard representing the pending lock, which will
			/// be returned by `poll` when the lock completes.
			guard: Option<$guard_ty<'a, Element, RawLock, LockState>>,
		}

		impl<'a, Element, RawLock, LockState> Future
			for $ident<'a, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
			type Output = $guard_ty<'a, Element, RawLock, LockState>;

			fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
				let this = Pin::into_inner(self);
				let guard = this.guard.as_mut().expect("future polled after completion");
				ready!(guard.raw.$poll_method(Pin::as_mut(&mut guard.lock_state), cx));

				Poll::Ready(this.guard.take().unwrap())
			}
		}

		impl<'a, Element, RawLock, LockState> FusedFuture
			for $ident<'a, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
			#[inline]
    		fn is_terminated(&self) -> bool {
    			self.guard.is_none()
    		}
		}
	};
}

define_lock_future! {
    /// A future representing a pending reader lock of [`SliceIntervalRwLock`],
    /// which will resolve when the lock has been successfully acquired.
    pub struct ReadLockFuture<'_, Element, RawLock, LockState>
    where
        RawLock: [RawAsyncIntervalRwLock<Index = usize, ReadLockState = LockState>]
    {
        guard: AsyncReadLockGuard,
    }

    impl Future for _ { => poll_lock_read }
}

define_lock_future! {
    /// A future representing a pending writer lock of [`SliceIntervalRwLock`],
    /// which will resolve when the lock has been successfully acquired.
    pub struct WriteLockFuture<'_, Element, RawLock, LockState>
    where
        RawLock: [RawAsyncIntervalRwLock<Index = usize, WriteLockState = LockState>]
    {
        guard: AsyncWriteLockGuard,
    }

    impl Future for _ { => poll_lock_write }
}

macro_rules! define_lock_guard {
	(
		$( #[$meta:meta] )*
		pub struct $ident:ident<'_, Element, RawLock, LockState>
		where
			RawLock: [$($raw_lock_bounds:tt)*];
		impl $deref:tt for _;
		impl Drop for _ { => $unlock_method:ident }
	) => {
		$( #[$meta] )*
		///
		/// # Notes
		///
		/// It's probably a bad idea to [`forget`] a value of this type. The
		/// underlying `LockState` will remain associated with a lock (and
		/// dropping it will abort the program), and you can't dissociate it.
		///
		/// [`forget`]: core::mem::forget
		pub struct $ident<'a, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
			lock_state: Pin<&'a mut LockState>,
			raw: Pin<&'a RawLock>,
			ptr: NonNull<[Element]>,
		}

		unsafe impl<Element: Send + Sync, RawLock: Sync, LockState: Send> Send
			for $ident<'_, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
		}

		unsafe impl<Element: Send + Sync, RawLock: Sync, LockState: Sync> Sync
			for $ident<'_, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
		}

		// impl `Deref` and optionally `DerefMut`
		define_lock_guard!(@deref $deref [$($raw_lock_bounds)*] $ident);

		impl<Element, RawLock, LockState> fmt::Debug for $ident<'_, Element, RawLock, LockState>
		where
			Element: fmt::Debug,
			RawLock: $($raw_lock_bounds)*,
		{
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				(**self).fmt(f)
			}
		}

		impl<Element, RawLock, LockState> Drop for $ident<'_, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
			#[inline]
			fn drop(&mut self) {
				self.raw.$unlock_method(Pin::as_mut(&mut self.lock_state));
			}
		}
	};

	(@deref DerefMut [$($raw_lock_bounds:tt)*] $ident:ident) => {
		impl<Element, RawLock, LockState> DerefMut for $ident<'_, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
			#[inline]
			fn deref_mut(&mut self) -> &mut Self::Target {
				// Safety: Upheld by a runtime check
				unsafe { self.ptr.as_mut() }
			}
		}

		define_lock_guard!(@deref Deref [$($raw_lock_bounds)*] $ident);
	};

	(@deref Deref [$($raw_lock_bounds:tt)*] $ident:ident) => {
		impl<Element, RawLock, LockState> Deref for $ident<'_, Element, RawLock, LockState>
		where
			RawLock: $($raw_lock_bounds)*,
		{
			type Target = [Element];

			#[inline]
			fn deref(&self) -> &Self::Target {
				// Safety: Upheld by a runtime check
				unsafe { self.ptr.as_ref() }
			}
		}
	};
}

define_lock_guard! {
    /// [`SliceIntervalRwLock`]'s RAII lock guard for a non-blocking reader lock.
    pub struct TryReadLockGuard<'_, Element, RawLock, LockState>
    where
        RawLock: [RawIntervalRwLock<Index = usize, TryReadLockState = LockState>];
    impl Deref for _;
    impl Drop for _ { => unlock_try_read }
}

define_lock_guard! {
    /// [`SliceIntervalRwLock`]'s RAII lock guard for a non-blocking writing lock.
    pub struct TryWriteLockGuard<'_, Element, RawLock, LockState>
    where
        RawLock: [RawIntervalRwLock<Index = usize, TryWriteLockState = LockState>];
    impl DerefMut for _;
    impl Drop for _ { => unlock_try_write }
}

define_lock_guard! {
    /// [`SliceIntervalRwLock`]'s RAII lock guard for a blocking reader lock.
    pub struct ReadLockGuard<'_, Element, RawLock, LockState>
    where
        RawLock: [RawBlockingIntervalRwLock<Index = usize, ReadLockState = LockState>];
    impl Deref for _;
    impl Drop for _ { => unlock_read }
}

define_lock_guard! {
    /// [`SliceIntervalRwLock`]'s RAII lock guard for a blocking writing lock.
    pub struct WriteLockGuard<'_, Element, RawLock, LockState>
    where
        RawLock: [RawBlockingIntervalRwLock<Index = usize, WriteLockState = LockState>];
    impl DerefMut for _;
    impl Drop for _ { => unlock_write }
}

define_lock_guard! {
    /// [`SliceIntervalRwLock`]'s RAII lock guard for a `Future`-based reader
    /// lock.
    pub struct AsyncReadLockGuard<'_, Element, RawLock, LockState>
    where
        RawLock: [RawAsyncIntervalRwLock<Index = usize, ReadLockState = LockState>];
    impl Deref for _;
    impl Drop for _ { => unlock_read }
}

define_lock_guard! {
    /// [`SliceIntervalRwLock`]'s RAII lock guard for a `Future`-based writer
    /// lock.
    pub struct AsyncWriteLockGuard<'_, Element, RawLock, LockState>
    where
        RawLock: [RawAsyncIntervalRwLock<Index = usize, WriteLockState = LockState>];
    impl DerefMut for _;
    impl Drop for _ { => unlock_write }
}

// Utility trait to extract an element type
// ----------------------------------------------------------------------------

mod hidden {
    pub trait DerefToSlice {
        type Element;
    }

    impl<Element, T: ?Sized + core::ops::DerefMut<Target = [Element]>> DerefToSlice for T {
        type Element = Element;
    }
}

// Convenient Type Aliases
// ----------------------------------------------------------------------------

/// A non-thread-safe readers-writer lock for borrowing subslices of
/// `Container`, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type LocalRbTreeSliceIntervalRwLock<Container> = SliceIntervalRwLock<
    Container,
    <Container as hidden::DerefToSlice>::Element,
    raw::local::LocalRawRbTreeIntervalRwLock<usize>,
>;

#[cfg(feature = "std")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "std")))]
/// A thread-safe, blocking readers-writer lock for borrowing subslices of
/// `Container`, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type SyncRbTreeSliceIntervalRwLock<Container, Priority = ()> = SliceIntervalRwLock<
    Container,
    <Container as hidden::DerefToSlice>::Element,
    raw::sync::SyncRawRbTreeIntervalRwLock<usize, Priority>,
>;

#[cfg(feature = "async")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "async")))]
/// A thread-safe, `Future`-oriented readers-writer lock for borrowing
/// subslices of `Container`, implemented by a [red-black tree][1].
/// `RawMutex: `[`lock_api::RawMutex`] is used to protect the internal state
/// data from concurrent accesses.
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type AsyncRbTreeSliceIntervalRwLock<RawMutex, Container, Priority = ()> = SliceIntervalRwLock<
    Container,
    <Container as hidden::DerefToSlice>::Element,
    raw::future::AsyncRawRbTreeIntervalRwLock<RawMutex, usize, Priority>,
>;

/// A non-thread-safe readers-writer lock for borrowing subslices of
/// `&'a mut [Element]`, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type LocalRbTreeSliceRefIntervalRwLock<'a, Element> = SliceIntervalRwLock<
    &'a mut [Element],
    Element,
    raw::local::LocalRawRbTreeIntervalRwLock<usize>,
>;

#[cfg(feature = "std")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "std")))]
/// A thread-safe, blocking readers-writer lock for borrowing subslices of
/// `&'a mut [Element]`, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type SyncRbTreeSliceRefIntervalRwLock<'a, Element, Priority = ()> = SliceIntervalRwLock<
    &'a mut [Element],
    Element,
    raw::sync::SyncRawRbTreeIntervalRwLock<usize, Priority>,
>;

#[cfg(feature = "async")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "async")))]
/// A thread-safe, `Future`-oriented readers-writer lock for borrowing
/// subslices of `&'a mut [Element]`, implemented by a [red-black tree][1].
/// `RawMutex: `[`lock_api::RawMutex`] is used to protect the internal state
/// data from concurrent accesses.
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type AsyncRbTreeSliceRefIntervalRwLock<'a, RawMutex, Element, Priority = ()> =
    SliceIntervalRwLock<
        &'a mut [Element],
        Element,
        raw::future::AsyncRawRbTreeIntervalRwLock<RawMutex, usize, Priority>,
    >;

#[cfg(feature = "alloc")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "alloc")))]
/// A non-thread-safe readers-writer lock for borrowing subslices of
/// `Vec<Element>`, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type LocalRbTreeVecIntervalRwLock<Element> = SliceIntervalRwLock<
    alloc::vec::Vec<Element>,
    Element,
    raw::local::LocalRawRbTreeIntervalRwLock<usize>,
>;

#[cfg(feature = "std")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "std")))]
/// A thread-safe, blocking readers-writer lock for borrowing subslices of
/// `Vec<Element>`, implemented by a [red-black tree][1].
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type SyncRbTreeVecIntervalRwLock<Element, Priority = ()> = SliceIntervalRwLock<
    alloc::vec::Vec<Element>,
    Element,
    raw::sync::SyncRawRbTreeIntervalRwLock<usize, Priority>,
>;

#[cfg(all(feature = "async", feature = "alloc"))]
#[cfg_attr(
    feature = "doc_cfg",
    doc(cfg(all(feature = "async", feature = "alloc")))
)]
/// A thread-safe, `Future`-oriented readers-writer lock for borrowing
/// subslices of `Vec<Element>`, implemented by a [red-black tree][1].
/// `RawMutex: `[`lock_api::RawMutex`] is used to protect the internal state
/// data from concurrent accesses.
///
/// [1]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
pub type AsyncRbTreeVecIntervalRwLock<RawMutex, Element, Priority = ()> = SliceIntervalRwLock<
    alloc::vec::Vec<Element>,
    Element,
    raw::future::AsyncRawRbTreeIntervalRwLock<RawMutex, usize, Priority>,
>;
