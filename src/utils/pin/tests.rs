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
