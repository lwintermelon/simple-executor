#![allow(unused)]

use basic::Arc;
use std::{
    ptr::addr_of,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    thread,
    time::Instant,
};

fn main() {
    let b = Box::new(12u128);
    dbg!(addr_of!(b));
    dbg!(size_of::<u128>());
    dbg!(size_of::<&u128>());
    dbg!(size_of::<&mut u128>());
    dbg!(size_of::<*mut u128>());
    dbg!(size_of::<*mut u128>());
    let t = &3128;
    dbg!(size_of_val(t));
    Arc::new(1);

    use std::hint::black_box;
    #[repr(align(64))] // This struct must be 64-byte aligned.
    struct Aligned(AtomicU64);
    // false sharing
    static A: [Aligned; 3] = [
        Aligned(AtomicU64::new(0)),
        Aligned(AtomicU64::new(0)),
        Aligned(AtomicU64::new(0)),
    ];

    black_box(&A);
    thread::spawn(|| loop {
        A[0].0.store(0, Relaxed);
        A[2].0.store(0, Relaxed);
    });
    let start = Instant::now();
    for _ in 0..1_000_000_000 {
        black_box(A[1].0.load(Relaxed));
    }
    println!("{:?}", start.elapsed());
}

mod basic {

    use std::{
        ops::Deref,
        ptr::NonNull,
        sync::atomic::{
            fence, AtomicUsize,
            Ordering::{Acquire, Relaxed, Release},
        },
    };

    struct ArcData<T> {
        ref_count: AtomicUsize,
        data: T,
    }

    pub struct Arc<T> {
        ptr: NonNull<ArcData<T>>,
    }

    unsafe impl<T: Send + Sync> Send for Arc<T> {}
    unsafe impl<T: Send + Sync> Sync for Arc<T> {}

    impl<T> Arc<T> {
        pub fn new(data: T) -> Arc<T> {
            Arc {
                ptr: Box::leak(Box::new(ArcData {
                    ref_count: AtomicUsize::new(1),
                    data,
                }))
                .into(),
            }
        }
        fn data(&self) -> &ArcData<T> {
            unsafe { self.ptr.as_ref() }
        }

        pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
            if arc.data().ref_count.load(Relaxed) == 1 {
                fence(Acquire);

                return Some(unsafe { &mut arc.ptr.as_mut().data });
            } else {
                return None;
            }
        }
    }
    impl<T> Deref for Arc<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.data().data
        }
    }

    impl<T> Clone for Arc<T> {
        fn clone(&self) -> Self {
            if self.data().ref_count.fetch_add(1, Relaxed) > usize::MAX / 2 {
                std::process::abort();
            }
            Self { ptr: self.ptr }
        }
    }
    impl<T> Drop for Arc<T> {
        fn drop(&mut self) {
            if self.data().ref_count.fetch_sub(1, Release) == 1 {
                fence(Acquire);
                let arc_data = unsafe { Box::from_raw(self.ptr.as_ptr()) };
                drop(arc_data);
            }
        }
    }

    #[test]
    fn test() {
        static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);
        struct DetectDrop;
        impl Drop for DetectDrop {
            fn drop(&mut self) {
                NUM_DROPS.fetch_add(1, Relaxed);
            }
        }
        // Create two Arcs sharing an object containing a string
        // and a DetectDrop, to detect when it's dropped.
        let x = Arc::new(("hello", DetectDrop));
        let y = x.clone();

        // Send x to another thread, and use it there.
        let t = std::thread::spawn(move || {
            assert_eq!(x.0, "hello");
        });

        // In parallel, y should still be usable here.
        assert_eq!(y.0, "hello");

        // Wait for the thread to finish.
        t.join().unwrap();

        // One Arc, x, should be dropped by now.
        // We still have y, so the object shouldn't have been dropped yet.\
        assert_eq!(NUM_DROPS.load(Relaxed), 0);

        // Drop the remaining `Arc`.
        drop(y);

        // Now that `y` is dropped too,
        // the object should've been dropped.
        assert_eq!(NUM_DROPS.load(Relaxed), 1);
    }
}

mod weak {
    use std::{
        cell::UnsafeCell,
        ops::Deref,
        ptr::NonNull,
        sync::atomic::{
            fence, AtomicUsize,
            Ordering::{Acquire, Relaxed, Release},
        },
    };

    struct ArcData<T> {
        data_ref_count: AtomicUsize,
        alloc_ref_count: AtomicUsize,
        data: UnsafeCell<Option<T>>,
    }

    pub struct Arc<T> {
        weak: Weak<T>,
    }

