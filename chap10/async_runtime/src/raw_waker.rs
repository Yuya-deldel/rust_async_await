use std::task::{RawWaker, RawWakerVTable};

// RawWaker: 任意のデータ構造を指す raw pointer + それを操作する4つの関数テーブル
// 一般には直接 RawWaker を定義するのは非推奨 ~> wake trait を実装したほうが良い
// RawWaker が参照する仮想関数 pointer table
static VTABLE: RawWakerVTable = RawWakerVTable::new(my_clone, my_wake, my_wake_by_ref, my_drop);

unsafe fn my_clone(raw_waker: *const ()) -> RawWaker {
    RawWaker::new(raw_waker, &VTABLE)
}

unsafe fn my_wake(raw_waker: *const ()) {       // waker を消費する
    drop(Box::from_raw(raw_waker as *mut u32));
}

unsafe fn my_wake_by_ref(_raw_waker: *const ()) {}  // waker を消費しない

unsafe fn my_drop(raw_waker: *const ()) {
    drop(Box::from_raw(raw_waker as *mut u32));
}

pub fn create_raw_waker() -> RawWaker {
    let data = Box::into_raw(Box::new(42u32));  // dummy data
    RawWaker::new(data as *const (), &VTABLE)
}