#![doc = include_str!("../README.md")]
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::needless_return)] // <https://github.com/rust-lang/rust-clippy/issues/7637>
#![feature(const_fn_trait_bound)]
#![feature(array_methods)]
#![feature(never_type)]
#![feature(type_alias_impl_trait)]
#![feature(const_impl_trait)]
#![feature(slice_ptr_len)]
#![feature(slice_ptr_get)]
#![feature(let_else)] // <https://github.com/rust-lang/rust/issues/87335>

#[cfg(test)]
extern crate std;

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
    pub mod rbtree;
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
