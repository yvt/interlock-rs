use quickcheck_macros::quickcheck;

use std::prelude::v1::*;

type Index = usize;
type Priority = u64;
type InProgress = LockId;

type IsWrite = bool;

const LEN: usize = 32;

// Reference Implementation
// --------------------------------------------------------------------------

// ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⢀⣀⣀⣠⣀⣄⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⢀⣀⣀⣠⣤⣤⣄⣤⣠⣤⣠⣤⣠⣄⣤⣄⣤⣤⣠⣤⣠⣄⣤⣄⣤⣠⣄⣤⣠⣄⣤⣠⣄⣤⣠⣠⣠⣠⣠⣠⣠⣠⣠
// ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠝⡎⠆⡋⡻⣻⢿⣻⢟⣿⣻⢯⡿⣯⡿⣽⣟⣿⢿⣻⣷⣷⣷⣿⣮⠑⣻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠂⠌⡌⡣⣳⣹⣪⣾⣽⣽⣯⣿⣷⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠫⠀⠀⠘⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⢜⣮⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢿⠿⠿⠿⡿⣋⣔⣤⡢⡻⢿⢿⣿⣿⣿⣿⣿⡿⠟⠉⠀⠀⠀⠀⠀⢻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⠀⠀⠀⠀⠀⢀⣤⡤⣄⣄⣄⣤⣤⣶⣵⣿⣿⣿⣿⣿⡿⠿⠻⠛⠛⠋⠉⠉⠈⡀⠠⣀⣢⣢⣷⣿⣿⣾⡪⡊⠨⡐⠕⢜⢜⢒⢆⢂⠀⠀⠀⠀⠀⠀⠀⠀⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⠀⠀⠀⠀⠀⠃⣿⠇⠀⠀⠀⣠⡤⣌⣌⣍⣉⠉⠙⣿⠇⠀⠀⠀⠀⠀⢀⠔⡡⡢⣿⣿⣿⣿⣿⢿⣿⡿⡣⠀⡪⣸⣼⣜⠔⠡⢱⢱⢡⢄⠀⠀⠀⠀wow⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⠐⠀⠀⠀⠀⢐⣿⠁⠀⠀⠀⠈⠈⠉⠈⠉⠑⠀⢠⣿⣅⣄⣠⣠⣤⣶⣵⣷⣷⣿⣿⣿⣿⠁⠀⠀⢹⣱⣔⣔⢽⡻⠵⠛⠎⢎⠔⡡⡣⡳⡳⡦⡄⠀⠀⠀⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⠀⠀⠀⠀⢀⣠⡿⠀⠀⠀⢄⢔⠤⡀⠀⠀⡰⡰⣺⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣄⣠⣴⣽⣷⣿⢎⠀⠀⠀⠀⠀⠀⢀⣇⣗⡽⣜⡵⡝⡆⡀⠀⢸⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⢀⢴⡟⠯⢬⢾⠃⠀⠀so much allocation⣿⣿⣿⣿⡿⠿⠛⠛⠻⣿⣿⣿⣳⢔⣄⣀⡀⡠⡴⣿⣿⣿⣿⣿⣿⣟⣮⣖⢌⢎⣿⣿⣿⣿⣿⣿⣿⣿⣿OMG⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⡀⣀⣁⣅⣀⣀⣻⣿⣿⣑⢔⢈⠌⠂⢷⣫⣖⡮⣕⣿⢥⠛⠿⣦⡀⡉⣟⣿⣿⣿⣿⠀⠀⠀⠀⠀⠈⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢿⣻⠪⠣⡫⡾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⣿⣿⣿⣿⣿⣿⣿⣿⣯⣿⣿⣿⣟⣿⣳⣟⢿⠷⣏⠣⡈⢠⣤⣺⣟⢯⡳⣿⣿⣿⡇⠀⠀⠀⠀⠀⠀⡻⡛⣗⠽⡿⡿⣿⣿⣿⣿⣿⢿⢟⢯⣫⣳⠪⠡⠁⡇⢝⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣾⣷⣼⣼⣵⣽⣿⣿⣿⣷⡀⠀⠀⠀⠀⠀⠈⠊⠂⠁⢈⡻⣪⣺⢼⣞⢮⣷⣿⣿⡿⡮⡣⠡⡑⡐⠅⣟⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣷⣄⡀⠀⠀⠀⠀⠀⢀⢠⢪⡺⡜⡎⣟⣾⣽⡾⣵⡳⢝⠜⠨⠐⠀⠀⠁⠆⡿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣻⢽⡕⡽⡸⡱⡑⡕⣜⣼⣯⣷⡯⡯⡻⡪⣪⠢⡁⠊⠀⠀⢀⠜⡨⣺⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⣿⡿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢟⣞⡷⣹⢜⢜⢜⢜⢜⢜⠪⡓⡗⣝⢮⢯⢳⢕⢕⢡⢀⡀⡆⣇⣗⣽⢾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⡷⣟⡯⣷⣻⣻⣟⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣳⢕⡁⡃⡃⡁⡁⡂⢅⢢⣱⢼⢼⣪⣗⢗⣝⡮⣪⣦⣾⣾⣽⣷⣿⣽⣻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⡯⣗⢯⢗⢯⢞⡾⡵⣗⡯⡯⣟⣿⣻⡿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣷⢳⡩⡲⡨⡨⡢⣣⣓⣟⢾⢽⣞⡮⣮⣛mallolc/free cometh⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⡇⣗⢝⢎⡗⣝⢎⢯⢺⡺⣝⢵⡳⣳⡻⣽⡺⣽⣻⡻⡿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢳⡱⣨⢢⡑⡕⡧⣗⡏⣗⡯⣶⢻⢯⣿⣵⣾⣿⣿⣿⣿⣿⣿⡷⡓⠍⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⡇⡇⡏⡮⡺⡸⡱⡹⡪⡪⣪⢣⡫⡺⡜⡮⢮⢳⢕⢯⣫⣳⣵⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢯⣗⢧⢣⡣⣳⢵⢳⢕⣯⣳⣻⡺⡻⡻⣽⣿⣷⣿⣿⣿⣿⢟⢝⢊⢊⢀⢻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⢕⢕⢕⢕⠕⡝⢜⢜⢎⢣wow⢣⢣⠣⡣⠣⡣⡣⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⡕⡎⡮⡪⡞⡗⣝⡯⣯⣗⣯⡷⣿⣻⣷⣽⢽⣿⣿⣿⢿⣮⣯⡧⡂⢀⠀⢯⢿⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
// ⠕⡡⠣⡑⢅⠣⡑⢕⢘⠌⠆⡣⢑⠡⠡⠑⠨⠈⠂⠌⡚⡻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⢯⡫⣎⢎⢇⢗⢝⢜⢎⢗⢗⢽⡺⡵⡫⡺⣝⣟⣞⣷⢿⡿⣿⣾⣿⡪⠀⢈⢪⢫⢯⡺⣝⢝⡮⣗⢯⡺⣺⢽⢝⡯⣟⢽⡫⡯⡯⡯
// ⠈⠄⠁⡂⠂⠅⠌⠐⠐⠨⠐⠀⠂⠈⠀⠁⠀⠠⢡⠳⢝⢗⢿⣿⣿⣿⣿⣿⣿⠿⠟⠯⡫⡺⡸⡪⡪⡣⡳⡱⡱⢱⢱⢣⢝⢎⢽⢺⢜⠮⣞⢯⣟⡿⣿⣿⣿⠇⠂⠀⢂⠣⠣⠣⡣⡣⡓⡕⡕⡝⡜⡵⡹⡪⠮⡳⡹⡹⡪⢯

