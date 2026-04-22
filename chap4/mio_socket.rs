use std::io::{Read, Write};
use std::time::Duration;
use std::error::Error;
use std::sync::LazyLock;
use std::pin::Pin;
use std::future::Future;
use std::panic::catch_unwind;
use std::thread;
use std::task::{Context, Poll};

use mio::net::{TcpListener, TcpStream};     // low-layer async I/O library; tokio 等の構成要素
use mio::{Events, Interest, Poll as MioPoll, Token};

use async_task::{Runnable, Task};
use flume::{Receiver, Sender};
use futures_lite::future;

#[derive(Debug, Clone, Copy)]
enum FutureType {
    High,
    Low,
}

fn spawn_task<F, T>(future: F, order: FutureType) -> Task<T> where 
    F: Future<Output = T> + Send + 'static, 
    T: Send + 'static 
{
    // task-stealing 用のチャネル
    static HIGH_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> = LazyLock::new(|| flume::unbounded::<Runnable>());
    static LOW_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> = LazyLock::new(|| flume::unbounded::<Runnable>());

    // 遅延評価で QUEUE を作成
    // LazyLock は最初に初期化され、その後初期化されないことを保証されている
    // static: プログラム実行中常に生存している
    static HIGH_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let queue_num = std::env::var("HIGH_NUM").unwrap().parse::<usize>().unwrap();
        for _ in 0..queue_num {
            let high_receiver = HIGH_CHANNEL.1.clone();
            let low_receiver = LOW_CHANNEL.1.clone();
            thread::spawn(move || {
                loop {
                    match high_receiver.try_recv() {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());    // Runnable を受信したら実行; catch_unwind: 実行時例外を捕捉
                        },

                        Err(_) => match low_receiver.try_recv() {       // task がない -> steal を試みる
                            Ok(runnable) => {
                                let _ = catch_unwind(|| runnable.run());
                            },
                            Err(_) => {     // 本番環境では sleep() は避ける
                                thread::sleep(Duration::from_millis(100));    
                            }
                        }
                    };
                }
            });
        }

        HIGH_CHANNEL.0.clone()  // sender を返す
    });

    static LOW_QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let queue_num = std::env::var("LOW_NUM").unwrap().parse::<usize>().unwrap();
        for _ in 0..queue_num {
            let high_receiver = HIGH_CHANNEL.1.clone();
            let low_receiver = LOW_CHANNEL.1.clone();
            thread::spawn(move || {
                loop {
                    match low_receiver.try_recv() {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());    // Runnable を受信したら実行; catch_unwind: 実行時例外を捕捉
                        },

                        Err(_) => match high_receiver.try_recv() {       // task がない -> steal を試みる
                            Ok(runnable) => {
                                let _ = catch_unwind(|| runnable.run());
                            },
                            Err(_) => {     // 本番環境では sleep() は避ける
                                thread::sleep(Duration::from_millis(100));    
                            }
                        }
                    };
                }
            });
        }

        LOW_CHANNEL.0.clone()  // sender を返す
    });

    // Runnable を QUEUE に送信するクロージャ
    let schedule = match order {
        FutureType::High => |runnable| HIGH_QUEUE.send(runnable).unwrap(),
        FutureType::Low => |runnable| LOW_QUEUE.send(runnable).unwrap(),
    };

    // Future を Heap 上に確保し、 pointer を保持する Runnable と task を作成
    let (runnable, task) = async_task::spawn(future, schedule);
    runnable.schedule();    // Runnable を QUEUE に送信

    return task;
}

// spawn_task macro
macro_rules! spawn_task {
    ($future:expr) => {     // default case
        spawn_task($future, FutureType::Low)
    };
    ($future:expr, $order:expr) => {
        spawn_task($future, $order)
    };
}

// join macro
macro_rules! join {
    ($($future:expr),*) => {
        {
            let mut results = Vec::new();
            $(
                results.push(future::block_on($future));
            )*
            results
        }          
    };
}

macro_rules! try_join {
    ($($future:expr),*) => {
        {
            let mut results = Vec::new();
            $(
                let result = catch_unwind(|| future::block_on($future));
                results.push(result);
            )*
            results
        }
    };
}

struct Runtime {
    high_priority_queue_num: usize,
    low_priority_queue_num: usize,
}

impl Runtime {
    pub fn new() -> Self {
        // 利用できる Core 数
        let num_cores = std::thread::available_parallelism().unwrap().get();
        Self {
            high_priority_queue_num: num_cores - 2,
            low_priority_queue_num: 1,
        }
    }

    pub fn set_hqnum(mut self, num: usize) -> Self {
        self.high_priority_queue_num = num;
        self
    }

    pub fn set_lqnum(mut self, num: usize) -> Self {
        self.low_priority_queue_num = num;
        self
    }

    pub fn run(&self) {     // 空のタスクを立ち上げて QUEUE を生成
        unsafe {    // 環境変数をグローバル変数として設定 (あまり良い方法ではない)
            std::env::set_var("HIGH_NUM", self.high_priority_queue_num.to_string());
            std::env::set_var("LOW_NUM", self.low_priority_queue_num.to_string());
        }
        let high = spawn_task(async {}, FutureType::High);
        let low = spawn_task(async {}, FutureType::Low);
        join!(high, low);
    }
}

///////////////////
const SERVER: Token = Token(0);
const CLIENT: Token = Token(1);

struct ServerFuture {
    server: TcpListener,
    poll: MioPoll,
}

impl Future for ServerFuture {
    type Output = String;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // socket から event を polling 
        // socket の polling time を 200ms に設定
        let mut events = Events::with_capacity(1);
        let _ = self.poll.poll(&mut events, Some(Duration::from_millis(200))).unwrap();

        for event in events.iter() {        // event ごとに処理
            if event.token() == SERVER && event.is_readable() {
                let (mut stream, _) = self.server.accept().unwrap();
                let mut buffer = [0u8; 1024];
                let mut received_data = Vec::new();

                loop {
                    match stream.read(&mut buffer) {
                        Ok(n) if n > 0 => {
                            received_data.extend_from_slice(&buffer[..n]);
                        },
                        Ok(_) => break,
                        Err(e) => {
                            eprintln!("Error reading from stream: {}", e);
                            break;
                        }
                    }
                }

                if !received_data.is_empty() {
                    let received_str = String::from_utf8_lossy(&received_data);
                    return Poll::Ready(received_str.to_string());
                }

                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        }
        
        cx.waker().wake_by_ref();
        return Poll::Pending;
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    Runtime::new().run();

    // server
    let addr = "127.0.0.1:13265".parse()?;
    let mut server = TcpListener::bind(addr)?;
    let mut stream = TcpStream::connect(server.local_addr()?)?;
    let poll: MioPoll = MioPoll::new()?;
    poll.registry().register(&mut server, SERVER, Interest::READABLE)?;

    let server_worker = ServerFuture {server, poll};
    let test = spawn_task!(server_worker);

    // client
    let mut client_poll: MioPoll = MioPoll::new()?;
    client_poll.registry().register(&mut stream, CLIENT, Interest::WRITABLE)?;

    let mut events = Events::with_capacity(128);
    let _ = client_poll.poll(&mut events, None).unwrap();
    for event in events.iter() {
        if event.token() == CLIENT && event.is_writable() {
            let message = "Hello World!\n";
            let _ = stream.write_all(message.as_bytes());
        }
    }

    let outcome = future::block_on(test);
    println!("outcome: {}", outcome);

    Ok(())
}