    pub struct Weak<T> {
        ptr: NonNull<ArcData<T>>,
    }

    impl<T> Weak<T> {
        fn data(&self) -> &ArcData<T> {
            unsafe { self.ptr.as_ref() }
        }

        pub fn upgrade(&self) -> Option<Arc<T>> {
            let mut current_data_ref_count = self.data().data_ref_count.load(Relaxed);
            loop {
                if current_data_ref_count == 0 {
                    return None;
                }
                assert!(current_data_ref_count <= usize::MAX / 2);

                match self.data().data_ref_count.compare_exchange_weak(
                    current_data_ref_count,
                    current_data_ref_count + 1,
                    Relaxed,
                    Relaxed,
                ) {
                    Ok(_) => return Some(Arc { weak: self.clone() }),
                    Err(v) => current_data_ref_count = v,
                }
            }
        }
    }
    unsafe impl<T: Send + Sync> Send for Weak<T> {}
    unsafe impl<T: Send + Sync> Sync for Weak<T> {}

    impl<T> Arc<T> {
        pub fn new(data: T) -> Arc<T> {
            Arc {
                weak: Weak {
                    ptr: NonNull::from(Box::leak(Box::new(ArcData {
                        data_ref_count: AtomicUsize::new(1),
                        alloc_ref_count: AtomicUsize::new(1),
                        data: UnsafeCell::new(Some(data)),
                    }))),
                },
            }
        }

        pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
            let arc_data = arc.weak.data();
            if arc_data.alloc_ref_count.load(Relaxed) == 1 {
                fence(Acquire);
                let ptr = arc_data.data.get();
                unsafe { (*ptr).as_mut() }
            } else {
                return None;
            }
        }

        pub fn downgrade(arc: &Self) -> Weak<T> {
            arc.weak.clone()
        }
    }
    impl<T> Deref for Arc<T> {
        type Target = T;

        fn deref(&self) -> &T {
            let ptr = self //
                .weak
                .data()
                .data
                .get();

            unsafe { &*ptr } //
                .as_ref()
                .unwrap()
        }
    }

    impl<T> Clone for Weak<T> {
        fn clone(&self) -> Self {
            if self.data().alloc_ref_count.fetch_add(1, Relaxed) > usize::MAX / 2 {
                std::process::abort();
            }
            Self { ptr: self.ptr }
        }
    }

    impl<T> Clone for Arc<T> {
        fn clone(&self) -> Self {
            let weak = self.weak.clone();
            if self.weak.data().data_ref_count.fetch_add(1, Relaxed) > usize::MAX / 2 {
                std::process::abort();
            }
            Self { weak }
        }
    }
    impl<T> Drop for Weak<T> {
        fn drop(&mut self) {
            if self.data().alloc_ref_count.fetch_sub(1, Release) == 1 {
                fence(Acquire);
                let arc_data = unsafe { Box::from_raw(self.ptr.as_ptr()) };
                drop(arc_data);
            }
        }
    }

    impl<T> Drop for Arc<T> {
        fn drop(&mut self) {
            if self.weak.data().data_ref_count.fetch_sub(1, Release) == 1 {
                fence(Acquire);
                let data = self.weak.data().data.get();
                // Safety: The data reference counter is zero,
                // so nothing will access it.
                unsafe {
                    *data = None;
                }
            }
        }
    }
    #[test]
    fn test() {
        static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);
        struct DetectDrop;
        impl Drop for DetectDrop {
            fn drop(&mut self) {
                NUM_DROPS.fetch_add(1, Relaxed);
            }
        }
        // Create two Arcs sharing an object containing a string
        // and a DetectDrop, to detect when it's dropped.
        let x = Arc::new(("hello", DetectDrop));
        let y = x.clone();

        // Send x to another thread, and use it there.
        let t = std::thread::spawn(move || {
            assert_eq!(x.0, "hello");
        });

        // In parallel, y should still be usable here.
        assert_eq!(y.0, "hello");

        // Wait for the thread to finish.
        t.join().unwrap();

        // One Arc, x, should be dropped by now.
        // We still have y, so the object shouldn't have been dropped yet.\
        assert_eq!(NUM_DROPS.load(Relaxed), 0);

        // Drop the remaining `Arc`.
        drop(y);

        // Now that `y` is dropped too,
        // the object should've been dropped.
        assert_eq!(NUM_DROPS.load(Relaxed), 1);
    }

    #[test]
    fn test_weak() {
        static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);
        struct DetectDrop;
        impl Drop for DetectDrop {
            fn drop(&mut self) {
                NUM_DROPS.fetch_add(1, Relaxed);
            }
        }

        // Create an Arc with two weak pointers.
        let x = Arc::new(("hello", DetectDrop));
        let y = Arc::downgrade(&x);
        let z = Arc::downgrade(&x);

        // Send x to another thread, and use it there.
        let t = std::thread::spawn(move || {
            // Weak pointer should be upgradable at this point.
            let y = y.upgrade().unwrap();
            assert_eq!(y.0, "hello");
        });

        assert_eq!(x.0, "hello");

        // Wait for the thread to finish.
        t.join().unwrap();

        // The data shouldn't be dropped yet,
        // and the weak pointer should be upgradable.
        assert_eq!(NUM_DROPS.load(Relaxed), 0);
        assert!(z.upgrade().is_some());

        drop(x);

        // Now, the data should be dropped, and the
        // weak pointer should no longer be upgradable.
        assert_eq!(NUM_DROPS.load(Relaxed), 1);
        assert!(z.upgrade().is_none());
    }

    #[allow(unused)]
    fn annoying(mut arc: Arc<u128>) {
        loop {
            //1. weak may be zero, and arc must be above one here.
            let weak = Arc::downgrade(&arc); //2. weak changes from 0 to 1
            drop(arc); //3. arc was reduced by one, it may be zero now.
            println!("I have no Arc!");
            arc = weak.upgrade().unwrap();
            drop(weak);
            println!("I have no Weak!");
        }
    }
}

