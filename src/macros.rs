/// Define a pinned, default-initialized local variable of type
/// [`Pin`]`<&mut _>`.
///
/// This macro is useful for holding the state data of a lock. See
/// [the crate-level documentation](crate) for an example.
///
/// [`Pin`]: core::pin::Pin
#[macro_export]
macro_rules! state {
    (let mut $var:ident $(: $ty:ty)? ) => {
        let $var $(: $ty)? = Default::default();
        $crate::pin_mut!($var);
    };
}

/// Gate items by `#[cfg(feature = "alloc")]`.
macro_rules! if_alloc {
    ($($tt:tt)*) => {
        cfg_if::cfg_if! {
            if #[cfg(feature = "alloc")] {
                $($tt)*
            }
        }
    };
}
