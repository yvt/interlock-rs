use cryo::{AtomicLock, Cryo};
use futures::future::{BoxFuture, FutureExt};
use parking_lot::RawMutex;
use pin_utils::pin_mut;
use rand::prelude::*;
use std::{
    prelude::v1::*,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
    vec,
};

use super::*;

macro_rules! gen_nonblocking_test {
    ($modname:ident, $ty:ty) => {
        mod $modname {
            use super::*;

            #[test]
            fn smoke() {
                let vec = <$ty>::new(vec![0i32; 16]);
                pin_mut!(vec);
                nonblocking::smoke(Pin::as_ref(&vec));
            }
        }
    };
}

gen_nonblocking_test!(nonblocking_local, LocalRbTreeVecIntervalRwLock<i32>);
gen_nonblocking_test!(nonblocking_sync, SyncRbTreeVecIntervalRwLock<i32>);
gen_nonblocking_test!(
    nonblocking_async,
    AsyncRbTreeVecIntervalRwLock<RawMutex, i32>
);

mod nonblocking {
    use super::*;

    // FIXME: The generic parameters are inconsistent with `blockingish`
    pub(super) fn smoke<Container, Element, RawLock>(
        rwlock: Pin<&SliceIntervalRwLock<Container, Element, RawLock>>,
    ) where
        Container: Deref<Target = [Element]> + DerefMut + StableDeref,
        RawLock: RawIntervalRwLock<Index = usize>,
    {
        // Read borrows should not conflict with each other
        state!(let mut state);
        let guard1 = rwlock.try_read(2..4, state).unwrap();

        state!(let mut state);
        let guard2 = rwlock.try_read(2..5, state).unwrap();

        state!(let mut state);
        let guard3 = rwlock.try_read(0..4, state).unwrap();

        state!(let mut state);
        let guard4 = rwlock.try_read(4..6, state).unwrap();

        // Write borrows conflict with read borrows
        for i in 0..6 {
            state!(let mut state);
            rwlock.try_write(i..i + 1, state).err().unwrap();
        }

        drop((guard1, guard4, guard3, guard2));

        // Write borrows
        state!(let mut state);
        let guard5 = rwlock.try_write(0..4, state).unwrap();

        state!(let mut state);
        let guard6 = rwlock.try_write(5..7, state).unwrap();

        // Any borrows conflict with write borrows
        for i in 0..7 {
            if i == 4 {
                continue;
            }
            state!(let mut state);
            rwlock.try_write(i..i + 1, state).err().unwrap();

            state!(let mut state);
            rwlock.try_read(i..i + 1, state).err().unwrap();
        }

        drop((guard6, guard5));
    }
}

trait DynAsyncIntervalRwLock: Send + Sync + 'static {
    type Element;
    type Priority;
    type ReadLockState: Default + Send + Sync;
    type WriteLockState: Default + Send + Sync;

    fn new(inner: Vec<Self::Element>) -> Self;

    fn is_cancellable(&self) -> bool;

    fn async_read<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: Self::Priority,
        lock_state: Pin<&'a mut Self::ReadLockState>,
    ) -> BoxFuture<'a, Box<dyn Deref<Target = [Self::Element]> + Send + Sync + 'a>>;

    fn async_write<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: Self::Priority,
        lock_state: Pin<&'a mut Self::WriteLockState>,
    ) -> BoxFuture<'a, Box<dyn DerefMut<Target = [Self::Element]> + Send + Sync + 'a>>;
}

impl<Element, Priority> DynAsyncIntervalRwLock
    for SliceIntervalRwLock<
        Vec<Element>,
        Element,
        raw::sync::SyncRawRbTreeIntervalRwLock<usize, Priority>,
    >