pub mod optimized {
    use core::{
        cell::UnsafeCell,
        mem::ManuallyDrop,
        ops::{Deref, DerefMut},
        ptr::NonNull,
        sync::atomic::{
            fence, AtomicUsize,
            Ordering::{Acquire, Relaxed, Release},
        },
    };

    const WEAK_LOCKED: usize = usize::MAX;
    struct ArcData<T> {
        strong_count: AtomicUsize,
        weak_count: AtomicUsize,
        data: UnsafeCell<ManuallyDrop<T>>,
    }

    pub struct Arc<T> {
        ptr: NonNull<ArcData<T>>,
    }
    unsafe impl<T: Send + Sync> Send for Arc<T> {}
    unsafe impl<T: Send + Sync> Sync for Arc<T> {}

    pub struct Weak<T> {
        ptr: NonNull<ArcData<T>>,
    }

    impl<T> Weak<T> {
        fn data(&self) -> &ArcData<T> {
            unsafe { self.ptr.as_ref() }
        }

        pub fn upgrade(&self) -> Option<Arc<T>> {
            let mut current_strong_count = self.data().strong_count.load(Relaxed);
            loop {
                if current_strong_count == 0 {
                    return None;
                }
                assert!(current_strong_count <= usize::MAX / 2);

                match self.data().strong_count.compare_exchange_weak(
                    current_strong_count,
                    current_strong_count + 1,
                    Relaxed,
                    Relaxed,
                ) {
                    Ok(_) => return Some(Arc { ptr: self.ptr }),
                    Err(v) => current_strong_count = v,
                }
            }
        }
    }
    unsafe impl<T: Send + Sync> Send for Weak<T> {}
    unsafe impl<T: Send + Sync> Sync for Weak<T> {}

    impl<T> Arc<T> {
        pub fn new(data: T) -> Arc<T> {
            Arc {
                ptr: NonNull::from(Box::leak(Box::new(ArcData {
                    strong_count: AtomicUsize::new(1),
                    weak_count: AtomicUsize::new(1),
                    data: UnsafeCell::new(ManuallyDrop::new(data)),
                }))),
            }
        }

        fn data(&self) -> &ArcData<T> {
            unsafe { self.ptr.as_ref() }
        }

        pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
            // We need to make sure (strong_count==1,weak_count==1) at the same time.
            // If we first check weak_count first, then check weak_count,
            // there may be a change from  (strong_count>1,weak_count==1) to (strong_count==1,weak_count>1),
            // Does that possible? Yes，(weak_count==1 and this arc exists => no Weak, and at least 1 Arc.)
            // suppose there are other Arcs, any other Arc could do downgrade then all other Arc dropped, making the change possible.
            // We solve this by locking weak_count at the same time it's 1 by CAS,
            // then load strong_count to check it, then we can unlock weak_count,
            // the downgrade method (the only one that can modify weak_count, sine Clone of Weak is impossible at this moment)
            // will just spine since there is just a brief load,
            // then we can check if the loading strong_count is also 1 <=> whether this arc is the only one.
            // The load from strong_count must be synced, for correctness.

            // but how to do it? Look at the operations below.
            // weak_count: 1->locked by CAS. (1)
            // load strong_count. (2)
            // weak_count: locked->1 by store (3)

