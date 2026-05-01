use std::sync::LazyLock;
use std::cell::UnsafeCell;
use std::collections::HashMap;

use tokio;
use tokio_util::task::LocalPoolHandle;

thread_local! {     // thread ごとに異なる値を持つ
    pub static COUNTER: UnsafeCell<HashMap<u32, u32>> = UnsafeCell::new(HashMap::new());
}

async fn something(number: u32) {       // tokio::sleep(number) & increment(COUNTER) 
    tokio::time::sleep(std::time::Duration::from_secs(number as u64)).await;
    COUNTER.with(|counter| {    // 非同期ブロックではないので、await 文を用いてはならない
        let counter = unsafe {&mut *counter.get()};
        match counter.get_mut(&number) {
            Some(count) => {
                let placeholder = *count + 1;
                *count = placeholder;
            },
            None => {
                counter.insert(number, 1);
            }
        }
    });
}

static RUNTIME: LazyLock<LocalPoolHandle> = LazyLock::new(|| {
    LocalPoolHandle::new(4)
});

fn extract_data_from_thread() -> HashMap<u32, u32> {
    let mut extracted_counter: HashMap<u32, u32> = HashMap::new();
    COUNTER.with(|counter| {
        let counter = unsafe {&mut *counter.get()};
        extracted_counter = counter.clone()
    });

    extracted_counter
}

async fn get_complete_count() -> HashMap<u32, u32> {
    let mut complete_counter = HashMap::new();
    let mut extracted_counters = Vec::new();
    for i in 0..4 {
        // 個々のスレッドに対し一回ずつ extract_data_from_thread() を呼ぶ
        extracted_counters.push(RUNTIME.spawn_pinned_by_idx(|| async move {
            extract_data_from_thread()
        }, i));
    }

    for counter_future in extracted_counters {
        let extracted_counter = counter_future.await.unwrap_or_default();
        for (key, count) in extracted_counter {
            *complete_counter.entry(key).or_insert(0) += count;
        }
    }

    complete_counter
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let _handle = tokio::spawn(async {
        let sequence = [1, 2, 3, 4, 5];
        let repeated_sequence: Vec<_> = sequence.iter().cycle().take(500000).cloned().collect();
        
        let mut futures = Vec::new();
        for number in repeated_sequence {
            futures.push(RUNTIME.spawn_pinned(move || async move {
                something(number).await;
                something(number).await
            }));
        }
        for i in futures {
            let _ = i.await.unwrap();
        }
        println!("All futures completed");
    });

    tokio::signal::ctrl_c().await.unwrap();
    println!("ctrl-c received!");

    let complete_counter = get_complete_count().await;
    println!("Complete counter: {:?}", complete_counter);
}