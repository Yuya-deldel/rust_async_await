use std::future::Future;
use std::sync::{Arc, Mutex};
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
                Poll::Pending
            },
            StateHello::World => {
                println!("World!");
                (*self).state = StateHello::End;
                Poll::Pending
            },
            StateHello::End => Poll::Ready(())
        }
    }
}

struct Task {
    hello: Mutex<BoxFuture<'static, ()>>
}

impl Task {     // async/await におけるプロセスの実行単位
    fn new() -> Self {
        let hello = Hello::new();
        Task { hello: Mutex::new(hello.boxed()) }
    }
}

impl ArcWake for Task {     // プロセスのスケジューリングを行う trait
    fn wake_by_ref(_arc_self: &Arc<Self>) {}     // do nothing
}

fn main() {
    let task = Arc::new(Task::new());
    let waker = waker_ref(&task);
    let mut ctx = Context::from_waker(&waker);
    let mut hello = task.hello.lock().unwrap();

    hello.as_mut().poll(&mut ctx);
    hello.as_mut().poll(&mut ctx);
    hello.as_mut().poll(&mut ctx);
}