use core::{fmt, marker::PhantomPinned, ops::Deref};

/// Wraps `&T` where `T: `[`EarlyDrop`]. By using this as `Pin<&Guard<T>>`,
/// it can be enforced that [`EarlyDrop::early_drop`] is always called on `T`.
#[deprecated = "`Guard` is useless because it doesn't do what it claims to do"]
pub struct Guard<'a, T: EarlyDrop> {
    inner: &'a T,
    /// Ensures `Guard`'s destructor is called.
    _pin: PhantomPinned,
}

pub trait EarlyDrop {
    /// Quasi-destructor. Called on the referent when a [`Guard`] is dropped.
    ///
    /// Since this method takes `&self`, the existence of an aliasing reference
    /// doesn't cause an undefined behavior. However, this method may be called
    /// more than once for each instance.
    #[inline]
    fn early_drop(&self) {}
}

impl<'a, T: EarlyDrop> Guard<'a, T> {
    /// Construct a `Guard`.
    ///
    /// # Safety
    ///
    /// `Pin<&Guard<T>>` actually doesn't enforce that `early_drop` is always
    /// called on `T`. See the following example.
    ///
    /// ```rust,ignore
    /// let storage: MyType;
    /// let storage_ptr  = (&storage) as *const MyType;
    /// let guard = Box::pin(Guard::new(&storage));
    ///
    /// std::mem::forget(guard);
    /// drop(storage);
    ///
    /// // `Guard` still exists (meaning `early_drop` isn't called yet), but
    /// // the target is gone, so the following line causes a UB
    /// unsafe { (*storage_ptr).hoge() };
    /// ```
    #[inline]
    pub const unsafe fn new(inner: &'a T) -> Self {
        Self {
            inner,
            _pin: PhantomPinned,
        }
    }
}

impl<T: EarlyDrop> Drop for Guard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.early_drop();
    }
}

impl<T: EarlyDrop> Deref for Guard<'_, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl<T: EarlyDrop + fmt::Debug> fmt::Debug for Guard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.inner, f)
    }
}
