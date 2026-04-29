use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::TcpListener;
use std::collections::HashMap;
use std::os::unix::io::{AsRawFd, RawFd};

use nix::sys::epoll::{epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollEvent, EpollFlags, EpollOp};


// simple echo server: 一つの処理が完了するまで次の処理ができない
/* 
fn main() {
    // TCP: localhost; port: 10000 で bind & listen
    let listener = TcpListener::bind("127.0.0.1:10000").unwrap();
    
    // connection 要求を accept
    while let Ok((stream, _)) = listener.accept() {
        let read_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(read_stream);
        let mut writer = BufWriter::new(stream);

        let mut buf = String::new();
        reader.read_line(&mut buf).unwrap();
        writer.write(buf.as_bytes()).unwrap();
        writer.flush().unwrap();
    }
}   */

// concurrent server by epoll; blocking ver; linux 用
fn main() {
    // TCP: localhost; port: 10000 で bind & listen
    let listener = TcpListener::bind("127.0.0.1:10000").unwrap();

    // epoll 用 file descriptor 作成
    let epfd = epoll_create1(EpollCreateFlags::empty()).unwrap();
    
    // listen 用 socket を epoll に登録
    let listen_fd = listener.as_raw_fd();
    let mut ev = EpollEvent::new(EpollFlags::EPOLLIN, listen_fd as u64);
    epoll_ctl(epfd, EpollOp::EpollCtlAdd, listen_fd, &mut ev).unwrap();

    let mut fd_to_buf = HashMap::new();
    let mut events = vec![EpollEvent::empty(); 1024];
    // epoll_wait(epfd, events, timeout) -> 発生したイベントの数
    // events にイベントが発生した file descriptor が書き込まれる
    while let Ok(nfds) = epoll_wait(epfd, &mut events, -1) {
        for n in 0..nfds {
            if events[n].data() == listen_fd as u64 {   // listen socket のイベント
                if let Ok((stream, _)) = listener.accept() {
                    let fd = stream.as_raw_fd();
                    let read_stream = stream.try_clone().unwrap();
                    let reader = BufReader::new(read_stream);
                    let writer = BufWriter::new(stream);
                    fd_to_buf.insert(fd, (reader, writer));     // fd から reader_buf, writer_buf を探せるようにする

                    println!("accept: fd = {}", fd);

                    // fd を epoll で監視するように登録
                    let mut ev = EpollEvent::new(EpollFlags::EPOLLIN, fd as u64);
                    epoll_ctl(epfd, EpollOp::EpollCtlAdd, fd, &mut ev);
                }
            } else {    // client からのデータ到着
                let fd = events[n].data() as RawFd;
                // fd から reader_buf, writer_buf を探す
                let (reader, writer) = fd_to_buf.get_mut(&fd).unwrap();
                
                // 一行ずつ読み込み
                let mut buf = String::new();
                let n = reader.read_line(&mut buf).unwrap();
                if n == 0 {     // connection is closed
                    // epoll の監視対象から外す
                    let mut ev = EpollEvent::new(EpollFlags::EPOLLIN, fd as u64);
                    epoll_ctl(epfd, EpollOp::EpollCtlDel, fd, &mut ev).unwrap();
                    fd_to_buf.remove(&fd);

                    println!("closed: fd = {}", fd);

                    continue;
                }   // 一行 read に成功

                print!("read: fd = {}, buf = {}", fd, buf);
                writer.write(buf.as_bytes()).unwrap();
                writer.flush().unwrap();
            }
        }
    }
}