mod refr {
    use super::{Index, IsWrite, LockId, Priority, LEN};

    use std::{
        cmp::Ordering,
        collections::{BTreeSet, HashMap, HashSet},
        fmt,
        ops::Range,
        prelude::v1::*,
    };

    #[derive(Default)]
    struct RwLock {
        owners: HashSet<(LockId, IsWrite)>,
        waits: BTreeSet<Wait>,
        next_order: u64,
    }

    #[derive(PartialEq, Eq)]
    struct Wait {
        id: LockId,
        write: IsWrite,
        priority: Priority,
        /// Enforce FIFO ordering
        order: u64,
    }

    impl PartialOrd for Wait {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
            Some(self.cmp(o))
        }
    }

    impl Ord for Wait {
        fn cmp(&self, o: &Self) -> Ordering {
            // (-priority, order)
            o.priority
                .cmp(&self.priority)
                .then(self.order.cmp(&o.order))
        }
    }

    struct Lock {
        range: Range<Index>,
        write: IsWrite,
        pos: Index,
        priority: Priority,
        /// The last copy of `Wait::order`
        order: Option<u64>,
    }

    #[derive(Default)]
    pub struct RwLockSet {
        rwlocks: Vec<RwLock>,
        locks: HashMap<LockId, Lock>,
    }

    impl RwLockSet {
        pub fn new() -> Self {
            Self {
                rwlocks: (0..LEN).map(|_| RwLock::default()).collect(),
                locks: HashMap::new(),
            }
        }

        /// Acquire a lock. Returns whether the lock is complete.
        pub fn lock(
            &mut self,
            id: LockId,
            range: Range<Index>,
            write: IsWrite,
            priority: Priority,
        ) -> bool {
            let lock = Lock {
                range: range.clone(),
                write,
                priority,
                pos: range.start,
                order: None,
            };
            self.locks.insert(id, lock);
            self.resume(id)
        }

        /// Release a lock.
        pub fn unlock(&mut self, id: LockId) {
            let mut lock = self.locks.remove(&id).unwrap();

            if let Some(order) = lock.order.take() {
                let rwl = &mut self.rwlocks[lock.pos];
                assert!(rwl.waits.remove(&Wait {
                    id,
                    write: lock.write,
                    priority: lock.priority,
                    order: order,
                }));
            }

            while lock.pos > lock.range.start {
                lock.pos -= 1;

                let rwl = &mut self.rwlocks[lock.pos];
                rwl.owners.remove(&(id, lock.write));

                // Check for unblocked pending borrows
                let maybe_unblocked_ids: Vec<_> = rwl.waits.iter().map(|w| w.id).collect();
                for id in maybe_unblocked_ids {
                    self.resume(id);
                }
            }
        }

        pub fn is_complete(&self, id: LockId) -> bool {
            let lock = self.locks.get(&id).expect("non-existent lock");
            lock.pos == lock.range.end
        }

        fn resume(&mut self, id: LockId) -> bool {
            let lock = self.locks.get_mut(&id).unwrap();

            if let Some(order) = lock.order.take() {
                let rwl = &mut self.rwlocks[lock.pos];
                assert!(rwl.waits.remove(&Wait {
                    id,
                    write: lock.write,
                    priority: lock.priority,
                    order,
                }));
            }

            while lock.pos < lock.range.end {
                let rwl = &mut self.rwlocks[lock.pos];
                match lock.write {
                    true => {
                        if !rwl.owners.is_empty() {
                            break;
                        }
                    }
                    false => {
                        if rwl
                            .owners
                            .iter()
                            .next()
                            .map_or(false, |&(_, writing_lock)| writing_lock)
                        {
                            break;
                        }
                    }
                }
                rwl.owners.insert((id, lock.write));
                lock.pos += 1;
            }

            if lock.pos < lock.range.end {
                let rwl = &mut self.rwlocks[lock.pos];
                let order = rwl.next_order;
                rwl.next_order += 1;
                lock.order = Some(order);
                rwl.waits.insert(Wait {
                    id,
                    write: lock.write,
                    priority: lock.priority,
                    order,
                });
            }

            lock.pos == lock.range.end
        }
    }

    impl fmt::Debug for RwLockSet {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let borrow_sym = |is_write: Option<IsWrite>| match is_write {
                None => " ",
                Some(true) => "▓",
                Some(false) => "░",
            };

            // Lock state bitmap
            writeln!(f, "Memory bitmap:")?;
            write!(f, "    ")?;
            for x in (0..LEN).step_by(8) {
                write!(f, "{:<8}", x)?;
            }
            writeln!(f, "{}", LEN)?;

            write!(f, "   |")?;
            for rwlock in self.rwlocks.iter() {
                write!(
                    f,
                    "{}",
                    borrow_sym(rwlock.owners.iter().next().map(|&(_, is_write)| is_write))
                )?;
            }
            writeln!(f, "|")?;

            // Locks
            writeln!(f, "Borrows:")?;
            for (id, lock) in self.locks.iter() {
                write!(f, "{:3}:", id)?;

                for _ in 0..lock.range.start {
                    write!(f, " ")?;
                }
                for _ in lock.range.start..lock.pos {
                    write!(f, "{}", borrow_sym(Some(lock.write)))?;
                }
                for _ in lock.pos..lock.range.end {
                    write!(f, "-")?;
                }
                writeln!(f)?;
            }

            Ok(())
        }
    }
}

