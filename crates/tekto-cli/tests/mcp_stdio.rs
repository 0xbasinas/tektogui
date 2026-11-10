use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn stdio_subprocess_recovers_and_keeps_stdout_protocol_only() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tekto"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"{bad}\n{\"jsonrpc\":\"2.0\",\"method\":\"unknown\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":1}\r\n").unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let messages: Vec<serde_json::Value> = output
        .stdout
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["error"]["code"], -32700);
    assert_eq!(messages[1]["id"], 1);
}
