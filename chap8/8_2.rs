use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::error::TryRecvError;

async fn actor_replacement(state: Arc<Mutex<i64>>, value: i64) -> i64 {
    let mut state = state.lock().await;
    *state += value;
    println!("result: {}", *state);
    return *state;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let state = Arc::new(Mutex::new(0));
    let mut handles = Vec::new();
    let now = tokio::time::Instant::now();
    for i in 0..10 {
        let state_ref = state.clone();
        let _mutex_handle = tokio::spawn(async move {
            actor_replacement(state_ref, i).await
        });
        
        handles.push(_mutex_handle);
    }

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    println!("Elapesd: {:?}", now.elapsed());
}