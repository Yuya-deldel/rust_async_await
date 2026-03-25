use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::task::JoinHandle;
use futures_util::future::join_all;

type AsyncFileHandle = Arc<Mutex<File>>;
type FileJoinHandle = JoinHandle<Result<bool, String>>;

fn get_handle(file_path: &dyn ToString) -> AsyncFileHandle {
    match OpenOptions::new().append(true).open(file_path.to_string()) {
        Ok(opened_file) => Arc::new(Mutex::new(opened_file)),
        Err(_) => Arc::new(Mutex::new(File::create(file_path.to_string()).unwrap())),
    }
}

struct AsyncWriteFuture {
    pub handle: AsyncFileHandle,
    pub entry: String,
}

impl Future for AsyncWriteFuture {
    type Output = Result<bool, String>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = match self.handle.try_lock() {
            Ok(guard) => guard,
            Err(error) => {
                println!("error for {} : {}", self.entry, error);
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };
        let lined_entry = format!("{}\n", self.entry);
        match guard.write_all(lined_entry.as_bytes()) {
            Ok(_) => println!("written for: {}", self.entry),
            Err(e) => println!("{}", e),
        };
        return Poll::Ready(Ok(true));
    }
}

fn write_log(file_handle: AsyncFileHandle, line: String) -> FileJoinHandle {
    let future = AsyncWriteFuture {
        handle: file_handle,
        entry: line,
    };
    tokio::task::spawn(async move {
        future.await
    })
}

#[tokio::main]
async fn main() {
    let login_handle = get_handle(&"login.txt");
    let logout_handle = get_handle(&"logout.txt");

    let names = ["one", "two", "three", "four", "five", "six"];
    let mut handles = Vec::new();
    for name in names {
        let login_handle = login_handle.clone();
        let logout_handle = logout_handle.clone();
        let write_login_hdr = write_log(login_handle, name.to_string());
        let write_logout_hdr = write_log(logout_handle, name.to_string());

        handles.push(write_login_hdr);
        handles.push(write_logout_hdr);
    }
    join_all(handles).await;
}