where
    Priority: Ord,
    Self: Send + Sync + 'static,
    Element: Send + Sync + 'static,
    Priority: Send + Sync + 'static,
{
    type Element = Element;
    type Priority = Priority;
    type ReadLockState = <
    	raw::sync::SyncRawRbTreeIntervalRwLock<usize, Priority>
    	as RawBlockingIntervalRwLock
    >::ReadLockState;
    type WriteLockState = <
    	raw::sync::SyncRawRbTreeIntervalRwLock<usize, Priority>
    	as RawBlockingIntervalRwLock
    >::WriteLockState;

    fn new(inner: Vec<Element>) -> Self {
        Self::new(inner)
    }

    fn is_cancellable(&self) -> bool {
        false
    }

    fn async_read<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: Self::Priority,
        lock_state: Pin<&'a mut Self::ReadLockState>,
    ) -> BoxFuture<'a, Box<dyn Deref<Target = [Self::Element]> + Send + Sync + 'a>> {
        async move {
            // Extend the lifetime of inputs and let `cryo` do runtime
            // enforcement. This can be circumvented by doing `forget` on the
            // `Future`, so it's actually unsound (see `cryo::cryo!`'s
            // documentation).
            let this: Pin<&'static Self> = unsafe { std::mem::transmute(self) };
            let lock_state: Pin<&'static mut _> = unsafe { std::mem::transmute(lock_state) };

            let cryo = unsafe { Cryo::<_, AtomicLock>::new(&()) };
            pin_mut!(cryo);
            let cryo_ref = cryo.as_ref().borrow();
            let guard = tokio::task::spawn_blocking(move || {
                let _cryo_ref = cryo_ref;
                this.read(range, priority, lock_state)
            })
            .await
            .unwrap();

            // Shorten `'static` to `'a`
            Box::new(guard) as _
        }
        .boxed()
    }

    fn async_write<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: Self::Priority,
        lock_state: Pin<&'a mut Self::WriteLockState>,
    ) -> BoxFuture<'a, Box<dyn DerefMut<Target = [Self::Element]> + Send + Sync + 'a>> {
        async move {
            // Extend the lifetime of inputs and ditto.
            let this: Pin<&'static Self> = unsafe { std::mem::transmute(self) };
            let lock_state: Pin<&'static mut _> = unsafe { std::mem::transmute(lock_state) };

            let cryo = unsafe { Cryo::<_, AtomicLock>::new(&()) };
            pin_mut!(cryo);
            let cryo_ref = cryo.as_ref().borrow();
            let guard = tokio::task::spawn_blocking(move || {
                let _cryo_ref = cryo_ref;
                this.write(range, priority, lock_state)
            })
            .await
            .unwrap();

            // Shorten `'static` to `'a`
            Box::new(guard) as _
        }
        .boxed()
    }
}

impl<Element, Priority, RawMutex> DynAsyncIntervalRwLock
    for SliceIntervalRwLock<
        Vec<Element>,
        Element,
        raw::future::AsyncRawRbTreeIntervalRwLock<RawMutex, usize, Priority>,
    >
where
    Self: Send + Sync + 'static,
    RawMutex: lock_api::RawMutex + Send + Sync + 'static,
    Priority: Ord,
    Element: Send + Sync + 'static,
    Priority: Send + Sync + 'static,
{
    type Element = Element;
    type Priority = Priority;
    type ReadLockState = <
    	raw::future::AsyncRawRbTreeIntervalRwLock<RawMutex, usize, Priority>
    	as RawAsyncIntervalRwLock
    >::ReadLockState;
    type WriteLockState = <
    	raw::future::AsyncRawRbTreeIntervalRwLock<RawMutex, usize, Priority>
    	as RawAsyncIntervalRwLock
    >::WriteLockState;

    fn new(inner: Vec<Element>) -> Self {
        Self::new(inner)
    }

    fn is_cancellable(&self) -> bool {
        true
    }

    fn async_read<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: Self::Priority,
        lock_state: Pin<&'a mut Self::ReadLockState>,
    ) -> BoxFuture<'a, Box<dyn Deref<Target = [Self::Element]> + Send + Sync + 'a>> {
        async move { Box::new(self.async_read(range, priority, lock_state).await) as _ }.boxed()
    }

    fn async_write<'a>(
        self: Pin<&'a Self>,
        range: Range<usize>,
        priority: Self::Priority,
        lock_state: Pin<&'a mut Self::WriteLockState>,
    ) -> BoxFuture<'a, Box<dyn DerefMut<Target = [Self::Element]> + Send + Sync + 'a>> {
        async move { Box::new(self.async_write(range, priority, lock_state).await) as _ }.boxed()
    }
}

