use std::{
    sync::atomic::{AtomicU32, Ordering::Relaxed},
    thread,
    time::Duration,
};
fn main() {
    let a = AtomicU32::new(0);
    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(3));
            a.store(1, Relaxed);
            wake_one(&a);
        });

        println!("Waiting...");
        while a.load(Relaxed) == 0 {
            wait(&a, 0);
        }
        println!("Done!");
    });
}

#[cfg(not(target_os = "linux"))]
compile_error!("Linux only. Sorry!");

pub fn wait(a: &AtomicU32, expected: u32) {
    // Refer to the futex (2) man page for the syscall signature.
    unsafe {
        libc::syscall(
            libc::SYS_futex,                             // The futex syscall.
            a as *const AtomicU32,                       // The atomic to operate on.
            libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG, // The futex operation.
            expected,                                    // The expected value.
            std::ptr::null::<libc::timespec>(),          // No timeout.
        );
    }
}

pub fn wake_one(a: &AtomicU32) {
    // Refer to the futex (2) man page for the syscall signature.
    unsafe {
        libc::syscall(
            libc::SYS_futex,                             // The futex syscall.
            a as *const AtomicU32,                       // The atomic to operate on.
            libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG, // The futex operation.
            1,                                           // The number of threads to wake up.
        );
    }
}
