use core::{ops, pin::Pin};

/// The equivalent of `DerefMut` for `!Unpin` types.
///
/// The target is unpinned. It doesn't override dereference operations (like
/// `*smart_ptr`).
pub trait PinDerefMut: ops::Deref {
    fn pin_deref_mut(self: Pin<&mut Self>) -> &mut Self::Target;
}

impl<T: ?Sized> PinDerefMut for &mut T {
    #[inline]
    fn pin_deref_mut(self: Pin<&mut Self>) -> &mut Self::Target {
        *Pin::into_inner(self)
    }
}
