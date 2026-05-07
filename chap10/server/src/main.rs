use std::thread;
use std::sync::mpsc::channel;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::{self, Read, Write, ErrorKind, Cursor};
use std::net::{TcpListener, TcpStream};

use data_layer::data::Data;
use async_runtime::{executor::Executor, sleep::Sleep};

// 3 つのスレッドにリクエストを分散する
// 各スレッドに処理すべきリクエストがあるかどうかのフラグ
static FLAGS: [AtomicBool; 3] = [
    AtomicBool::new(false),     // parking していない
    AtomicBool::new(false),     // true ならリクエスト到着時にスレッドを起こす
    AtomicBool::new(false),
];

// 個々のスレッドにおけるリクエスト処理
macro_rules! spawn_worker {
    ($name:expr, $rx:expr, $flag:expr) => {
        thread::spawn(move || {
            let mut executor = Executor::new();
            loop {
                if let Ok(stream) = $rx.try_recv() {        // request が到着
                    println!("{} Received connection: {}", $name, stream.peer_addr().unwrap());
                    executor.spawn(handle_client(stream));      // task 起動
                } else {
                    if executor.polling.len() == 0 {    // poll するタスクがない
                        println!("{} is sleeping", $name);
                        $flag.store(true, Ordering::SeqCst);
                        thread::park();     // parking
                    }
                }

                executor.poll();    // task を poll
            }
        })
    };
}

async fn handle_client(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_nonblocking(true)?;
    let mut buffer = Vec::new();
    let mut local_buf = [0; 1024];
    loop {
        match stream.read(&mut local_buf) {
            Ok(0) => {      // 読み込み終了
                break;
            },
            Ok(len) => {    // stream -> local_buf 読み込み
                buffer.extend_from_slice(&local_buf[..len]);
            },
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {    // blocking された
                if buffer.len() > 0 {       // buffer の中身がある
                    break;
                }

                // 少しだけ待機 == 他の task を poll する 
                Sleep::new(std::time::Duration::from_millis(10)).await;     
                continue;
            },
            Err(e) => {
                println!("Failed to read from connection: {}", e);
            }
        }
    }

    match Data::deserialize(&mut Cursor::new(buffer.as_slice())) {
        Ok(message) => {
            println!("Received message: {:?}", message);
        },
        Err(e) => {
            println!("Failed to decode message: {}", e);
        }
    }

    Sleep::new(std::time::Duration::from_secs(1)).await;
    stream.write_all(b"Hello, cliend!")?;

    Ok(())
}

fn main() -> io::Result<()> {
    // リクエストをスレッドに送るチャネル
    let (one_tx, one_rx) = channel::<TcpStream>();
    let (two_tx, two_rx) = channel::<TcpStream>();
    let (three_tx, three_rx) = channel::<TcpStream>();
    // threads
    let one = spawn_worker!("One", one_rx, &FLAGS[0]);
    let two = spawn_worker!("Two", two_rx, &FLAGS[1]);
    let three = spawn_worker!("Three", three_rx, &FLAGS[2]);
    // スレッドとの通信
    let router = [one_tx, two_tx, three_tx];
    let threads = [one, two, three];
    
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Server listening on port 7878");

    let mut index = 0;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {     // data が来たら thread に振り分ける
                let _ = router[index].send(stream);

                if FLAGS[index].load(Ordering::SeqCst) {        // if parking 
                    FLAGS[index].store(false, Ordering::SeqCst);
                    threads[index].thread().unpark();
                }

                index += 1;
                if index == 3 {
                    index = 0;
                }
            },
            Err(e) => {
                println!("Connection failed: {}", e);
            }
        }
    }
    Ok(())
}