// Subject
// --------------------------------------------------------------------------

mod subj {

    use super::super::*;
    use super::{InProgress, Index, IsWrite, LockId, Priority};
    use std::{collections::HashMap, prelude::v1::*};

    #[derive(Debug)]
    struct Lock {
        range: Range<Index>,
        priority: Priority,
        state: LockState,
        complete: bool,
    }

    #[derive(Debug)]
    enum LockState {
        Read(Pin<Box<ReadLockState<Index, Priority, InProgress>>>),
        Write(Pin<Box<WriteLockState<Index, Priority, InProgress>>>),
        TryRead(Pin<Box<TryReadLockState<Index, Priority, InProgress>>>),
        TryWrite(Pin<Box<TryWriteLockState<Index, Priority, InProgress>>>),
    }

    pub struct RwLockSet {
        rwlocks: Pin<Box<RbTreeIntervalRwLockCore<Index, Priority, InProgress>>>,
        locks: HashMap<LockId, Lock>,
    }

    impl RwLockSet {
        pub fn new() -> Self {
            Self {
                rwlocks: Box::pin(RbTreeIntervalRwLockCore::new()),
                locks: HashMap::new(),
            }
        }

        /// Acquire a lock. Returns whether the lock is complete.
        pub fn lock(
            &mut self,
            id: LockId,
            range: Range<Index>,
            write: IsWrite,
            priority: Priority,
            is_try: bool,
        ) -> bool {
            let rwlocks = Pin::as_mut(&mut self.rwlocks);

            let (complete, state) = match (write, is_try) {
                (false, false) => {
                    let mut state = Box::pin(ReadLockState::new());
                    let complete = rwlocks.lock_read(
                        range.clone(),
                        priority,
                        Pin::as_mut(&mut state),
                        TestLockCallback(id),
                    );
                    (complete, LockState::Read(state))
                }
                (true, false) => {
                    let mut state = Box::pin(WriteLockState::new());
                    let complete = rwlocks.lock_write(
                        range.clone(),
                        priority,
                        Pin::as_mut(&mut state),
                        TestLockCallback(id),
                    );
                    (complete, LockState::Write(state))
                }
                (false, true) => {
                    let mut state = Box::pin(TryReadLockState::new());
                    let complete = rwlocks.try_lock_read(range.clone(), Pin::as_mut(&mut state));
                    (complete, LockState::TryRead(state))
                }
                (true, true) => {
                    let mut state = Box::pin(TryWriteLockState::new());
                    let complete = rwlocks.try_lock_write(range.clone(), Pin::as_mut(&mut state));
                    (complete, LockState::TryWrite(state))
                }
            };

            if complete || !is_try {
                // Remember the lock
                self.locks.insert(
                    id,
                    Lock {
                        state,
                        range,
                        priority,
                        complete,
                    },
                );
            }

            complete
        }

