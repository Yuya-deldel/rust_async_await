use std::sync::{Arc, Mutex};
use std::task::Context;
use std::pin::Pin;
use std::future::Future;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use core::task::Poll;

#[derive(Debug)]
enum CounterType {
    Increment,
    Decrement
}

struct SharedData {
    counter: i32
}

impl SharedData {
    fn increment(&mut self) {
        self.counter += 1;
    }

    fn decrement(&mut self) {
        self.counter -= 1;
    }
}

struct CounterFuture {
    counter_type: CounterType,
    data_reference: Arc<Mutex<SharedData>>,
    count: u32,
}

impl Future for CounterFuture {
    type Output = u32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        std::thread::sleep(Duration::from_secs(1));
        
        // poll() は循環依存を避けるため async にできない => std::sync::Mutex を用いる
        // try_lock() は non-blocking に lock を試みる => Polling
        let mut guard = match self.data_reference.try_lock() {
            Ok(guard) => guard,
            Err(error) => {
                println!("error for {:?}: {}", self.counter_type, error);
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };      // data が取得できた

        let value = &mut *guard;
        match self.counter_type {
            CounterType::Increment => {
                value.increment();
                println!("after increment: {}", value.counter);
            },
            CounterType::Decrement => {
                value.decrement();
                println!("after decrement: {}", value.counter);
            }
        }
        std::mem::drop(guard);      // Mutex 解放

        self.count += 1;
        if self.count < 3 {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        } else {
            return Poll::Ready(self.count);
        }
    }
}

async fn count(count: u32, data: Arc<tokio::sync::Mutex<SharedData>>, counter_type: CounterType) -> u32 {
    for _ in 0..count {
        let mut data = data.lock().await;
        match counter_type {
            CounterType::Increment => {
                data.increment();
                println!("after increment: {}", data.counter);
            },
            CounterType::Decrement => {
                data.decrement();
                println!("after decrement: {}", data.counter);
            },
        }
        std::mem::drop(data);

        std::thread::sleep(Duration::from_secs(1));
    }

    return count;
}

/* 
#[tokio::main]
async fn main() {
    let shared_data = Arc::new(Mutex::new(SharedData{counter: 0}));
    let counter_1 = CounterFuture {
        counter_type: CounterType::Increment,
        data_reference: shared_data.clone(),
        count: 0,
    };
    let counter_2 = CounterFuture {
        counter_type: CounterType::Decrement,
        data_reference: shared_data.clone(),
        count: 0,
    };
    let handle_1 = tokio::task::spawn(async move {
        counter_1.await
    });
    let handle_2 = tokio::task::spawn(async move {
        counter_2.await
    });
    
    tokio::join!(handle_1, handle_2);
}
*/

#[tokio::main]
async fn main() {
    let shared_data = Arc::new(tokio::sync::Mutex::new(SharedData{counter: 0}));
    let shared_data_2 = shared_data.clone();
    let handle_1 = tokio::task::spawn(async move {
        count(3, shared_data, CounterType::Increment).await
    });
    let handle_2 = tokio::task::spawn(async move {
        count(3, shared_data_2, CounterType::Decrement).await
    });
    
    tokio::join!(handle_1, handle_2);
}