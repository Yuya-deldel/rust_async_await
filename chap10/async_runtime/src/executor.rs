use crate::waker::MyWaker;

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{mpsc, Arc};
use std::task::{Context, Poll, Waker};

pub struct Task {
    future: Pin<Box<dyn Future<Output = ()> + Send>>,   // 結果をチャネルに送信する future
    waker: Arc<Waker>
}

pub struct Executor {   // タスクがたまっているキュー
    pub polling: VecDeque<Task>
}

impl Executor {
    pub fn new() -> Self {
        Executor { polling: VecDeque::new() }
    }

    // 実際の実装では JoinHandle 構造体を返す: 実態は future の結果を受け取るチャネル
    pub fn spawn<F, T>(&mut self, future: F) -> mpsc::Receiver<T> 
    where 
        F: Future<Output = T> + 'static + Send,
        T: Send + 'static,
        {
            let (tx, rx) = mpsc::channel();
            
            // 与えられた future を実行し、channel に送信する future を定義
            let future_send_to_channel: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(async move {
                let result = future.await;
                let _ = tx.send(result);
            });
            let task = Task {
                future: future_send_to_channel,     // 結果をチャネルに送信する future
                waker: self.create_waker(),         // waker を生成
            };
            self.polling.push_back(task);           // task を追加

            rx
        }

    pub fn poll(&mut self) {
        let mut task = match self.polling.pop_front() {
            Some(task) => task,
            None => return,
        };

        // 取り出した task から waker と context を生成
        let waker = task.waker.clone();
        let context = &mut Context::from_waker(&waker);

        // context の元で future を実行
        match task.future.as_mut().poll(context) {
            Poll::Ready(()) => {},  // 結果をチャネルに送信完了
            Poll::Pending => {      // 完了していなければもう一度実行
                self.polling.push_back(task);
            }
        }
    }

    pub fn create_waker(&self) -> Arc<Waker> {
        Arc::new(Arc::new(MyWaker{}).into())
    }
}