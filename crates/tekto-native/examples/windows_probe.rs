//! Opt-in interactive Windows native API probe; overwrites the clipboard.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use tekto_native::{native, Clipboard, Files};
    let host = native();
    let text = "Tekto λ 漢字 😀";
    host.set_text(text)?;
    assert_eq!(host.get_text()?, text);
    println!("Clipboard set and read: {text}");
    println!("Paste into another app now, then press Enter.");
    std::io::stdin().read_line(&mut String::new())?;
    println!("Copy Unicode text from another app, then press Enter.");
    std::io::stdin().read_line(&mut String::new())?;
    println!("Clipboard read: {:?}", host.get_text()?);
    host.write_string("windows-probe.txt", text)?;
    assert_eq!(host.read_to_string("windows-probe.txt")?, text);
    host.write_bytes("windows-probe.bin", &[0, 1, 127, 128, 255])?;
    assert_eq!(host.read_bytes("windows-probe.bin")?, [0, 1, 127, 128, 255]);
    assert!(host.write_string("../windows-probe-escape", "bad").is_err());
    println!("Text and bytes round-tripped; traversal rejected. Probe files remain in the tekto app-data directory.");
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This probe requires Windows.");
    std::process::exit(1);
}
