use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::net::TcpStream;

pub struct TcpSender {
    pub stream: Arc<Mutex<TcpStream>>,
    pub buffer: Vec<u8>
}

impl Future for TcpSender {
    type Output = io::Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // stream を取得 + non-blocking にする
        let mut stream = match self.stream.try_lock() {
            Ok(stream) => stream,
            Err(_) => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };
        stream.set_nonblocking(true)?;
        
        // buffer を send
        match stream.write_all(&self.buffer) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            },
            Err(e) => Poll::Ready(Err(e))
        }
    }
}