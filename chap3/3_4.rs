use std::future::Future;
use std::panic::catch_unwind;
use std::thread;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use std::sync::LazyLock;

use async_task::{Runnable, Task};
use futures_lite::future;
use flume::{Receiver, Sender};

// thread 数を増やす (remark: 多くの場合、非同期処理は重いタスクには不適切ゆえ、 THREAD_NUM = 1)
const HIGH_PRIORITY_THREAD_NUM: i32 = 3;

// 複数の (優先順位付き) QUEUE with task-stealing
#[derive(Debug, Clone, Copy)]
enum FutureType {
    High,
    Low,
}

trait FutureOrderLabel: Future {
    fn get_order(&self) -> FutureType;
}


fn spawn_task<F, T>(future: F) -> Task<T> where 
    F: Future<Output = T> + Send + 'static + FutureOrderLabel, 
    T: Send + 'static 
{
    // task-stealing 用のチャネル
    static HIGH_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> = LazyLock::new(|| flume::unbounded::<Runnable>());
    static LOW_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> = LazyLock::new(|| flume::unbounded::<Runnable>());

    // 遅延評価で QUEUE を作成
    // LazyLock は最初に初期化され、その後初期化されないことを保証されている
    // static: プログラム実行中常に生存している
    static HIGH_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        for _ in 0..HIGH_PRIORITY_THREAD_NUM {
            let high_receiver = HIGH_CHANNEL.1.clone();
            let low_receiver = LOW_CHANNEL.1.clone();
            thread::spawn(move || {
                loop {
                    match high_receiver.try_recv() {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());    // Runnable を受信したら実行; catch_unwind: 実行時例外を捕捉
                        },

                        Err(_) => {     // task がない -> steal を試みる
                            match low_receiver.try_recv() {
                                Ok(runnable) => {
                                    let _ = catch_unwind(|| runnable.run());
                                },
                                Err(_) => {     // 本番環境では sleep() は避ける
                                    thread::sleep(Duration::from_millis(100));
                                }
                            }
                        }
                    };
                }
            });
        }

        HIGH_CHANNEL.0.clone()  // sender を返す
    });

    static LOW_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let high_receiver = HIGH_CHANNEL.1.clone();
        let low_receiver = LOW_CHANNEL.1.clone();
        thread::spawn(move || {     // 入力待ちのスレッド
            loop {
                match low_receiver.try_recv() {
                    Ok(runnable) => {
                        let _ = catch_unwind(|| runnable.run());    // Runnable を受信したら実行; catch_unwind: 実行時例外を捕捉
                    },

                    Err(_) => {     // task がない -> steal を試みる
                        match high_receiver.try_recv() {
                            Ok(runnable) => {
                                let _ = catch_unwind(|| runnable.run());
                            },
                            Err(_) => {     // 本番環境では sleep() は避ける
                                thread::sleep(Duration::from_millis(100));
                            }
                        }
                    }
                };
            }
        });

        LOW_CHANNEL.0.clone()   // sender を返す: Runnable をスレッドに送信できるようにする
    });

    // Runnable を QUEUE に送信するクロージャ
    let schedule = match future.get_order() {
        FutureType::High => |runnable| HIGH_QUEUE.send(runnable).unwrap(),
        FutureType::Low => |runnable| LOW_QUEUE.send(runnable).unwrap(),
    };

    // Future を Heap 上に確保し、 pointer を保持する Runnable と task を作成
    let (runnable, task) = async_task::spawn(future, schedule);
    runnable.schedule();    // Runnable を QUEUE に送信

    return task;
}

struct CounterFuture {
    count: u32,
    order: FutureType,
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

impl FutureOrderLabel for CounterFuture {
    fn get_order(&self) -> FutureType {
        self.order
    }
}

// function for test
async fn async_fn() {
    std::thread::sleep(Duration::from_secs(1));     // blocking sleep
    println!("async_fn");
}

fn main() {
    let future_1 = CounterFuture {count: 0, order: FutureType::High};
    let future_2 = CounterFuture {count: 0, order: FutureType::Low};
    let task_1 = spawn_task(future_1);
    let task_2 = spawn_task(future_2);

    std::thread::sleep(Duration::from_secs(5));
    println!("before the block");

    future::block_on(task_1);
    future::block_on(task_2);
}