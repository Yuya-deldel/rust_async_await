use std::sync::Arc;
use std::task::Wake;

// wake trait 実装による Waker の実装
pub struct MyWaker;
impl Wake for MyWaker {
    fn wake(self: Arc<Self>) {}     // waker を消費する
}   
    // wake_by_ref は wake から自動的に実装される: waker を消費しない
    // clone, drop にはデフォルト実装あり

// waker は何もせず、executor をただ無限に Polling し続ける 
// 無駄に CPU を浪費する実装