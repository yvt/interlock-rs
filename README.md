# 🌎🔒 interlock

[<img src="https://docs.rs/interlock/badge.svg" alt="docs.rs">](https://docs.rs/interlock/)

*Experimental, requires unstable compiler features*

[Readers-writer locks][5] designed for locking intervals. `#![no_std]` compatible.

```rust
use std::pin::Pin;
use interlock::{hl::slice::SyncRbTreeVecIntervalRwLock, state};

let vec = Box::pin(SyncRbTreeVecIntervalRwLock::new(vec![0u8; 64]));
let vec = vec.as_ref();

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

```rust
use parking_lot::RawMutex;
use interlock::{hl::slice::AsyncRbTreeVecIntervalRwLock, state};

#[tokio::main]
async fn main() {
    let vec = Box::pin(AsyncRbTreeVecIntervalRwLock::<RawMutex, _>::new(vec![0u8; 64]));
    let vec = vec.as_ref();

    state!(let mut state);
    let _guard = vec.async_read(0..32, (), state).await;

    state!(let mut state);
    let _guard = vec.async_read(16..48, (), state).await;
}
```

[5]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock

## Design

### Red-black tree based implementation

|       Locking Interface       |  Storage   |                        Type Alias                       |
|-------------------------------|------------|---------------------------------------------------------|
| Fallible + Panicking, `!Sync` | `&mut [T]` | [`crate::hl::slice::LocalRbTreeSliceRefIntervalRwLock`] |
| ↑                             | `Vec<T>`   | [`crate::hl::slice::LocalRbTreeVecIntervalRwLock`]      |
| Fallible + Blocking           | `&mut [T]` | [`crate::hl::slice::SyncRbTreeSliceRefIntervalRwLock`]  |
| ↑                             | `Vec<T>`   | [`crate::hl::slice::SyncRbTreeVecIntervalRwLock`]       |
| Fallible + `Future`-oriented  | `&mut [T]` | [`crate::hl::slice::AsyncRbTreeSliceRefIntervalRwLock`] |
| ↑                             | `Vec<T>`   | [`crate::hl::slice::AsyncRbTreeVecIntervalRwLock`]      |

They are modeled as an array of virtual readers-writer locks. When locking, the virtual locks in the specified range are locked in ascending order. When unlocking, they are unlocked in descending order. (The locking order is important to prevent deadlocks.) The wait list of each virtual lock is ordered by `(priority, sequence)`, where `sequence` is a monotonically increasing number (thus enforcing FIFO ordering). When a virtual lock is unlocked, all entries in the wait list are examined and resumed instantly (if possible) in order.

On a lock conflict, the fallible interface fails and unlocks the already borrowed range, rolling back the lock to the original state. Other interfaces maintain the incomplete borrow until it's cancelled or completed by the removal of a conflicting borrow.

The `Future`-oriented interface supports cancellation.

The worst-case time complexity is shown below:

| Operation |                        Time Complexity                         |
|-----------|----------------------------------------------------------------|
| Locking   | `O(log(existing_borrows))`                                     |
| Unlocking | `O((1 + potentially_resumed_borrows) * log(existing_borrows))` |

The space complexity is `O(existing_borrows)`.

## Cargo features

 - **`std`** enables the items that depend on `std` or `alloc`.
 - **`alloc`** enables the items that depend on `alloc`. This currently requires a target with `usize` atomics support because `stable_deref_trait/alloc` unconditionally implements `StableDeref` on `Arc`, which is gated by `cfg(target_has_atomic = "ptr")`.
 - **`async`** enables the `Future`-oriented API. This currently requires a target with load/store atomics support. When [`lock_api` issue #277][6] is resolved, this requirement will be lifted, and this Cargo feature will be deprecated.

[6]: https://github.com/Amanieu/parking_lot/issues/277

## Future directions

 - More thorough testing is probably required with respect to untrusted trait `impl`s and various kinds of safety.
 - The `Mutex` and `ReentrantMutex` counterparts should be provided in addition to that of `RwLock`.
 - Variants with simpler algorithms, such as linked lists, should be provided.
 - The dependency on unstable compiler features should preferably be removed at some point.
 - The hack for sound intrusive data structures (`EarlyDropGuard`) should be removed when Rust's memory model is adjusted to properly accommodate such structures.

## Alternatives

`interlock` is surprisingly niche. While it may scale well for large-scale inputs, its complexity and overhead are likely to outweigh the benefit in many practical situations. Consider the following alternatives before choosing `interlock`:

 - **Dividing slices:** The language and the standard library provide many ways to divide `&mut [T]`. For example:
    - Pattern matching (e.g., `if let [first, rest @ ..] ...`) can separate the first or last few elements from the rest of a given slice.
    - [`slice::iter_mut`] creates a set of `&mut T`, which can be used separately.
    - [`slice::chunks_mut`] breaks `&mut [T]` into chunks.
    - [`slice::split_at_mut`] divides `&mut [T]` into two. An arbitrary number of pieces can be created by repeating this process.

 - **No ordering requirements, no reference forming:** If borrows can be unordered in respect to other borrows, getting access to `&[T]` or `&mut [T]` is not necessary, and...
     - **Single-threaded:** ... the slice `[T]` is only referenced by a single thread, then consider wrapping individual elements with [`Cell`]`<T>`. You can turn `&mut [T]` into `&[Cell<T>]` by [`Cell::as_slice_of_cells`].
     - **Integers:** ... the elements are of a primitive integer type, then consider changing the element type to [`std::sync::atomic`]`::Atomic*` and accessing them with `Ordering::Relaxed`.

 - **Short-lived borrows:** If each borrow is expected to be short-lived, consider wrapping the whole slice with a [`Mutex`] or [`RwLock`]. `interlock` uses a `Mutex` to protect internal structures, so it incurs the overhead of `Mutex` *plus* bookkeeping.

 - **Parallel data processing:** Consider using [Rayon][1] for parallel computation.

 - **Per-element or per-segment borrows:** If you only need to borrow individual elements or segments (i.e., if it's acceptable to get `&mut T` or `&mut [T; SEGMENT_LEN]` instead of a more general, contiguous `&mut [T]`), consider wrapping individual elements or segments with [`parking_lot`][2]`::{Mutex, RwLock}`, which only take one byte and one word of space, respectively. In some cases, [`spin`][4] might be a more preferable choice.
    - Suppose you replace `N` borrows from `Mutex` with one borrow from `SyncRbTreeSliceIntervalRwLock`. While the latter's locking time doesn't depend on `N`, due its inherent complexity, if `N` is too small, you might actually lose performance. According to some single-threaded micro-benchmark conducted on an AMD Zen processor, the threshold seems to be somewhere around `N = 6`. Expect a higher threshold in a real application because of cache effects and lock contention.

 - **Single processor, CPU-bound:** If only one processor accesses the slice simultaneously, and it's expected that the processor is fully occupied while the slice is borrowed, consider wrapping the whole slice with a `Mutex` or `RwLock`. In such cases, being able to borrow disjoint subslices simultaneously doesn't improve the overall throughput.

[1]: https://crates.io/crates/rayon
[2]: https://docs.rs/parking_lot/0.11.2/parking_lot/
[3]: https://docs.rs/rayon/1.5.1/rayon/fn.scope.html
[4]: https://crates.io/crates/spin
[`Cell`]: std::cell::Cell
[`Cell::as_slice_of_cells`]: std::cell::Cell::as_slice_of_cells
[`Mutex`]: std::sync::Mutex
[`RwLock`]: std::sync::RwLock
