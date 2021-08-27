/// Call the specified closure. Abort if it panics.
///
/// Our RB tree implementation leaves a tree in an indeterminate state
/// (specifically, it might or might not leave the node in question in the tree;
/// this is very problematic for intrusive data structures) when provided
/// callbacks panic.
#[inline]
pub fn abort_on_unwind<R>(f: impl FnOnce() -> R) -> R {
    struct Aborter;
    impl Drop for Aborter {
        #[inline]
        fn drop(&mut self) {
            panic!("can't maintain safety after unwinding");
        }
    }

    let aborter = Aborter;
    let ret = f();
    core::mem::forget(aborter);

    ret
}
