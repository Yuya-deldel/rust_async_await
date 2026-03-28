use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::sync::LazyLock;
use std::time::Duration;
use std::future::Future;
use std::panic::catch_unwind;
use std::thread;

use anyhow::{Context as _, Error, Result, bail};

use async_native_tls::TlsStream;
use http::Uri;
use hyper::{Body, Client, Request, Response};

use smol::{io, prelude::*, Async};
use async_task::{Runnable, Task};
use flume::{Receiver, Sender};
use futures_lite::future;

// 現バージョンではコンパイルできない

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

struct CustomExecutor;

impl<F: Future + Send + 'static> hyper::rt::Executor<F> for CustomExecutor {
    fn execute(&self, fut: F) {
        spawn_task!(async {
            println!("sending request");
            fut.await;
        }).detach();        // detach: task の pointer はタスクが完了してから drop される
    }
}

enum CustomStream {
    Plain(Async<TcpStream>),            // http (TCP)
    Tls(TlsStream<Async<TcpStream>>),   // https (TCP + SSL/TLS)
}

// hyper (http の実装) の Service trait を実装
#[derive(Clone)]
struct CustomConnector;

impl hyper::service::Service<Uri> for CustomConnector {
    type Response = CustomStream;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    // リクエストを処理できるかどうかを調べるために hyper が用いる関数
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))     // 単に Ready を返せばよい
    }

    // poll_ready() が Ok() を返したことを前提として使われる
    fn call(&mut self, uri: Uri) -> Self::Future {
        Box::pin(async move {       // ピン止めされた Future 型
            let host = uri.host().context("cannot parse host")?;
            match uri.scheme_str() {
                Some("http") => {
                    let socket_addr = {     // socket address を non-blocking に解決する
                        let host = host.to_string();
                        let port = uri.port_u16().unwrap_or(80);
                        smol::unblock(move || (host.as_str(), port).to_socket_addrs())
                            .await?.next().context("cannot resolve address")?
                    };
                    // TCP connect 
                    let stream = Async::<TcpStream>::connect(socket_addr).await?;

                    Ok(CustomStream::Plain(stream))
                },
                Some("https") => {
                    let socket_addr = {     // socket address を non-blocking に解決する
                        let host = host.to_string();
                        let port = uri.port_u16().unwrap_or(443);
                        smol::unblock(move || (host.as_str(), port).to_socket_addrs())
                            .await?.next().context("cannot resolve address")?
                    };
                    // TCP + SSL/TLS connect
                    let stream = Async::<TcpStream>::connect(socket_addr).await?;
                    let stream = async_native_tls::connect(host, stream).await?;

                    Ok(CustomStream::Tls(stream))
                }
                scheme => bail!("unsupported scheme: {:?}", scheme),
            }
        })
    }
}

impl tokio::io::AsyncRead for CustomStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            CustomStream::Plain(s) => {
                Pin::new(s).poll_read(cx, buf.initialize_unfilled()).map_ok(|size| {
                    buf.advance(size);
                })
            },
            CustomStream::Tls(s) => {
                Pin::new(s).poll_read(cx, buf.initialize_unfilled()).map_ok(|size| {
                    buf.advance(size);
                })
            }
        }
    }
}

impl tokio::io::AsyncWrite for CustomStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        match &mut *self {
            CustomStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            CustomStream::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }   
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            CustomStream::Plain(s) => Pin::new(s).poll_flush(cx),
            CustomStream::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            CustomStream::Plain(s) => {
                s.get_ref().shutdown(Shutdown::Write)?;
                Poll::Ready(Ok(()))
            },
            CustomStream::Tls(s) => Pin::new(s).poll_close(cx),
        }
    }
}

impl hyper::client::connect::Connection for CustomStream {
    fn connected(&self) -> hyper::client::connect::Connected {
        hyper::client::connect::Connected::new()
    }
}

async fn fetch(req: Request<Body>) -> Result<Response<Body>> {
    Ok(Client::builder()
        .executor(CustomExecutor)
        .build::<_, Body>(CustomConnector)      // <- 現バージョンではコンパイル不可
        .request(req)
        .await?
    )
}

fn main() {
    Runtime::new().run();

    let future = async {
        let req = Request::get("https://www.rust-lang.org").body(Body::empty()).unwrap();
        let response = fetch(req).await.unwrap();
        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let html = String::from_utf8(body_bytes.to_vec()).unwrap();

        println!("{}", html); 
    };

    let test = spawn_task!(future);
    let _outcome = future::block_on(test);
}