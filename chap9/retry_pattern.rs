async fn get_data() -> Result<String, Box<dyn std::error::Error>> {
    Err("Error".into())
}

async fn do_something() -> Result<(), Box<dyn std::error::Error>> {
    let mut milliseconds = 1000;
    let total_count = 5;    // total_count 回 Err() が返ってきたら終了
    let mut count = 0;
    let result: String;
    loop {
        match get_data().await {
            Ok(data) => {
                result = data;
                break;
            },
            Err(err) => {
                println!("Error: {}", err);
                count += 1;
                if count == total_count {
                    return Err(err);
                }
            }
        }

        // Err() が返るたびに待機時間を倍倍にする
        tokio::time::sleep(tokio::time::Duration::from_millis(milliseconds)).await;
        milliseconds *= 2;
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let outcome = do_something().await;
    println!("Outcome: {:?}", outcome);
}