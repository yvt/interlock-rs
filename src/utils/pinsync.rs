use std::{
    cell::UnsafeCell,
    fmt, ops,
    pin::Pin,
    sync::{Mutex, MutexGuard as StdMutexGuard, OnceLock, TryLockError},
};

#[pin_project::pin_project]
pub struct PinMutex<T> {
    mutex: OnceLock<Mutex<()>>,
    payload: UnsafeCell<T>,
}

// Safety:
unsafe impl<T: Send> Sync for PinMutex<T> {}
unsafe impl<T: Send> Send for PinMutex<T> {}

impl<T> PinMutex<T> {
    #[inline]
    pub const fn new(x: T) -> Self {
        Self {
            mutex: OnceLock::new(),
            payload: UnsafeCell::new(x),
        }
    }

    #[inline]
    pub fn lock(self: Pin<&Self>) -> Pin<MutexGuard<'_, T>> {
        let this = self.get_ref();
        let inner = this.mutex.get_or_init(|| Mutex::new(())).lock().unwrap();

        // Safety: `self.payload` is pinned, so `*mutex_guard` is pinned, too
        unsafe {
            Pin::new_unchecked(MutexGuard {
                _inner: inner,
                ptr: this.payload.get(),
            })
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for PinMutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("PinMutex");
        let mutex = self.mutex.get_or_init(|| Mutex::new(()));
        match mutex.try_lock() {
            Ok(_guard) => {
                d.field("data", unsafe { &*self.payload.get() })
                    .field("poisoned", &false);
            }
            Err(TryLockError::Poisoned(_err)) => {
                d.field("data", unsafe { &*self.payload.get() })
                    .field("poisoned", &true);
            }
            Err(TryLockError::WouldBlock) => {
                struct LockedPlaceholder;
                impl fmt::Debug for LockedPlaceholder {
                    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("<locked>")
                    }
                }
                d.field("data", &LockedPlaceholder);
            }
        }
        d.finish_non_exhaustive()
    }
}

pub struct MutexGuard<'a, T> {
    _inner: StdMutexGuard<'a, ()>,
    ptr: *mut T,
}

impl<T> ops::Deref for MutexGuard<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        // Safety: The uniqueness of the reference is guaranteed by
        //`PinMutex::mutex`
        unsafe { &*self.ptr }
    }
}

impl<T> ops::DerefMut for MutexGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: The uniqueness of the reference is guaranteed by
        //`PinMutex::mutex`
        unsafe { &mut *self.ptr }
    }
}
