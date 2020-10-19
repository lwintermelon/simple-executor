pub mod simple_excutor;
pub mod simple_future;

use std::time::Duration;
use simple_future::TimerFuture;

fn main() {
    println!("Hello, world!");
    let (executor, spawner) = simple_excutor::new_executor_and_spawner();
    spawner.spawn(async {
        println!("howdy!");
        TimerFuture::new(Duration::new(2, 0)).await;
        println!("done!");
    });
    drop(spawner);
    executor.run();
}
