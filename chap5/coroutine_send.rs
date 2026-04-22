#![feature(coroutines, coroutine_trait)]
use std::time::Duration;
use std::ops::{Coroutine, CoroutineState};
use std::pin::Pin;

use rand::Rng;

struct RandCoroutine {
    pub value: u8,
    pub live: bool,
}

impl RandCoroutine {
    fn new() -> Self {
        let mut coroutine = Self {
            value: 0,
            live: true,
        };
        coroutine.generate();
        
        coroutine
    }

    fn generate(&mut self) {    // 0 ~ 10 の整数をランダムに生成
        self.value = rand::random_range(0..=10);
    }
}

impl Coroutine<()> for RandCoroutine {
    type Return = ();
    type Yield = u8;

    fn resume(mut self: Pin<&mut Self>, _arg: ()) -> CoroutineState<Self::Yield, Self::Return> {
        self.generate();
        CoroutineState::Yielded(self.value)
    }
}

fn main() {
    let (sender, receiver) = std::sync::mpsc::channel::<RandCoroutine>();
    let _thread = std::thread::spawn(move || {
        loop {
            let mut coroutine = match receiver.recv() {
                Ok(coroutine) => coroutine,
                Err(_) => break,
            };

            match Pin::new(&mut coroutine).resume(()) {
                CoroutineState::Yielded(result) => {
                    println!("Coroutine yielded: {}", result);
                },
                CoroutineState::Complete(_) => {
                    panic!("Coroutine should not complete");
                }
            }
        }
    });

    std::thread::sleep(Duration::from_secs(1));
    sender.send(RandCoroutine::new()).unwrap();
    sender.send(RandCoroutine::new()).unwrap();
    std::thread::sleep(Duration::from_secs(1));
}