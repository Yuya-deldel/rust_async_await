use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::net::TcpStream;
use std::io::{self, Read};

pub struct TcpReceiver {
    pub stream: Arc<Mutex<TcpStream>>,
    pub buffer: Vec<u8>
}

impl Future for TcpReceiver {
    type Output = io::Result<Vec<u8>>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // stream 取得
        let mut stream = match self.stream.try_lock() {
            Ok(stream) => stream,
            Err(_) => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };
        stream.set_nonblocking(true)?;
        
        // stream 読み込み
        let mut local_buf = [0; 1024];
        match stream.read(&mut local_buf) {
            Ok(0) => Poll::Ready(Ok(self.buffer.to_vec())),     // 読み込み終了
            Ok(n) => {
                std::mem::drop(stream);
                self.buffer.extend_from_slice(&local_buf[..n]);     // local_buf -- copy --> self.buffer
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            Err(e) => Poll::Ready(Err(e))
        }
    }
}