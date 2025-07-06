use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};

use crate::ch9_locks::MutexGuard;

pub struct Condvar {
    counter: AtomicU32,
    num_waiters: AtomicUsize,
}

impl Condvar {
    pub fn new() -> Self {
        Self {
            counter: AtomicU32::new(0),
            num_waiters: AtomicUsize::new(0),
        }
    }

    pub fn notify_one(&self) {
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            atomic_wait::wake_one(&self.counter);
        }
    }
    pub fn notify_all(&self) {
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            atomic_wait::wake_all(&self.counter);
        }
    }
    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        self.num_waiters.fetch_add(1, Relaxed);
        let current_counter = self.counter.load(Relaxed);
        let mutex = guard.mutex;

        // unlock the mutex, then the notifier thread who get the lock
        // will definitely see the added value, so this will get notified.
        drop(guard);
        atomic_wait::wait(&self.counter, current_counter);

        // Once the decrementing operation is executed,
        // the waiting thread  no longer needs to be wooken up anyway.
        self.num_waiters.fetch_sub(1, Relaxed);
        mutex.lock()
    }
}

#[test]
fn test_condvar() {
    let mutex = crate::ch9_locks::Mutex::new(0);
    let condvar = Condvar::new();
    let mut wakeups = 0;

    std::thread::scope(|s| {
        s.spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            *mutex.lock() = 123;
            condvar.notify_one();
        });

        let mut m = mutex.lock();
        while *m < 100 {
            m = condvar.wait(m);
            wakeups += 1;
        }
        assert_eq!(*m, 123);
    });

    assert!(wakeups < 10);
    dbg!(wakeups);
}
