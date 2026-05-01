use std::future::Future;
use std::time::Duration;
use std::sync::LazyLock;

use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(4)                  // 非同期タスクを処理するスレッド数
        .max_blocking_threads(1)            // blocking task に使用するスレッド数を制限する (spawn_blocking() で使用)
        .on_thread_start(|| {                               // worker thread が開始したときに実行される関数
            println!("thread starting for runtime A")
        })
        .on_thread_stop(|| {                                // worker thread が終了したときに実行される関数
            println!("thread stopping for runtime A")
        })
        .thread_keep_alive(Duration::from_secs(60)) // blocking task の task 終了後の timeout 時間
        // スレッド間で共有される global queue は、一定回数 local queue を処理するたびに実行される
        // その回数 (= interval) を定める
        // 公平性とオーバーヘッドとのトレードオフを定める
        .global_queue_interval(61)          
        .on_thread_park(|| {                                // 処理する task がなくなるたびに実行される関数
            println!("thread parking for runtime A")
        })
        .thread_name("our custom runtime A")    // runtime が作成する thread name
        .thread_stack_size(3 * 1024 * 1024)     // 各スレッドのスタックサイズ
        .enable_time()                          // tokio::time module を有効にする
        .build()
        .unwrap()
});

pub fn spawn_task<F, T>(future: F) -> JoinHandle<T> 
where 
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{   
    RUNTIME.spawn(future)
}

async fn sleep_example() -> i32 {
    println!("sleeping for 2 seconds");
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("done sleeping");
    20
}

fn main() {
    let handle = spawn_task(sleep_example());
    println!("spawned task");
    println!("task status: {}", handle.is_finished());
    std::thread::sleep(Duration::from_secs(3));
    println!("task status: {}", handle.is_finished());
    let result = RUNTIME.block_on(handle).unwrap();
    println!("task result: {}", result);
}