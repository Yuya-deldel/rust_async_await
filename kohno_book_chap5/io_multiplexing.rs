use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};    
use std::task::{Context, Poll, Waker};
use std::pin::Pin;

use futures::executor;
use futures::future::{BoxFuture, FutureExt};
use futures::task::{waker_ref, ArcWake};

use nix::errno::Errno;
use nix::sys::epoll::{
        epoll_create1, epoll_ctl, epoll_wait, 
        EpollCreateFlags, EpollEvent, EpollFlags, EpollOp
    };
use nix::sys::eventfd::{eventfd, EfdFlags};     // Linux のイベント通知用インターフェース
use nix::unistd::{read, write};

// Linux において、IO 多重化と async/await を組み合わせる例
// IOSelector へのリクエスト: queue に push
// IOSelector は eventfd で通知

fn write_eventfd(fd: RawFd, n: usize) {     // write syscall 呼び出し
    let ptr = &n as *const usize as *const u8;
    let val = unsafe {
        std::slice::from_raw_parts(ptr, std::mem::size_of_val(&n))
    };
    write(fd, &val).unwrap();
}

enum IOOps {    // request queue の中身
    ADD(EpollFlags, RawFd, Waker),  // epoll へ追加する情報
    REMOVE(RawFd)                   // epoll から削除
}

struct IOSelector {
    wakers: Mutex<HashMap<RawFd, Waker>>,   // fd と waker を関連付ける dict
    queue: Mutex<VecDeque<IOOps>>,          // IO-queue
    epfd: RawFd,                            // epoll の file descriptor
    event: RawFd,                           // eventfd の file descriptor
}

impl IOSelector {
    fn new() -> Arc<Self> {
        let selector = IOSelector {
            wakers: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            epfd: epoll_create1(EpollCreateFlags::empty()).unwrap(),
            event: eventfd(0, EfdFlags::empty()).unwrap()
        };
        let result = Arc::new(selector);
        let arc_selector = result.clone();

        // epoll 用の thread 作成
        std::thread::spawn(move || arc_selector.select());

        result
    }

    fn add_event(&self, flag: EpollFlags, fd: RawFd, waker: Waker, wakers: &mut HashMap<RawFd, Waker>) {
        // EPOLLONESHOT: 一度イベントが発生すると、その fd へのイベントは再設定されるまで通知されないようになる
        let mut ev = EpollEvent::new(flag | EpollFlags::EPOLLONESHOT, fd as u64);
        // epoll に登録
        if let Err(err) = epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, fd, &mut ev) {
            match err {
                nix::Error::Sys(Errno::EEXIST) => {     // 既に追加されていた場合 => 再設定
                    epoll_ctl(self.epfd, EpollOp::EpollCtlMod, fd, &mut ev).unwrap();
                },
                _ => panic!("epoll_ctl: {}", err)
            }
        }

        // fd-waker hashmap に登録
        assert!(!wakers.contains_key(&fd));
        wakers.insert(fd, waker);
    }

    fn rm_event(&self, fd: RawFd, wakers: &mut HashMap<RawFd, Waker>) {
        let mut ev = EpollEvent::new(EpollFlags::empty(), fd as u64);
        epoll_ctl(self.epfd, EpollOp::EpollCtlDel, fd, &mut ev).ok();   // epoll から削除
        wakers.remove(&fd);         // fd-waker hashmap から削除
    }

    fn select(&self) {      // 専用スレッドで fd の監視を行う関数
        // eventfd を epoll の監視対象に追加
        let mut ev = EpollEvent::new(EpollFlags::EPOLLIN, self.event as u64);
        epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, self.event, &mut ev).unwrap();

        let mut events = vec![EpollEvent::empty(); 1024];   // 発生したイベントの fd を格納する
        while let Ok(nfds) = epoll_wait(self.epfd, &mut events, -1) {
            let mut fd_to_waker = self.wakers.lock().unwrap();
            for n in 0..nfds {
                if events[n].data() == self.event as u64 {  // eventfd の場合
                    let mut queue = self.queue.lock().unwrap();
                    while let Some(op) = queue.pop_front() {    // request queue を処理
                        match op {
                            IOOps::ADD(flag, fd, waker) => self.add_event(flag, fd, waker, &mut fd_to_waker),
                            IOOps::REMOVE(fd) => self.rm_event(fd, &mut fd_to_waker),
                        }
                    }

                    let mut buf: [u8; 8] = [0; 8];
                    read(self.event, &mut buf).unwrap();    // eventfd の通知を clear
                } else {    // 普通の file descriptor
                    let data = events[n].data() as i32;
                    let waker = fd_to_waker.remove(&data).unwrap();
                    waker.wake_by_ref();        // waker に通知
                }
            }
        }
    }

    fn register(&self, flags: EpollFlags, fd: RawFd, waker: Waker) {
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(IOOps::ADD(flags, fd, waker));      // fd を IOSelector に登録
        write_eventfd(self.event, 1); 
    }

    fn unregister(&self, fd: RawFd) {
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(IOOps::REMOVE(fd));     // fd を IOSelector から削除
        write_eventfd(self.event, 1);
    }
}

//////////////////////////////////////////////////////////////
// future 型の定義
struct AsyncListener {  // 非同期に TCP の listen/accept を行う future 型
    listener: TcpListener, 
    selector: Arc<IOSelector>
}

impl AsyncListener {    // listen/accept の非同期処理を定義
    // 初期化処理の wrapper
    fn listen(addr: &str, selector: Arc<IOSelector>) -> AsyncListener {
        let listener = TcpListener::bind(addr).unwrap();
        listener.set_nonblocking(true).unwrap();    // non-blocking に設定

        AsyncListener { listener, selector }
    }

