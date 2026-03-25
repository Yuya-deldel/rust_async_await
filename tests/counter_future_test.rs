use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::task::JoinHandle;

struct CounterFuture {
    count: u32
}

impl Future for CounterFuture {
    type Output = u32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.count += 1;
        
        println!("polling with result: {}", self.count);
        std::thread::sleep(Duration::from_secs(1));

        if self.count < 5 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(self.count)
        }
    }
}

#[tokio::main] 
async fn main() {
    let counter_1 = CounterFuture { count: 0 };
    let counter_2 = CounterFuture { count: 0 };
    
    let handle_1 = tokio::task::spawn(async move {
        counter_1.await
    });
    let handle_2 = tokio::task::spawn(async move {
        counter_2.await
    });

    tokio::join!(handle_1, handle_2);
}