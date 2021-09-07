# interlock

*Work in progress*

Readers-writer locks optimized for locking intervals. `#![no_std]` compatible.

```rust
use std::pin::Pin;
use interlock::{hl::slice::SyncRbTreeSliceIntervalRwLock, state};

let vec = vec![0u8; 64];

let vec = Box::pin(SyncRbTreeSliceIntervalRwLock::new(vec));
let vec = Pin::as_ref(&vec);

// Borrow `vec[0..32]`
state!(let mut state);
let guard1 = vec.read(0..32, (), state);

// Borrow `vec[16..32]`
state!(let mut state);
let guard2 = vec.read(16..32, (), state);

// Mutably borrow `vec[16..48]` unsuccessfully
state!(let mut state);
vec.try_write(16..48, Pin::as_mut(&mut state)).unwrap_err();

// Unborrow `vec[0..32]` completely
drop(guard1);
drop(guard2);

// Mutably borrow `vec[16..48]`
vec.try_write(16..48, Pin::as_mut(&mut state)).unwrap();
```

## Cargo features

 - **`std`** enables the items that depend on `std` or `alloc`.
 - **`async`** enables the `Future`-oriented API. This currently requires a target with load/store atomics support. When [`lock_api` issue #277][1] is resolved, this requirement will be lifted, and this Cargo feature will be deprecated.

[1]: https://github.com/Amanieu/parking_lot/issues/277