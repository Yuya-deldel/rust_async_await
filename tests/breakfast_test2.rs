use std::time::{Duration, Instant};
use tokio::time::sleep;
use std::thread;

// tokio の sleep は non-blocking
// std::thread::sleep は blocking
// .await は結果が得られるまで実行を中断する
async fn prep_coffee_mug() {
    println!("Pouring milk");
    thread::sleep(Duration::from_secs(3));
    println!("Milk poured");

    println!("Putting instant coffee");
    thread::sleep(Duration::from_secs(3));
    println!("Instant coffee put");
}

async fn make_coffee() {
    println!("boiling kettle");
    sleep(Duration::from_secs(10)).await;
    println!("kettle boiled");

    println!("pouring boiled water");
    thread::sleep(Duration::from_secs(3));
    println!("boiled water poured");
}

async fn make_toast() {
    println!("putting bread in toaster");
    sleep(Duration::from_secs(10)).await;
    println!("bread toasted");

    println!("buttering toasted bread");
    thread::sleep(Duration::from_secs(5));
    println!("toasted bread buttered");
}

#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() {
    let start_time = Instant::now();
    
    let person_1 = tokio::task::spawn(async {
        let coffee_mug_step = prep_coffee_mug();
        let coffee_step = make_coffee();
        let toast_step = make_toast();
        tokio::join!(coffee_mug_step, coffee_step, toast_step);
    });
    
    let person_2 = tokio::task::spawn(async {
        let coffee_mug_step = prep_coffee_mug();
        let coffee_step = make_coffee();
        let toast_step = make_toast();
        tokio::join!(coffee_mug_step, coffee_step, toast_step);
    });

    let _ = tokio::join!(person_1, person_2);

    let elapsed_time = start_time.elapsed();
    println!("It took: {} seconds", elapsed_time.as_secs());
}
