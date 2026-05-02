use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::oneshot;

struct RespMessage {
    value: i32,
    responder: oneshot::Sender<i64>
}

/* 
async fn basic_actor(mut rx: Receiver<Message>) {
    let mut state = 0;
    while let Some(msg) = rx.recv().await {
        state += msg.value;
        println!("Received: {}", msg.value);
        println!("State: {}", state);
    }
}
*/

async fn resp_actor(mut rx: Receiver<RespMessage>) {
    let mut state = 0;
    while let Some(msg) = rx.recv().await {
        state += msg.value;
        if msg.responder.send(state.into()).is_err() {
            eprintln!("Failed to send response");
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (tx, rx) = channel::<RespMessage>(100);
    let _actor_handle = tokio::spawn(async {
        resp_actor(rx).await;
    });

    for i in 0..10 {
        let (resp_tx, resp_rx) = oneshot::channel::<i64>();
        let msg = RespMessage {
            value: i,
            responder: resp_tx
        };
        tx.send(msg).await.unwrap();
        println!("Response: {}", resp_rx.await.unwrap());
    }
}