    // accept().await とすると実際の accept が行われるように future 型を返すだけ
    fn accept(&self) -> Accept {
        Accept {listener: self}
    }
}

impl Drop for AsyncListener {
    fn drop(&mut self) {
        self.selector.unregister(self.listener.as_raw_fd());    // epoll への登録を解除
    }
}

struct Accept<'a>  {
    listener: &'a AsyncListener
}

impl<'a> Future for Accept<'a> {
    type Output = (AsyncReader, BufWriter<TcpStream>, SocketAddr);
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.listener.listener.accept() {     // TCP の accept() 処理
            Ok((stream, addr)) => {
                let read_stream = stream.try_clone().unwrap();

                Poll::Ready((
                    AsyncReader::new(read_stream, self.listener.selector.clone()),
                    BufWriter::new(stream),
                    addr
                ))
            },
            Err(err) => {
                // accept すべき connection がない -> epoll の監視対象に登録し、Pending
                if err.kind() == std::io::ErrorKind::WouldBlock {   // connection がない時の Error 型
                    self.listener.selector.register(
                        EpollFlags::EPOLLIN,
                        self.listener.listener.as_raw_fd(),
                        cx.waker().clone(),
                    );

                    Poll::Pending
                } else {
                    panic!("accept: {}", err);
                }
            }
        }
    }
}

struct AsyncReader {
    fd: RawFd, 
    reader: BufReader<TcpStream>,
    selector: Arc<IOSelector>
}

impl AsyncReader {      // 一行読み込む future 型
    fn new(stream: TcpStream, selector: Arc<IOSelector>) -> AsyncReader {
        stream.set_nonblocking(true).unwrap();      // non-blocking に設定
        AsyncReader { 
            fd: stream.as_raw_fd(), 
            reader: BufReader::new(stream), 
            selector 
        }
    }

    fn read_line(&mut self) -> ReadLine {       // 一行読み込む future を返す
        ReadLine {reader: self}
    }
}

impl Drop for AsyncReader {
    fn drop(&mut self) {
        self.selector.unregister(self.fd);
    }
}

struct ReadLine<'a> {
    reader: &'a mut AsyncReader
}

impl<'a> Future for ReadLine<'a> {
    type Output = Option<String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut line = String::new();
        match self.reader.reader.read_line(&mut line) {     // 一行読み込み実行
            Ok(0) => Poll::Ready(None),         // connection close
            Ok(_) => Poll::Ready(Some(line)),   // 読み込み成功
            Err(err) => {
                // accept すべき connection がない -> epoll の監視対象に登録し、Pending
                if err.kind() == std::io::ErrorKind::WouldBlock {  
                    self.reader.selector.register(
                        EpollFlags::EPOLLIN, 
                        self.reader.fd,
                        cx.waker().clone()
                    );
                    Poll::Pending
                } else {
                    Poll::Ready(None)       // connection close
                }
            }
        }
    }
}

////////////////////////////////////////////////////////////////
// Task & Scheduling
struct Task {   // Task と Waker を兼ねる
    future: Mutex<BoxFuture<'static, ()>>,  // 実行すべきコルーチン
    sender: SyncSender<Arc<Task>>           // Executor に Task を渡すためのチャネル
}

impl ArcWake for Task {     // プロセスのスケジューリングを行う trait
    fn wake_by_ref(arc_self: &Arc<Self>) {      // 自身の Arc を Executor に送信
        let self2 = arc_self.clone();
        arc_self.sender.send(self2).unwrap();
    }
}

struct Executor {       // 送受信チャネルの端点の情報を保持
    sender: SyncSender<Arc<Task>>,
    receiver: Receiver<Arc<Task>>
}

impl Executor {
    fn new() -> Self {
        // chanell のキューの最大個数を 1024 に設定
        let (sender, receiver) = sync_channel(1024);
        Executor { 
            sender: sender.clone(), 
            receiver 
        }
    }

    fn get_spawner(&self) -> Spawner {      // 新たな Task を生成する Spawner を生成
        Spawner {
            sender: self.sender.clone()
        }
    }

    fn run(&self) {
        while let Ok(task) = self.receiver.recv() {     // Task を受信
            // future: 実行すべきコルーチン
            let mut future = task.future.lock().unwrap();
            let waker = waker_ref(&task);   // task から Waker 作成
            let mut ctx = Context::from_waker(&waker);  // Waker から Context 作成
            let _ = future.as_mut().poll(&mut ctx);     // poll を実行
        }
    }
}

struct Spawner {    // 実行キューに追加するための端点を保持
    sender: SyncSender<Arc<Task>>
}

impl Spawner {      
    // future を Task に包んで実行キューに追加する
    fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let future = future.boxed();
        let task = Arc::new(Task {
            future: Mutex::new(future),
            sender: self.sender.clone()
        });

        self.sender.send(task).unwrap();
    }
}

fn main() {
    let executor = Executor::new();
    let selector = IOSelector::new();
    let spawner = executor.get_spawner();

    let server = async move {
        // listen()
        let listener = AsyncListener::listen("127.0.0.1:10000", selector.clone());
        loop {
            // 非同期 accept
            let (mut reader, mut writer, addr) = listener.accept().await;
            println!("accept: {}", addr);
            
            spawner.spawn(async move {
                // 非同期一行読み込み
                while let Some(buf) = reader.read_line().await {
                    print!("read: {}, {}", addr, buf);
                    writer.write(buf.as_bytes()).unwrap();
                    writer.flush().unwrap();
                }
                println!("close: {}", addr);
            });
        }
    };

    executor.get_spawner().spawn(server);
    executor.run();
}