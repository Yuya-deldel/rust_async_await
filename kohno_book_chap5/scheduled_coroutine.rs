use std::future::Future;
use std::sync::{Arc, Mutex};
// mpsc: Multiple Producers, Single Consumers;
// 送信は複数スレッドから行えるが、受信は単一スレッドからのみ可能なチャネル
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};    
use std::task::{Context, Poll};
use std::pin::Pin;
use futures::future::{BoxFuture, FutureExt};
use futures::task::{waker_ref, ArcWake};

struct Hello {
    state: StateHello
}

enum StateHello {
    Hello,
    World,
    End
}

impl Hello {
    fn new() -> Self {
        Hello {state: StateHello::Hello}
    }
}

impl Future for Hello {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match (*self).state {
            StateHello::Hello => {
                print!("Hello, ");
                (*self).state = StateHello::World;
                cx.waker().wake_by_ref();       // 実行キュー (waker) に追加
                Poll::Pending
            },
            StateHello::World => {
                println!("World!");
                (*self).state = StateHello::End;
                cx.waker().wake_by_ref();       // 実行キューに追加
                Poll::Pending
            },
            StateHello::End => Poll::Ready(())
        }
    }
}

struct Task {   // Task と Waker を兼ねる
    future: Mutex<BoxFuture<'static, ()>>,  // 実行すべきコルーチン
    sender: SyncSender<Arc<Task>>           // Executor に Task を渡すためのチャネル
}

impl ArcWake for Task {     // プロセスのスケジューリングを行う trait
    fn wake_by_ref(arc_self: &Arc<Self>) {      // 自身の Arc を Executor に送信
        let self2 = arc_self.clone();
        arc_self.sender.send(self2).unwrap();
    }
}

struct Executor {       // 送受信チャネルの端点の情報を保持
    sender: SyncSender<Arc<Task>>,
    receiver: Receiver<Arc<Task>>
}

impl Executor {
    fn new() -> Self {
        // chanell のキューの最大個数を 1024 に設定
        let (sender, receiver) = sync_channel(1024);
        Executor { 
            sender: sender.clone(), 
            receiver 
        }
    }

    fn get_spawner(&self) -> Spawner {      // 新たな Task を生成する Spawner を生成
        Spawner {
            sender: self.sender.clone()
        }
    }

    fn run(&self) {
        while let Ok(task) = self.receiver.recv() {     // Task を受信
            // future: 実行すべきコルーチン
            let mut future = task.future.lock().unwrap();
            let waker = waker_ref(&task);   // task から Waker 作成
            let mut ctx = Context::from_waker(&waker);  // Waker から Context 作成
            let _ = future.as_mut().poll(&mut ctx);     // poll を実行
        }
    }
}

struct Spawner {    // 実行キューに追加するための端点を保持
    sender: SyncSender<Arc<Task>>
}

impl Spawner {      
    // future を Task に包んで実行キューに追加する
    fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let future = future.boxed();
        let task = Arc::new(Task {
            future: Mutex::new(future),
            sender: self.sender.clone()
        });

        self.sender.send(task).unwrap();
    }
}

fn main() {
    let executor = Executor::new();
    executor.get_spawner().spawn(Hello::new());
    executor.run();
}