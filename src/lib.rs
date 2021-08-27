#![doc = include_str!("../README.md")]
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![feature(const_fn_trait_bound)]
#![feature(array_methods)]

mod core;
pub mod hl;
pub mod raw;
mod utils {
    pub mod panicking;
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
