#![allow(unused)]

use std::{
    cell::{Cell, LazyCell, OnceCell, RefCell},
    collections::VecDeque,
    sync::{
        atomic::{
            fence, AtomicBool, AtomicU32, AtomicU64, AtomicUsize,
            Ordering::{Acquire, Relaxed, Release, SeqCst},
        },
        Arc, Condvar, Mutex, Once,
    },
    thread,
    time::Duration,
};

use rand::Rng;

fn main() {
    // example_locking::run();
    chapter3_memory_ordering::fence::run();
}

mod chapter3_memory_ordering {
    pub use super::*;
    pub mod lazy_init {
        pub use super::*;
        use std::sync::atomic::AtomicPtr;

        type Data = i128;
        fn get_data() -> &'static Data {
            static PTR: AtomicPtr<Data> = AtomicPtr::new(std::ptr::null_mut());
            let mut p = PTR.load(Acquire);
            if p.is_null() {
                p = Box::into_raw(Box::new(111i128));
                match PTR.compare_exchange(std::ptr::null_mut(), p, Release, Acquire) {
                    Ok(_) => {}
                    Err(v) => unsafe {
                        drop(Box::from_raw(p));
                        p = v
                    },
                }
            }
            unsafe { &*p }
        }
    }
    pub mod sequence_consistent_ordering {
        pub use super::*;

        static A: AtomicBool = AtomicBool::new(false);
        static B: AtomicBool = AtomicBool::new(false);
        static mut S: String = String::new();
        pub fn run() {
            let a = thread::spawn(|| {
                A.store(true, SeqCst);
                if !B.load(SeqCst) {
                    unsafe {
                        (&mut *&raw mut S).push('!');
                    }
                }
            });
            let b = thread::spawn(|| {
                B.store(true, SeqCst);
                if !A.load(SeqCst) {
                    unsafe {
                        (&mut *&raw mut S).push('!');
                    }
                }
            });
            a.join().unwrap();
            b.join().unwrap();
            dbg!(unsafe { &*S });
        }
    }

    pub mod fence {
        pub use super::*;

        static mut DATA: [u64; 10] = [0u64; 10];
        const ATOMIC_FALSE: AtomicBool = AtomicBool::new(false);
        static READY: [AtomicBool; 10] = [ATOMIC_FALSE; 10];
        pub fn run() {
            for i in 0..10 {
                thread::spawn(move || {
                    let data = some_calculation(i as u64);
                    unsafe {
                        DATA[i] = data;
                    }
                    READY[i].store(true, Release);
                });
            }
            thread::sleep(Duration::from_millis(500));
            let read: [bool; 10] = std::array::from_fn(|i| READY[i].load(Relaxed));
            if read.contains(&true) {
                fence(Acquire);
                for i in 0..10 {
                    if read[i] {
                        dbg!((i, unsafe { DATA[i] }));
                    }
                }
            }
        }
    }
}

mod example_locking {
    use super::*;

    static mut DATA: String = String::new();
    static LOCKED: AtomicBool = AtomicBool::new(false);

    fn f() {
        if LOCKED
            .compare_exchange(false, true, Acquire, Relaxed)
            .is_ok()
        {
            unsafe {
                (&mut *&raw mut DATA).push('!');
            }
            LOCKED.store(false, Release);
        }
    }
    pub fn run() {
        thread::scope(|s| {
            for _ in 0..100 {
                s.spawn(f);
            }
        });
        dbg!(unsafe {
            let s = &*DATA;
            (s, s.len())
        });
    }
}

mod archive {
    use rand::Rng;

    pub use super::*;
    fn condvar_practice() {
        let queue = Mutex::new(VecDeque::new());
        let not_empty = Condvar::new();
        thread::scope(|s| {
            s.spawn(|| loop {
                let mut q = queue.lock().unwrap();
                let item = loop {
                    if let Some(item) = q.pop_front() {
                        break item;
                    } else {
                        q = not_empty.wait(q).unwrap();
                    }
                };
                drop(q);
                dbg!(item);
            });
            for i in 0.. {
                queue.lock().unwrap().push_back(i);
                not_empty.notify_one();
                thread::sleep(Duration::from_secs(1));
            }
        });
    }

    fn allocate_new_id() -> u32 {
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);
        let id = NEXT_ID.fetch_add(1, Relaxed);
        id
    }

    fn allocate_new_id2() -> u32 {
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);
        let mut id = NEXT_ID.load(Relaxed);

        loop {
            assert!(id < 1000, "too many IDs!");

            match NEXT_ID.compare_exchange_weak(id, id + 1, Relaxed, Relaxed) {
                Ok(_) => return id,
                Err(v) => id = v,
            }
        }
    }

    fn get_key() -> u64 {
        static KEY: AtomicU64 = AtomicU64::new(0);
        let key = KEY.load(Relaxed);
        if key == 0 {
            let new_key: u64 = rand::rng().random_range(1u64..=u64::MAX);
            match KEY.compare_exchange(0, new_key, Relaxed, Relaxed) {
                Ok(_) => new_key,
                Err(k) => k,
            }
        } else {
            key
        }
    }

    fn progress_reporting() {
        let num_done = AtomicUsize::new(0);
        let main_thread = thread::current();
        thread::scope(|s| {
            // A background thread to process all 100 items.
            s.spawn(|| {
                for i in 0..100usize {
                    some_work(i);
                    num_done.store(i + 1, Relaxed);
                    main_thread.unpark();
                }
            });

            // The main thread shows status updates.
            loop {
                let n = num_done.load(Relaxed);
                println!("Working.. {n}/100 done");
                if n == 100 {
                    break;
                }
                thread::park_timeout(Duration::from_secs(1));
            }
        });
        println!("Done!");
    }

    fn release_and_aquire_ordering() {
        static DATA: AtomicU64 = AtomicU64::new(0);
        static READY: AtomicBool = AtomicBool::new(false);
        thread::spawn(|| {
            DATA.store(123, Relaxed);
            READY.store(true, Release);
        });
        while READY.load(Acquire) == false {
            thread::sleep(Duration::from_millis(100));
            println!("waiting...");
        }
        dbg!(DATA.load(Relaxed));
    }
}

fn some_work(i: usize) {
    let mut rng = rand::rng();
    let ms: u64 = rng.random_range(20..=1200);

    thread::sleep(Duration::from_millis(ms));
}

fn some_calculation(i: u64) -> u64 {
    let mut rng = rand::rng();
    let ms: u64 = rng.random_range(20..=1200);

    thread::sleep(Duration::from_millis(ms));
    ms
}