/// Generate tests that uses [`DynAsyncIntervalRwLock`]
macro_rules! gen_blockingish_test {
    ($modname:ident, $ty:ty) => {
        mod $modname {
            use super::*;

            #[tokio::test]
            async fn stress() {
                blockingish::stress::<$ty>().await;
            }
        }
    };
}

gen_blockingish_test!(blockingish_sync, SyncRbTreeVecIntervalRwLock<AtomicUsize>);
gen_blockingish_test!(
    blockingish_async,
    AsyncRbTreeVecIntervalRwLock<RawMutex, AtomicUsize>
);

mod blockingish {
    use super::*;

    pub(super) async fn stress<
        IntervalRwLock: DynAsyncIntervalRwLock<Element = AtomicUsize, Priority = ()>,
    >() {
        let vec = (0..256).map(|_| AtomicUsize::new(0)).collect();
        let rwl = Arc::pin(IntervalRwLock::new(vec));

        const EXCLUSIVE: usize = usize::MAX / 2 + 1;

        let until = Instant::now() + Duration::from_secs(1);

        let joiners: Vec<_> = (0..16)
            .map(|tid| {
                let rwl = rwl.clone();
                tokio::spawn(async move {
                    let rwl = Pin::as_ref(&rwl);
                    while until > Instant::now() {
                        let mut rng = rand::rngs::StdRng::from_rng(rand::thread_rng()).unwrap();
                        let start_i = rng.gen_range(0..256);
                        let end_i = rng.gen_range(start_i + 1..=256);
                        let writing = rng.gen_bool(0.5);

                        if writing {
                            state!(let mut state);
                            log::trace!("[{}] start write {:?}", tid, start_i..end_i);
                            let guard = rwl.async_write(start_i..end_i, (), state).await;

                            for e in guard.iter() {
                                assert_eq!(
                                    e.swap(EXCLUSIVE, Ordering::Relaxed),
                                    0,
                                    "expected exclusive ownership"
                                );
                            }

                            tokio::task::yield_now().await;

                            for e in guard.iter() {
                                e.store(0, Ordering::Relaxed);
                            }

                            log::trace!("[{}] end write {:?}", tid, start_i..end_i);
                        } else {
                            state!(let mut state);
                            log::trace!("[{}] start read {:?}", tid, start_i..end_i);
                            let guard = rwl.async_read(start_i..end_i, (), state).await;

                            for e in guard.iter() {
                                assert_ne!(
                                    e.fetch_add(1, Ordering::Relaxed),
                                    EXCLUSIVE,
                                    "expected the lack of exclusive ownership"
                                );
                            }

                            tokio::task::yield_now().await;

                            for e in guard.iter() {
                                e.fetch_sub(1, Ordering::Relaxed);
                            }

                            log::trace!("[{}] end read {:?}", tid, start_i..end_i);
                        }
                    }
                })
            })
            .collect();

        for joiner in joiners {
            joiner.await.unwrap();
        }
    }
}

#[tokio::test]
async fn async_cancel() {
    let rwlock = AsyncRbTreeVecIntervalRwLock::<RawMutex, _>::new(vec![0; 16]);
    pin_mut!(rwlock);
    let rwlock = Pin::as_ref(&rwlock);

    state!(let mut state);
    let _guard = rwlock.async_write(5..12, (), state).await;

    state!(let mut state);
    let mut blocked_attempt = rwlock.async_read(0..15, (), state).fuse();

    let timeout = tokio::time::sleep(Duration::from_millis(200));

    // Check that `blocked_attempt` won't complete
    tokio::select! {
        _ = timeout => {}
        _ = &mut blocked_attempt => unreachable!(),
    }

    // Although `blocked_attempt` is incomplete, `0..5` should be locked
    // by `blocked_attempt`. So the following lock attempt will fail
    state!(let mut state);
    rwlock.try_write(0..3, state).err().unwrap();

    // Now cancel `blocked_attempt`
    drop(blocked_attempt);

    // Since `0..5` is now unlocked, the following attempt will succeed
    state!(let mut state);
    rwlock.try_write(0..3, state).unwrap();
}