            // (2) can't be reoredered above (1) or below (3), so (1) must be acquire, and (3) must be (release)
            // how to explain these two memory orders?

            //(1): though load weak_count==1 suggest there is no Weak(since this arc exist, contributing one to weak_count),
            // but without sync, the access to Weak still is possible, so it can do upgrade.
            // the later load from strong_count may not see the newly upgraded values (it may be reordered after load).
            // So acquire here, sync with any previous Weak's Drop => sync with any previous Weak's upgrade.

            //(3): (1) shows without sync, previous Weak's upgrade operation may be reordered,
            // so can a future Arc's drop be reordered?
            // Yes,  the locked weak_count prevent the execution of any downgrade, so there must be a sync with downgrade method.
            // Without sync, later downgraded Weak

            match arc
                .data()
                .weak_count
                .compare_exchange(1, WEAK_LOCKED, Acquire, Relaxed)
            {
                // weak_count is locked
                Ok(_) => {
                    let strong_count = arc.data().strong_count.load(Relaxed); // load strong_count
                    arc.data().weak_count.store(1, Release); // yes, we can unlock weak_count here

                    if strong_count == 1 {
                        // Any other Arc may access to T before its Drop,
                        // which must happens before return to the mut ref.
                        fence(Acquire);
                        let ptr = arc.data().data.get();
                        let data_mut_ref = unsafe { &mut *ptr } //
                            .deref_mut();

                        return Some(data_mut_ref);
                    } else {
                        return None;
                    }
                }
                Err(_) => {
                    return None;
                }
            }
        }

        pub fn downgrade(arc: &Self) -> Weak<T> {
            let mut current_weak_count = arc.data().weak_count.load(Relaxed);

            loop {
                // spin until weak_count is unlocked
                if current_weak_count == WEAK_LOCKED {
                    core::hint::spin_loop();
                    current_weak_count = arc.data().weak_count.load(Relaxed);
                    continue;
                }
                assert!(current_weak_count <= usize::MAX / 2);
                match arc.data().weak_count.compare_exchange_weak(
                    current_weak_count,
                    current_weak_count + 1,
                    Acquire,
                    Relaxed,
                ) {
                    Ok(_) => return Weak { ptr: arc.ptr },
                    Err(v) => {
                        current_weak_count = v;
                        continue; // Of course, we need to check if it's locked again.
                    }
                }
            }
        }
    }
    impl<T> Deref for Arc<T> {
        type Target = T;

        fn deref(&self) -> &T {
            let ptr = self //
                .data()
                .data
                .get();

            unsafe { &*ptr } //
                .deref()
        }
    }

    impl<T> Clone for Weak<T> {
        fn clone(&self) -> Self {
            if self.data().weak_count.fetch_add(1, Relaxed) > usize::MAX / 2 {
                std::process::abort();
            }
            Self { ptr: self.ptr }
        }
    }

    impl<T> Clone for Arc<T> {
        fn clone(&self) -> Self {
            if self.data().strong_count.fetch_add(1, Relaxed) > usize::MAX / 2 {
                std::process::abort();
            }
            Self { ptr: self.ptr }
        }
    }
    impl<T> Drop for Weak<T> {
        fn drop(&mut self) {
            if self.data().weak_count.fetch_sub(1, Release) == 1 {
                fence(Acquire);
                let arc_data = unsafe { Box::from_raw(self.ptr.as_ptr()) };
                drop(arc_data);
            }
        }
    }

    impl<T> Drop for Arc<T> {
        fn drop(&mut self) {
            if self.data().strong_count.fetch_sub(1, Release) == 1 {
                fence(Acquire);
                let data = self.data().data.get();
                // Safety: The data reference counter is zero,
                // so nothing will access it.
                unsafe {
                    ManuallyDrop::drop(&mut *data);
                }

                // Now that there is no `Arc<T> left`,
                // drop the implicit week pointer that represented all `Arc<T>`s.
                drop(Weak { ptr: self.ptr });
            }
        }
    }
    #[test]
    fn test() {
        static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);
        struct DetectDrop;
        impl Drop for DetectDrop {
            fn drop(&mut self) {
                NUM_DROPS.fetch_add(1, Relaxed);
            }
        }
        // Create two Arcs sharing an object containing a string
        // and a DetectDrop, to detect when it's dropped.
        let x = Arc::new(("hello", DetectDrop));
        let y = x.clone();

        // Send x to another thread, and use it there.
        let t = std::thread::spawn(move || {
            assert_eq!(x.0, "hello");
        });

        // In parallel, y should still be usable here.
        assert_eq!(y.0, "hello");

        // Wait for the thread to finish.
        t.join().unwrap();

        // One Arc, x, should be dropped by now.
        // We still have y, so the object shouldn't have been dropped yet.\
        assert_eq!(NUM_DROPS.load(Relaxed), 0);

        // Drop the remaining `Arc`.
        drop(y);

        // Now that `y` is dropped too,
        // the object should've been dropped.
        assert_eq!(NUM_DROPS.load(Relaxed), 1);
    }

    #[test]
    fn test_weak() {
        static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);
        struct DetectDrop;
        impl Drop for DetectDrop {
            fn drop(&mut self) {
                NUM_DROPS.fetch_add(1, Relaxed);
            }
        }

        // Create an Arc with two weak pointers.
        let x = Arc::new(("hello", DetectDrop));
        let y = Arc::downgrade(&x);
        let z = Arc::downgrade(&x);

        // Send x to another thread, and use it there.
        let t = std::thread::spawn(move || {
            // Weak pointer should be upgradable at this point.
            let y = y.upgrade().unwrap();
            assert_eq!(y.0, "hello");
        });

        assert_eq!(x.0, "hello");

        // Wait for the thread to finish.
        t.join().unwrap();

        // The data shouldn't be dropped yet,
        // and the weak pointer should be upgradable.
        assert_eq!(NUM_DROPS.load(Relaxed), 0);
        assert!(z.upgrade().is_some());

        drop(x);

        // Now, the data should be dropped, and the
        // weak pointer should no longer be upgradable.
        assert_eq!(NUM_DROPS.load(Relaxed), 1);
        assert!(z.upgrade().is_none());
    }
}

