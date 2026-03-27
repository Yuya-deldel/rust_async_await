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


// 複数の (優先順位付き) QUEUE with task-stealing
#[derive(Debug, Clone, Copy)]
enum FutureType {
    High,
    Low,
}

fn spawn_task<F, T>(future: F, order: FutureType) -> Task<T> where 
    F: Future<Output = T> + Send + 'static, 
    T: Send + 'static 
{
    // task-stealing 用のチャネル
    static HIGH_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> = LazyLock::new(|| flume::unbounded::<Runnable>());
    static LOW_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> = LazyLock::new(|| flume::unbounded::<Runnable>());

    // 遅延評価で QUEUE を作成
    // LazyLock は最初に初期化され、その後初期化されないことを保証されている
    // static: プログラム実行中常に生存している
    static HIGH_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let queue_num = std::env::var("HIGH_NUM").unwrap().parse::<usize>().unwrap();
        for _ in 0..queue_num {
            let high_receiver = HIGH_CHANNEL.1.clone();
            let low_receiver = LOW_CHANNEL.1.clone();
            thread::spawn(move || {
                loop {
                    match high_receiver.try_recv() {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());    // Runnable を受信したら実行; catch_unwind: 実行時例外を捕捉
                        },

                        Err(_) => match low_receiver.try_recv() {       // task がない -> steal を試みる
                            Ok(runnable) => {
                                let _ = catch_unwind(|| runnable.run());
                            },
                            Err(_) => {     // 本番環境では sleep() は避ける
                                thread::sleep(Duration::from_millis(100));    
                            }
                        }
                    };
                }
            });
        }

        HIGH_CHANNEL.0.clone()  // sender を返す
    });

    static LOW_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let queue_num = std::env::var("LOW_NUM").unwrap().parse::<usize>().unwrap();
        for _ in 0..queue_num {
            let high_receiver = HIGH_CHANNEL.1.clone();
            let low_receiver = LOW_CHANNEL.1.clone();
            thread::spawn(move || {
                loop {
                    match low_receiver.try_recv() {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());    // Runnable を受信したら実行; catch_unwind: 実行時例外を捕捉
                        },

                        Err(_) => match high_receiver.try_recv() {       // task がない -> steal を試みる
                            Ok(runnable) => {
                                let _ = catch_unwind(|| runnable.run());
                            },
                            Err(_) => {     // 本番環境では sleep() は避ける
                                thread::sleep(Duration::from_millis(100));    
                            }
                        }
                    };
                }
            });
        }

        LOW_CHANNEL.0.clone()  // sender を返す
    });

    // Runnable を QUEUE に送信するクロージャ
    let schedule = match order {
        FutureType::High => |runnable| HIGH_QUEUE.send(runnable).unwrap(),
        FutureType::Low => |runnable| LOW_QUEUE.send(runnable).unwrap(),
    };

    // Future を Heap 上に確保し、 pointer を保持する Runnable と task を作成
    let (runnable, task) = async_task::spawn(future, schedule);
    runnable.schedule();    // Runnable を QUEUE に送信

    return task;
}

// spawn_task macro
macro_rules! spawn_task {
    ($future:expr) => {     // default case
        spawn_task($future, FutureType::Low)
    };
    ($future:expr, $order:expr) => {
        spawn_task($future, $order)
    };
}

// join macro
macro_rules! join {
    ($($future:expr),*) => {
        {
            let mut results = Vec::new();
            $(
                results.push(future::block_on($future));
            )*
            results
        }          
    };
}

macro_rules! try_join {
    ($($future:expr),*) => {
        {
            let mut results = Vec::new();
            $(
                let result = catch_unwind(|| future::block_on($future));
                results.push(result);
            )*
            results
        }
    };
}

struct Runtime {
    high_priority_queue_num: usize,
    low_priority_queue_num: usize,
}

impl Runtime {
    pub fn new() -> Self {
        // 利用できる Core 数
        let num_cores = std::thread::available_parallelism().unwrap().get();
        Self {
            high_priority_queue_num: num_cores - 2,
            low_priority_queue_num: 1,
        }
    }

    pub fn set_hqnum(mut self, num: usize) -> Self {
        self.high_priority_queue_num = num;
        self
    }

    pub fn set_lqnum(mut self, num: usize) -> Self {
        self.low_priority_queue_num = num;
        self
    }

    pub fn run(&self) {     // 空のタスクを立ち上げて QUEUE を生成
        unsafe {    // 環境変数をグローバル変数として設定 (あまり良い方法ではない)
            std::env::set_var("HIGH_NUM", self.high_priority_queue_num.to_string());
            std::env::set_var("LOW_NUM", self.low_priority_queue_num.to_string());
        }
        let high = spawn_task(async {}, FutureType::High);
        let low = spawn_task(async {}, FutureType::Low);
        join!(high, low);
    }
}

#[derive(Debug, Clone, Copy)]
struct BackgroundProcess;

impl Future for BackgroundProcess {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("background process firing");
        std::thread::sleep(Duration::from_secs(1));
        cx.waker().wake_by_ref();
        Poll::Pending       // 常に Pending を返す
    }
}

struct CounterFuture {
    count: u32,
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


// function for test
async fn async_fn() {
    std::thread::sleep(Duration::from_secs(1));     // blocking sleep
    println!("async_fn");
}

fn main() {
    Runtime::new().run();

    spawn_task!(BackgroundProcess{}).detach();

    let future_1 = CounterFuture {count: 0};
    let future_2 = CounterFuture {count: 0};
    let task_1 = spawn_task!(future_1, FutureType::High);
    let task_2 = spawn_task!(future_2);

    let task_3 = spawn_task!(async_fn());
    let task_4 = spawn_task!(async {
        async_fn().await;
        async_fn().await;
    }, FutureType::High);

    std::thread::sleep(Duration::from_secs(5));
    println!("before the block");

    let outcome: Vec<u32> = join!(task_1, task_2);
    let outcome_2: Vec<()> = join!(task_3, task_4);
}