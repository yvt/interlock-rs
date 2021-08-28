use core::{
    fmt,
    future::Future,
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll},
};
use pin_utils::pin_mut;

pub trait EarlyDrop {
    /// Early-destructor. Called on the referent when a [`EarlyDropGuard`] is
    /// dropped.
    ///
    /// Since this method takes `&self`, the existence of an aliasing reference
    /// doesn't cause an undefined behavior.
    ///
    /// # Safety
    ///
    /// This must be the last use of `*self` before `Self::drop`.
    #[inline]
    unsafe fn early_drop(self: Pin<&Self>) {}
}

/// When dropped, magically calls `<T as `[`EarlyDrop`]`>::`[`early_drop`] on
/// the inner object through a shared reference without creating an intermediate
/// mutable reference.
///
/// [`early_drop`]: EarlyDrop::early_drop
#[pin_project::pin_project]
pub struct EarlyDropGuard<T> {
    /// The `Future` holding the inner object.
    #[pin]
    holder: Option<Holder<T>>,
    /// A pointer to [`Holder`]'s local variable holding the inner object.
    storage: Option<NonNull<T>>,
}

unsafe impl<T: Send> Send for EarlyDropGuard<T> {}
unsafe impl<T: Sync> Sync for EarlyDropGuard<T> {}

/// The `Future` stored in `EarlyDropGuard` that provides a storage for `T`
/// from its local variable. When dropped (cancelled), it calls
/// `T::`[`early_drop`] *before* finally dropping the `T`.
///
/// [`early_drop`]: EarlyDrop::early_drop
type Holder<T> = impl Future<Output = !>;

impl<T> EarlyDropGuard<T> {
    /// Construct an `EarlyDropGuard`. The created `EarlyDropGuard` initially
    /// doesn't contain the inner object.
    pub const fn new() -> Self {
        Self {
            holder: None,
            storage: None,
        }
    }

    #[inline]
    pub fn get(&self) -> Option<Pin<&T>> {
        Self::get_inner(self.storage)
    }

    /// `storage` must be `EarlyDropGuard::storage`.
    #[inline]
    fn get_inner<'a>(storage: Option<NonNull<T>>) -> Option<Pin<&'a T>> {
        storage.map(|storage| {
            // Safety: `storage` points to a local variable of the still-running
            // `Future` `self.holder`. The `Future` wouldn't even run if `self`
            // wasn't pinned. Also, `storage` becomes immutable once being set.
            unsafe { Pin::new_unchecked(storage.as_ref()) }
        })
    }
}

impl<T: EarlyDrop> EarlyDropGuard<T> {
    /// Get a reference to the inner object, creating one with `init` if it
    /// hasn't been created yet.
    pub fn get_or_insert_with(self: Pin<&mut Self>, init: impl FnOnce() -> T) -> Pin<&T> {
        let mut this = self.project();

        if let Some(inner) = Self::get_inner(*this.storage) {
            return inner;
        }

        let p_storage = &mut *this.storage as *mut Option<NonNull<T>>;

        assert!(this.holder.is_none());

        // Work-around for <https://github.com/rust-lang/rust/issues/65442>
        // (The compiler incorrectly considers that `init`'s type is part of the
        // `async` block's concrete type (= `Holder<T>`) even if `init` is
        // actually not moved into the `async` block.)
        #[inline]
        fn make_holder<T: EarlyDrop>(p_storage: *mut Option<NonNull<T>>, init: T) -> Holder<T> {
            async move {
                let inner = init;
                pin_mut!(inner);

                // The RAII guard to call `early_drop` on call
                let guard = DoEarlyDropOnDrop(Pin::as_ref(&inner));

                // Provide access to `inner`
                // Safety: `p_storage` is still valid. In fact, this line executes
                // while the containing method is still running.
                unsafe { *p_storage = Some(NonNull::from(&*guard.0)) };

                // Stall this future indefinitely. Thus `inner` will remain
                // available for shared access until this future is dropped.
                loop {
                    core::future::pending::<!>().await;
                }
            }
        }

        this.holder.set(Some(make_holder(p_storage, init())));
        let mut holder = this.holder.as_pin_mut().unwrap();

        // Run `holder` until it gives us `NonNull<T>`
        loop {
            match Pin::as_mut(&mut holder)
                .poll(&mut Context::from_waker(&futures::task::noop_waker()))
            {
                Poll::Ready(never) => match never {},
                Poll::Pending => {}
            }

            if let Some(inner) = Self::get_inner(*this.storage) {
                return inner;
            }
        }
    }

    /// Get a reference to the inner object, creating one with
    /// [`Default::default`] if it hasn't been created yet.
    pub fn get_or_insert_default(self: Pin<&mut Self>) -> Pin<&T>
    where
        T: Default,
    {
        self.get_or_insert_with(T::default)
    }
}

struct DoEarlyDropOnDrop<'a, T: EarlyDrop>(Pin<&'a T>);

impl<T: EarlyDrop> Drop for DoEarlyDropOnDrop<'_, T> {
    #[inline]
    fn drop(&mut self) {
        unsafe { self.0.early_drop() };
    }
}

impl<T: fmt::Debug> fmt::Debug for EarlyDropGuard<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(inner) = self.get() {
            fmt::Debug::fmt(&inner, f)
        } else {
            f.write_str("< uninitialized >")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate std;
    use std::{cell::Cell, prelude::v1::*};

    #[test]
    fn early_drop_called() {
        struct DropCheck<'a>(&'a Cell<usize>, &'a mut bool);

        impl EarlyDrop for DropCheck<'_> {
            unsafe fn early_drop(self: Pin<&Self>) {
                assert_eq!(self.0.get(), 1, "early_drop was called too early");
                self.0.set(2);
            }
        }

        impl Drop for DropCheck<'_> {
            fn drop(&mut self) {
                assert_eq!(self.0.get(), 2, "early_drop wasn't called");
                self.0.set(3);

                assert_eq!(*self.1, false, "drop was called twice");
                *self.1 = true;
            }
        }

        let step = Cell::new(0);
        let mut dropped = false;
        let mut edg = Box::pin(EarlyDropGuard::new());

        Pin::as_mut(&mut edg).get_or_insert_with(|| DropCheck(&step, &mut dropped));
        Pin::as_mut(&mut edg)
            .get_or_insert_with(|| unreachable!())
            .0
            .set(1);

        drop(edg);

        assert_eq!(step.get(), 3, "drop wasn't called");
        assert_eq!(dropped, true, "drop wasn't called");
    }
}
