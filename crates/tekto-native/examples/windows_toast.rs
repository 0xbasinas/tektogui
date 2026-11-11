//! Windows only: show one toast. Run with Do not disturb on to test delivery.
#[cfg(windows)]
fn main() {
    use tekto_native::{native, Notifications};
    let r = native().show("Tekto toast", "Hello from the toast path");
    println!("show -> {r:?}");
    std::thread::sleep(std::time::Duration::from_secs(8));
}
#[cfg(not(windows))]
fn main() {
    eprintln!("windows only");
    std::process::exit(1);
}