        /// Release a lock.
        pub fn unlock(&mut self, id: LockId) {
            let mut lock = self.locks.remove(&id).unwrap();
            let rwlocks = Pin::as_mut(&mut self.rwlocks);

            let callback = TestUnlockCallback(&mut self.locks);

            match &mut lock.state {
                LockState::Read(state) => {
                    rwlocks.unlock_read(Pin::as_mut(state), callback);
                }
                LockState::Write(state) => {
                    rwlocks.unlock_write(Pin::as_mut(state), callback);
                }
                LockState::TryRead(state) => {
                    rwlocks.unlock_try_read(Pin::as_mut(state), callback);
                }
                LockState::TryWrite(state) => {
                    rwlocks.unlock_try_write(Pin::as_mut(state), callback);
                }
            }
        }

        pub fn is_complete(&self, id: LockId) -> bool {
            self.locks.get(&id).expect("non-existent lock").complete
        }

        pub fn validate(&self) {
            unsafe {
                rbtree::Node::validate(&mut ReadNodeCallback, &self.rwlocks.reads);
                rbtree::Node::validate(&mut WriteNodeCallback, &self.rwlocks.writes);
                rbtree::Node::validate(&mut PendingNodeCallback, &self.rwlocks.pendings);
            }
        }
    }

    impl Drop for RwLockSet {
        fn drop(&mut self) {
            // Remove borrows in a not-so-subtle way so that this won't abort
            let rwlocks = unsafe { Pin::get_unchecked_mut(Pin::as_mut(&mut self.rwlocks)) };
            rwlocks.reads = None;
            rwlocks.writes = None;
            rwlocks.pendings = None;

            for (_, lock) in self.locks.iter_mut() {
                match &mut lock.state {
                    LockState::Read(state) => {
                        state.inner.get().unwrap().parent.set(None);
                    }
                    LockState::Write(state) => {
                        state.inner.get().unwrap().parent.set(None);
                    }
                    LockState::TryRead(state) => {
                        state.inner.get().unwrap().parent.set(None);
                    }
                    LockState::TryWrite(state) => {
                        state.inner.get().unwrap().parent.set(None);
                    }
                }
            }
        }
    }

