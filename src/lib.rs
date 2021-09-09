#![doc = include_str!("../README.md")]
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::needless_return)] // <https://github.com/rust-lang/rust-clippy/issues/7637>
#![cfg_attr(feature = "doc_cfg", feature(doc_cfg))]
#![feature(array_methods)]
#![feature(never_type)]
#![feature(type_alias_impl_trait)]
#![feature(const_impl_trait)]
#![feature(slice_ptr_len)]
#![feature(slice_ptr_get)]
#![feature(once_cell)]
#![feature(let_else)] // <https://github.com/rust-lang/rust/issues/87335>

#[cfg(any(test, feature = "std"))]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

/// Used by [`state!`].
#[doc(hidden)]
pub use pin_utils::pin_mut;

#[cfg(doc)]
#[doc = include_str!("../CHANGELOG.md")]
pub mod _changelog_ {}

#[macro_use]
mod macros;

mod core;
pub mod hl;
pub mod raw;
mod utils {
    pub mod panicking;
    #[cfg(not(miri))]
    pub mod pin;
    #[cfg(miri)]
    #[path = "pin_boxed.rs"]
    pub mod pin;
    #[cfg(feature = "async")]
    pub mod pinlock;
    #[cfg(feature = "std")]
    pub mod pinsync;
    pub mod rbtree;
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
