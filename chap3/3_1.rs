use std::future::Future;
use std::panic::catch_unwind;
use std::thread;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use std::sync::LazyLock;
use async_task::{Runnable, Task};
use futures_lite::future;

fn spawn_task<F, T>(future: F) -> Task<T>
where F: Future<Output = T> + Send + 'static, T: Send + 'static {
    // LazyLock は最初に初期化され、その後初期化されないことを保証されている
    // static: プログラム実行中常に生存している
    static QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let (tx, rx) = flume::unbounded::<Runnable>();
        thread::spawn(move || {     // 入力待ちのスレッド
            while let Ok(runnable) = rx.recv() {    // Runnable を受信したら実行
                println!("runnable accepted");
                let _ = catch_unwind(|| runnable.run());        // catch_unwind: 実行時例外を捕捉
            }
        });

        tx  // sender を返す: Runnable をスレッドに送信できるようにする
    });

    // Runnable を QUEUE に送信するクロージャ
    let schedule = |runnable| QUEUE.send(runnable).unwrap();
    // Future を Heap 上に確保し、 pointer を保持する Runnable と task を作成
    let (runnable, task) = async_task::spawn(future, schedule);
    runnable.schedule();    // Runnable を QUEUE に送信

    println!("Here is the queue count: {:?}", QUEUE.len());
    return task;
}

struct CounterFuture {
    count: u32
}

impl Future for CounterFuture {
    type Output = u32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.count += 1;

        println!("polling with result: {}", self.count);
        std::thread::sleep(Duration::from_secs(1));     // blocking sleep
    
        if self.count < 3 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(self.count)
        }
    }
}

async fn async_fn() {
    std::thread::sleep(Duration::from_secs(1));     // blocking sleep
    println!("async_fn");
}

fn main() {
    let future_1 = CounterFuture {count: 0};
    let future_2 = CounterFuture {count: 0};
    let task_1 = spawn_task(future_1);
    let task_2 = spawn_task(future_2);
    let task_3 = spawn_task(async {
        async_fn().await;
        async_fn().await;
        async_fn().await;
        async_fn().await;
    });

    std::thread::sleep(Duration::from_secs(5));
    println!("before the block");

    future::block_on(task_1);
    future::block_on(task_2);
    future::block_on(task_3);
}