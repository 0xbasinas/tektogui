//! Opt-in store history. Undo, redo, or jump without a window.
//!
//! `Store::new` keeps one value. This example uses `Store::recording`.

use std::sync::{Arc, Mutex};
use tekto::Store;

fn main() {
    let store = Arc::new(Mutex::new(Store::recording(
        serde_json::json!({"count": 0}),
        16,
    )));
    {
        let mut history = store.lock().expect("store");
        history.set(serde_json::json!({"count": 1}), Some("one".into()));
        history.set(serde_json::json!({"count": 2}), None);
        history.set(serde_json::json!({"count": 3}), Some("three".into()));
        println!("now {}", history.get()["count"]);
        history.undo().expect("undo");
        history.undo().expect("undo");
        println!("back {}", history.get()["count"]);
        history.jump(3).expect("jump");
        println!("restored {}", history.get()["count"]);
        for frame in history.list() {
            let mark = if frame.current { "*" } else { " " };
            println!("{mark} {} {}", frame.index, frame.label.unwrap_or_default());
        }
    }
    println!("history frames stay on the store; apps that use Store::new do not keep them");
}
