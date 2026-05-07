use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use waker_fn::waker_fn;

pub struct Task {
    future: Pin<Box<dyn Future<Output = ()>>>,
    task_id: usize,
    waker: Arc<Waker>
}

pub struct Executor {
    sender: mpsc::Sender<usize>,                // waker 端 
    receiver: mpsc::Receiver<usize>,            // waker からの受信端
    count: AtomicUsize,                         // task id 生成用
    pub waiting: Mutex<HashMap<usize, Task>>,   // waker 待ちをしている task 
    pub polling: Mutex<VecDeque<Task>>          // polling してよい task
}

impl Executor {
    pub fn new() -> Arc<Self> {
        let (sender, receiver) = mpsc::channel();
        Arc::new(Executor { 
            sender, 
            receiver, 
            count: AtomicUsize::new(0), 
            waiting: Mutex::new(HashMap::new()), 
            polling: Mutex::new(VecDeque::new()) 
        })
    }

    pub fn spawn_rcv<F, T>(&self, future: F) -> mpsc::Receiver<T> 
    where 
        F: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        // 与えられた future から、その結果を送信する future をつくる
        let (tx, rx) = mpsc::channel();
        let future_rcv: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
            let result = future.await;
            let _ = tx.send(result);
        });

        let task_id = self.count.fetch_add(1, Ordering::Relaxed);   // task id 生成
        let sender = self.sender.clone();
        
        // waker_fn crate を用いて、wake させる task_id を送信する waker を生成
        let waker = waker_fn(move || sender.send(task_id).expect("send")).into();
        
        let task = Task {
            future: future_rcv,
            task_id,
            waker    
        };
        self.polling.lock().unwrap().push_back(task);
        return rx;
    }

    pub fn run(&self) {
        loop {
            self.poll();    // polling queue にある task をすべて実行

            // 実行すべき task なし & 待機すべき waker なし
            if self.polling.lock().unwrap().is_empty() && self.waiting.lock().unwrap().is_empty() {
                break;
            }

            let task_id = self.receiver.recv().unwrap();    // waker から起こすべき task を受信
            if let Some(task) = self.waiting.lock().unwrap().remove(&task_id) {
                self.polling.lock().unwrap().push_back(task);       // polling に追加
            }
        }
    }

    pub fn poll(&self) {
        loop {
            // task を queue から取得、なければ終了
            let mut task = match self.polling.lock().unwrap().pop_front() {
                Some(task) => task,
                None => break,
            };

            // future の poll 処理
            let waker: Arc<Waker> = task.waker.clone();
            let context = &mut Context::from_waker(&waker);
            match task.future.as_mut().poll(context) {
                Poll::Ready(()) => {},
                Poll::Pending => {
                    self.waiting.lock().unwrap().insert(task.task_id, task);
                }
            }
        }
    }
}

fn main() {
    let executor = Executor::new();
    let rx = executor.spawn_rcv(async {
        async_std::task::sleep(Duration::from_secs(1)).await;
        10
    });
    executor.run();
    println!("got value {:}", rx.recv().unwrap());
}