    #[derive(Clone, Copy)]
    struct TestLockCallback(LockId);

    impl LockCallback<LockId> for TestLockCallback {
        type Output = bool;

        fn in_progress(self) -> (Self::Output, LockId) {
            (false, self.0)
        }

        fn complete(self) -> Self::Output {
            true
        }
    }

    struct TestUnlockCallback<'a>(&'a mut HashMap<LockId, Lock>);

    impl UnlockCallback<LockId> for TestUnlockCallback<'_> {
        fn complete(&mut self, in_progress: LockId) {
            log::debug!("... lock #{} is now complete", in_progress);

            let lock = self.0.get_mut(&in_progress).unwrap();
            assert_eq!(lock.complete, false);
            lock.complete = true;
        }
    }
}

// Tests
// --------------------------------------------------------------------------

type LockId = usize;

#[quickcheck]
fn qc_rbtree_interval_rw_lock_core(cmds: Vec<u8>) {
    let mut cmds = cmds.into_iter();
    let mut next_lock_id = 1;

    let mut locks: Vec<LockId> = Vec::new();

    log::info!("cmds = {:?}", cmds);

    let mut subject_rwlock = subj::RwLockSet::new();
    let mut reference_rwlock = refr::RwLockSet::new();

    (|| -> Option<()> {
        while let Some(cmd) = cmds.next() {
            match cmd % 2 {
                0 if !locks.is_empty() => {
                    // Choose the lock to unlock
                    let i = cmds.next()? as usize % locks.len();
                    let id = locks.swap_remove(i);

                    log::debug!("Unlocking lock #{:?}", id);

                    // `subject_rwlock`
                    subject_rwlock.unlock(id);

                    // `reference_rwlock`
                    reference_rwlock.unlock(id);
                }

                1 if locks.len() < 64 => {
                    let range = (cmds.next()? as usize % LEN)..(cmds.next()? as usize % LEN);
                    let range = range.start.min(range.end)..range.start.max(range.end);
                    if range.start == range.end {
                        continue;
                    }

                    let priority = cmds.next()? as Priority;

                    let lock_id = next_lock_id;
                    next_lock_id += 1;

                    let lock_ty = (cmd / 2 % 4) as usize;
                    let lock_ty_name = ["Read", "Write", "TryRead", "TryWrite"][lock_ty];
                    let is_write = (lock_ty % 2) == 1;
                    let is_try = lock_ty >= 2;

                    log::debug!(
                        "Locking {:?} as #{}",
                        (&range, priority, lock_ty_name),
                        lock_id
                    );

                    // `subject_rwlock`
                    let subject_complete =
                        subject_rwlock.lock(lock_id, range.clone(), is_write, priority, is_try);

                    if !subject_complete {
                        log::debug!("... it did not complete");
                    }

                    // `reference_rwlock`
                    let reference_complete =
                        reference_rwlock.lock(lock_id, range.clone(), is_write, priority);

                    // Compare the results
                    assert_eq!(subject_complete, reference_complete);

                    // Lock failure?
                    if is_try && !subject_complete {
                        reference_rwlock.unlock(lock_id);
                        continue;
                    }

                    // Remember the lock
                    locks.push(lock_id);
                }

                _ => {}
            }

            // Validate trees after each command completion
            subject_rwlock.validate();

            // Compare completeness
            for &id in locks.iter() {
                let subject_complete = subject_rwlock.is_complete(id);
                let reference_complete = reference_rwlock.is_complete(id);
                assert_eq!(
                    subject_complete, reference_complete,
                    "lock #{} is completed in one but not in the other",
                    id
                );
            }

            // Dump the borrow state
            log::trace!("{:?}", reference_rwlock);
        }

        // Release completed locks. This will unblock and complete other locks
        // in turn. After repeating this step, if there are still incomplete
        // locks, that means our interval lock is prone to deadlocks.
        while !locks.is_empty() {
            let i = locks
                .iter()
                .cloned()
                .position(|id| subject_rwlock.is_complete(id));

            if let Some(i) = i {
                let id = locks.swap_remove(i);
                subject_rwlock.unlock(id);
            } else {
                panic!("deadlock");
            }
        }

        None
    })();
}

