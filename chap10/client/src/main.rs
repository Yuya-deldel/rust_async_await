use std::io;
use std::sync::{Arc, Mutex};
use std::net::TcpStream;
use std::time::Instant;

use data_layer::data::Data;
use async_runtime::{executor::Executor, receiver::TcpReceiver, sender::TcpSender};

async fn send_data(field1: u32, field2: u16, field3: String) -> io::Result<String> {
    let stream = Arc::new(Mutex::new(TcpStream::connect("127.0.0.1:7878")?));
    let message = Data {field1, field2, field3};
    TcpSender {stream: stream.clone(), buffer: message.serialize()?}.await?;    // TcpSender.poll()
    let receiver = TcpReceiver {
        stream: stream.clone(),
        buffer: Vec::new()
    };
    String::from_utf8(receiver.await?)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8"))
}

fn main() -> io::Result<()> {
    let mut executor = Executor::new();
    let mut handles = Vec::new();
    
    let start = Instant::now();

    for i in 0..4000 {      // task 起動
        let handle = executor.spawn(send_data(
            i, i as u16, format!("Hello, server! {}", i)
        ));
        handles.push(handle);
    }

    std::thread::spawn(move || {    // 実行
        loop {
            executor.poll();
        }
    });

    println!("Waiting for result...");

    for handle in handles {
        match handle.recv().unwrap() {      // 結果を表示
            Ok(result) => println!("Result: {}", result),
            Err(e) => println!("Error: {}", e)
        };
    }

    let duration = start.elapsed();
    println!("Time elapsed in expensive_func() is: {:?}", duration);
    Ok(())
}
