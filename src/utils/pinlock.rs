//! [`lock_api`] + [`Pin`]
use core::{fmt, pin::Pin};
use lock_api::{Mutex, MutexGuard, RawMutex};

pub struct PinMutex<R, T: ?Sized> {
    inner: Mutex<R, T>,
}

impl<R, T> PinMutex<R, T> {
    #[inline]
    pub const fn const_new(raw_mutex: R, val: T) -> Self {
        Self {
            inner: Mutex::const_new(raw_mutex, val),
        }
    }
}

impl<R: RawMutex, T> PinMutex<R, T> {
    #[inline]
    pub fn lock(&self) -> Pin<MutexGuard<'_, R, T>> {
        let guard = self.inner.lock();
        unsafe { Pin::new_unchecked(guard) }
    }
}

impl<R: RawMutex, T: ?Sized + fmt::Debug> fmt::Debug for PinMutex<R, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
    }
}
