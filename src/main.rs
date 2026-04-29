use tokio::io;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::TcpListener;    // 非同期 TCP listener

#[tokio::main]
async fn main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:10000").await.unwrap();
    loop {
        let (mut socket, addr) = listener.accept().await?;
        println!("accept: {}", addr);

        tokio::spawn(async move {
            // set buffer 
            let (readsocket, writesocket) = socket.split();
            let mut reader = io::BufReader::new(readsocket);
            let mut writer = io::BufWriter::new(writesocket);

            let mut line = String::new();
            loop {
                line.clear();   // tokio の read_line() は末尾に文字列を追加していくため clear が必要
                match reader.read_line(&mut line).await {   // 一行読み込み
                    Ok(0) => {      // connection close
                        println!("closed: {}", addr);
                        return;
                    },
                    Ok(_) => {
                        print!("read: {}, {}", addr, line);
                        writer.write_all(line.as_bytes()).await.unwrap();
                    },
                    Err(e) => {
                        println!("error: {}, {}", addr, e);
                        return;
                    }
                }
            }
        });
    }
}