use super::*;
use quickcheck_macros::quickcheck;

use std::prelude::v1::*;

use crate::utils::rbtree;

type Index = usize;
type Priority = u64;
type InProgress = ();

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
    use super::{Index, LockId, Priority, LEN};

    use std::{
        cmp::Ordering,
        collections::{BTreeSet, HashMap, HashSet},
        fmt,
        ops::Range,
        prelude::v1::*,
    };

    type IsWrite = bool;

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
    pub struct NotSoIntervalRwLockCore {
        rwlocks: Vec<RwLock>,
        locks: HashMap<LockId, Lock>,
    }

    impl NotSoIntervalRwLockCore {
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

    impl fmt::Debug for NotSoIntervalRwLockCore {
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

// Tests
// --------------------------------------------------------------------------

type LockId = usize;

#[derive(Debug)]
struct Lock {
    id: LockId,
    range: Range<Index>,
    priority: Priority,
    state: LockState,
}

#[derive(Debug)]
enum LockState {
    Read(Pin<Box<ReadLockState<Index, Priority, InProgress>>>),
    Write(Pin<Box<WriteLockState<Index, Priority, InProgress>>>),
    TryRead(Pin<Box<TryReadLockState<Index, Priority, InProgress>>>),
    TryWrite(Pin<Box<TryWriteLockState<Index, Priority, InProgress>>>),
}

#[quickcheck]
fn qc_rbtree_interval_rw_lock_core(cmds: Vec<u8>) {
    let mut cmds = cmds.into_iter();
    let mut next_lock_id = 1;

    let mut locks: Vec<Lock> = Vec::new();

    log::info!("cmds = {:?}", cmds);

    let mut subject_rwlock =
        Box::pin(RbTreeIntervalRwLockCore::<Index, Priority, InProgress>::new());
    let mut reference_rwlock = refr::NotSoIntervalRwLockCore::new();

    (|| -> Option<()> {
        while let Some(cmd) = cmds.next() {
            match cmd % 2 {
                0 if !locks.is_empty() => {
                    // Choose the lock to unlock
                    let i = cmds.next()? as usize % locks.len();
                    let mut lock = locks.swap_remove(i);

                    log::debug!("Unlocking {:?}", lock);

                    // `subject_rwlock`
                    match &mut lock.state {
                        LockState::Read(state) => {
                            Pin::as_mut(&mut subject_rwlock).unlock_read(Pin::as_mut(state), ());
                        }
                        LockState::Write(state) => {
                            Pin::as_mut(&mut subject_rwlock).unlock_write(Pin::as_mut(state), ());
                        }
                        LockState::TryRead(state) => {
                            Pin::as_mut(&mut subject_rwlock)
                                .unlock_try_read(Pin::as_mut(state), ());
                        }
                        LockState::TryWrite(state) => {
                            Pin::as_mut(&mut subject_rwlock)
                                .unlock_try_write(Pin::as_mut(state), ());
                        }
                    }

                    // `reference_rwlock`
                    reference_rwlock.unlock(lock.id);
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

                    log::debug!("Locking {:?}", (lock_id, &range, priority, lock_ty_name));

                    // `subject_rwlock`
                    let (ok, lock_state) = match lock_ty {
                        0 => {
                            let mut state = Box::pin(ReadLockState::new());
                            Pin::as_mut(&mut subject_rwlock).lock_read(
                                range.clone(),
                                priority,
                                Pin::as_mut(&mut state),
                                (),
                            );
                            (true, LockState::Read(state))
                        }
                        1 => {
                            let mut state = Box::pin(WriteLockState::new());
                            Pin::as_mut(&mut subject_rwlock).lock_write(
                                range.clone(),
                                priority,
                                Pin::as_mut(&mut state),
                                (),
                            );
                            (true, LockState::Write(state))
                        }
                        2 => {
                            let mut state = Box::pin(TryReadLockState::new());
                            let ok = Pin::as_mut(&mut subject_rwlock)
                                .try_lock_read(range.clone(), Pin::as_mut(&mut state));
                            (ok, LockState::TryRead(state))
                        }
                        3 => {
                            let mut state = Box::pin(TryWriteLockState::new());
                            let ok = Pin::as_mut(&mut subject_rwlock)
                                .try_lock_write(range.clone(), Pin::as_mut(&mut state));
                            (ok, LockState::TryWrite(state))
                        }
                        _ => unreachable!(),
                    };

                    if !ok {
                        log::debug!("... but did not success");
                    }

                    let is_write = match lock_state {
                        LockState::Read(_) | LockState::TryRead(_) => false,
                        LockState::Write(_) | LockState::TryWrite(_) => true,
                    };
                    let is_try = match lock_state {
                        LockState::Read(_) | LockState::Write(_) => false,
                        LockState::TryRead(_) | LockState::TryWrite(_) => true,
                    };

                    // `reference_rwlock`
                    let reference_complete =
                        reference_rwlock.lock(lock_id, range.clone(), is_write, priority);

                    // Compare the results
                    if is_try {
                        // Both locks' behavior should match
                        assert_eq!(ok, reference_complete);
                    }

                    // Lock failure?
                    if !ok {
                        reference_rwlock.unlock(lock_id);
                        continue;
                    }

                    // Remember the lock
                    let lock = Lock {
                        id: lock_id,
                        state: lock_state,
                        range,
                        priority,
                    };

                    locks.push(lock);
                }

                _ => {}
            }

            // Validate trees after each command completion
            unsafe {
                rbtree::Node::validate(&mut ReadNodeCallback, &subject_rwlock.reads);
                rbtree::Node::validate(&mut WriteNodeCallback, &subject_rwlock.writes);
                rbtree::Node::validate(&mut PendingNodeCallback, &subject_rwlock.pendings);
            }

            // Dump the borrow state
            log::trace!("{:?}", reference_rwlock);
        }

        None
    })();

    // Remove all borrows, or `subject_rwlock`'s destructor will panic
    for mut lock in locks {
        match &mut lock.state {
            LockState::Read(state) => {
                Pin::as_mut(&mut subject_rwlock).unlock_read(Pin::as_mut(state), ());
            }
            LockState::Write(state) => {
                Pin::as_mut(&mut subject_rwlock).unlock_write(Pin::as_mut(state), ());
            }
            LockState::TryRead(state) => {
                Pin::as_mut(&mut subject_rwlock).unlock_try_read(Pin::as_mut(state), ());
            }
            LockState::TryWrite(state) => {
                Pin::as_mut(&mut subject_rwlock).unlock_try_write(Pin::as_mut(state), ());
            }
        }
    }
}