#[quickcheck]
fn qc_waitlist_is_fifo(write1: IsWrite, write2: IsWrite) {
    log::info!("{:?}", (write1, write2));

    let mut subject_rwlock = subj::RwLockSet::new();
    assert!(subject_rwlock.lock(1, 0..4, true, 0, true));
    assert!(!subject_rwlock.lock(2, 0..3, write1, 0, false));
    assert!(!subject_rwlock.lock(3, 0..2, write2, 0, false));

    // Lock #2 and #3 are blocked at index 0

    subject_rwlock.unlock(1);

    // Now they are unblocked. However, lock #2 blocked first, so it will
    // complete first. This means index 2 is still locked for reading or
    // writing.

    assert!(!subject_rwlock.lock(4, 2..3, true, 0, true));
}

#[test]
fn unblock_multiple() {
    let mut subject_rwlock = subj::RwLockSet::new();
    assert!(subject_rwlock.lock(1, 0..1, true, 0, true));
    assert!(!subject_rwlock.lock(2, 0..4, false, 0, false));
    assert!(!subject_rwlock.lock(3, 0..5, false, 0, false));

    // Lock #2 and #3 (read) are blocked at index 0

    subject_rwlock.unlock(1);

    // Now they are unblocked.
    assert!(subject_rwlock.is_complete(2));
    assert!(subject_rwlock.is_complete(3));
}