mod test_processor {
    use core::sync::atomic::{compiler_fence, fence, AtomicI32, Ordering::*, *};

    #[no_mangle]
    pub fn load(x: &i32) -> i32 {
        *x
    }

    #[no_mangle]
    pub fn load_atomic(x: &AtomicI32) -> i32 {
        x.load(Relaxed)
    }

    #[no_mangle]
    pub fn fetch_add_atomic(x: &AtomicI32) {
        x.fetch_add(10, Relaxed);
    }

    #[no_mangle]
    pub fn fetch_or_atomic(x: &AtomicI32) -> i32 {
        x.fetch_or(10, Relaxed)
    }

    #[no_mangle]
    pub fn cas_loop(x: &AtomicI32) -> i32 {
        let mut current = x.load(Relaxed);
        loop {
            let new = current | 10;
            match x.compare_exchange(current, new, Relaxed, Relaxed) {
                Ok(v) => return v,
                Err(v) => current = v,
            }
        }
    }
    #[no_mangle]
    pub fn cas_weak(x: &AtomicI32) {
        let _ = x.compare_exchange_weak(5, 6, Relaxed, Relaxed);
    }

    #[no_mangle]
    pub fn cas_strong(x: &AtomicI32) {
        let _ = x.compare_exchange(5, 6, Relaxed, Relaxed);
    }

    #[no_mangle]
    pub fn fetch_store(x: &AtomicI32) {
        x.swap(6, Relaxed);
    }

    #[no_mangle]
    fn test_fn(locked: &AtomicBool, counter: &AtomicUsize) {
        // Acquire the lock, using the wrong memory ordering.
        while locked.swap(true, Acquire) {}
        while locked
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_err()
        {}
        let old = counter.load(Relaxed);
        let new = old + 1;
        counter.store(new, Relaxed);
        locked.store(false, Release);
    }

    #[no_mangle]
    pub fn test_acquire_fence() {
        fence(Acquire);
    }

    #[no_mangle]
    pub fn test_release_fence() {
        fence(Release);
    }

    #[no_mangle]
    pub fn test_acqrel_fence() {
        fence(AcqRel);
    }

    #[no_mangle]
    pub fn test_seqcst() {
        fence(SeqCst);
    }

    #[no_mangle]
    pub fn test_compile_acquire_fence() {
        compiler_fence(Acquire);
    }

    #[no_mangle]
    pub fn test_compile_release_fence() {
        compiler_fence(Release);
    }

    #[no_mangle]
    pub fn test_compile_acqrel_fence() {
        compiler_fence(AcqRel);
    }

    #[no_mangle]
    pub fn test_compile_seqcst() {
        compiler_fence(SeqCst);
    }
}

#[no_mangle]
pub fn arc_test() {
    let mut arc = optimized::Arc::new(3);
    optimized::Arc::downgrade(&arc);
    optimized::Arc::get_mut(&mut arc);
}
