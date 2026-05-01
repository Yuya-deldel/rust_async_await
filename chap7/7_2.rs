use std::cell::RefCell;

use tokio_util::task::LocalPoolHandle;

// local-pool: 特定の非同期タスクがあるスレッドに固定される
// 構造体が特定のスレッドに固定されるので、排他制御が必要なくなる
// task-stealing などの最適化処理は行われなくなる

thread_local! {     // 個々のスレッドごとに異なる値を持つ
    pub static COUNTER: RefCell<u32> = RefCell::new(1);     // Send trait は必要ない
}

async fn something(number: u32) -> u32 {
    std::thread::sleep(std::time::Duration::from_secs(3));
    // tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    COUNTER.with(|counter| {
        *counter.borrow_mut() += 1;
        println!("COUNTER: {} for: {}", *counter.borrow(), number);
    });

    number
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let pool = LocalPoolHandle::new(3);
    let one = pool.spawn_pinned(|| async {
        println!("one");
        something(1).await
    });
    let two = pool.spawn_pinned(|| async {
        println!("two");
        something(2).await
    });
    let three = pool.spawn_pinned(|| async {
        println!("three");
        something(3).await
    });
    let result = async {
        let one = one.await.unwrap();
        let two = two.await.unwrap();
        let three = three.await.unwrap();

        one + two + three
    };

    println!("result: {}", result.await);
}