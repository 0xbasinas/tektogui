//! Linux desktop notifications over the freedesktop notification spec.
//!
//! Talks to `org.freedesktop.Notifications.Notify` on the session bus with a
//! tiny D-Bus client (no extra crate). A missing bus or a missing notification
//! service is [`NativeError::Unavailable`], not [`NativeError::Unsupported`].
//! This module does not open an X11 or Wayland display.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::{current_platform, NativeError};

const CAPABILITY: &str = "notifications.show";
const REASON: &str = "no session bus or notification daemon";
const IO_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_MESSAGE: usize = 1024 * 1024;

pub(crate) fn show(title: &str, body: &str) -> Result<(), NativeError> {
    if title.contains('\0') || body.contains('\0') {
        return Err(NativeError::Message(
            "notifications.show: text contains a NUL byte".into(),
        ));
    }
    let addresses = session_addresses();
    if addresses.is_empty() {
        return Err(unavailable());
    }
    for address in &addresses {
        match show_at(address, title, body) {
            Ok(()) => return Ok(()),
            Err(ShowError::Unavailable) => return Err(unavailable()),
            Err(ShowError::Connect) => {}
        }
    }
    Err(unavailable())
}

fn unavailable() -> NativeError {
    NativeError::Unavailable {
        capability: CAPABILITY,
        platform: current_platform(),
        reason: REASON,
    }
}

#[derive(Debug)]
enum ShowError {
    Connect,
    Unavailable,
}

/// One session-bus address (`unix:path=...` or `unix:abstract=...`).
fn show_at(address: &str, title: &str, body: &str) -> Result<(), ShowError> {
    let mut stream = connect(address).map_err(|_| ShowError::Connect)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|_| ShowError::Connect)?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|_| ShowError::Connect)?;
    authenticate(&mut stream)?;
    let hello = build_method_call(
        1,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "Hello",
        None,
        &[],
    );
    stream.write_all(&hello).map_err(|_| ShowError::Connect)?;
    let hello_reply = read_reply(&mut stream, 1)?;
    if hello_reply.error_name.is_some() {
        return Err(ShowError::Unavailable);
    }
    let payload = notify_body(title, body);
    let notify = build_method_call(
        2,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        Some("susssasa{sv}i"),
        &payload,
    );
    stream.write_all(&notify).map_err(|_| ShowError::Connect)?;
    let reply = read_reply(&mut stream, 2)?;
    if reply.error_name.is_some() || reply.message_type != 2 {
        return Err(ShowError::Unavailable);
    }
    Ok(())
}

fn session_addresses() -> Vec<String> {
    if let Ok(raw) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        let list: Vec<String> = raw
            .split(';')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
        if !list.is_empty() {
            return list;
        }
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let bus = Path::new(&runtime).join("bus");
        if bus.exists() {
            return vec![format!("unix:path={}", bus.display())];
        }
    }
    Vec::new()
}

fn connect(address: &str) -> std::io::Result<UnixStream> {
    let spec = address.split(';').next().unwrap_or(address);
    let Some(rest) = spec.strip_prefix("unix:") else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unsupported dbus address",
        ));
    };
    let mut path = None;
    let mut abstract_name = None;
    for part in rest.split(',') {
        if let Some(value) = part.strip_prefix("path=") {
            path = Some(unescape(value));
        } else if let Some(value) = part.strip_prefix("abstract=") {
            abstract_name = Some(unescape(value));
        }
    }
    if let Some(path) = path {
        return UnixStream::connect(path);
    }
    if let Some(name) = abstract_name {
        use std::os::linux::net::SocketAddrExt;
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
        return UnixStream::connect_addr(&addr);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "dbus unix address has no path",
    ))
}

fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn authenticate(stream: &mut UnixStream) -> Result<(), ShowError> {
    // SAFETY: `getuid` is a pure read of the process credential and cannot fail.
    let uid = unsafe { libc::getuid() };
    let hex_uid = hex_encode(&uid.to_string());
    let auth = format!("\0AUTH EXTERNAL {hex_uid}\r\n");
    stream
        .write_all(auth.as_bytes())
        .map_err(|_| ShowError::Connect)?;
    let line = read_auth_line(stream)?;
    if !line.starts_with("OK") {
        return Err(ShowError::Unavailable);
    }
    stream
        .write_all(b"BEGIN\r\n")
        .map_err(|_| ShowError::Connect)?;
    Ok(())
}

fn hex_encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn read_auth_line(stream: &mut UnixStream) -> Result<String, ShowError> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).map_err(|_| ShowError::Connect)?;
        if n == 0 {
            return Err(ShowError::Connect);
        }
        if byte[0] == b'\n' {
            break;
        }
        if buf.len() > 512 {
            return Err(ShowError::Unavailable);
        }
        buf.push(byte[0]);
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

struct Reply {
    message_type: u8,
    error_name: Option<String>,
}

fn read_reply(stream: &mut UnixStream, serial: u32) -> Result<Reply, ShowError> {
    for _ in 0..8 {
        let message = read_message(stream)?;
        if reply_serial(&message) != Some(serial) {
            continue;
        }
        return Ok(Reply {
            message_type: message[1],
            error_name: header_string(&message, 4),
        });
    }
    Err(ShowError::Unavailable)
}

fn read_message(stream: &mut UnixStream) -> Result<Vec<u8>, ShowError> {
    let mut header = [0u8; 16];
    stream
        .read_exact(&mut header)
        .map_err(|_| ShowError::Connect)?;
    let big = header[0] == b'B';
    let read_u32 = |bytes: &[u8]| {
        let raw = [bytes[0], bytes[1], bytes[2], bytes[3]];
        if big {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        }
    };
    let body_len = read_u32(&header[4..8]) as usize;
    let fields_len = read_u32(&header[12..16]) as usize;
    if body_len.saturating_add(fields_len) > MAX_MESSAGE {
        return Err(ShowError::Unavailable);
    }
    let mut message = Vec::with_capacity(16 + fields_len + body_len + 8);
    message.extend_from_slice(&header);
    let mut fields = vec![0u8; fields_len];
    stream
        .read_exact(&mut fields)
        .map_err(|_| ShowError::Connect)?;
    message.extend_from_slice(&fields);
    let mut pad = [0u8; 1];
    while !message.len().is_multiple_of(8) {
        stream
            .read_exact(&mut pad)
            .map_err(|_| ShowError::Connect)?;
        message.push(pad[0]);
    }
    let mut body = vec![0u8; body_len];
    stream
        .read_exact(&mut body)
        .map_err(|_| ShowError::Connect)?;
    message.extend_from_slice(&body);
    Ok(message)
}

fn reply_serial(message: &[u8]) -> Option<u32> {
    header_u32(message, 5)
}

fn header_string(message: &[u8], code: u8) -> Option<String> {
    let mut found = None;
    walk_headers(message, |field, sig, value| {
        if field == code && sig == "s" {
            if let FieldValue::Str(text) = value {
                found = Some(text);
            }
        }
    });
    found
}

fn header_u32(message: &[u8], code: u8) -> Option<u32> {
    let mut found = None;
    walk_headers(message, |field, sig, value| {
        if field == code && sig == "u" {
            if let FieldValue::U32(number) = value {
                found = Some(number);
            }
        }
    });
    found
}

enum FieldValue {
    Str(String),
    U32(u32),
}

fn walk_headers(message: &[u8], mut visit: impl FnMut(u8, &str, FieldValue)) {
    if message.len() < 16 || (message[0] != b'l' && message[0] != b'B') {
        return;
    }
    let big = message[0] == b'B';
    let Some(fields_len) = read_at_u32(message, 12, big) else {
        return;
    };
    let end = 16 + fields_len as usize;
    if end > message.len() {
        return;
    }
    let mut pos = 16;
    while pos < end {
        while pos % 8 != 0 {
            pos += 1;
        }
        if pos >= end {
            break;
        }
        let code = message[pos];
        pos += 1;
        let Some((sig, next)) = read_signature(message, pos) else {
            return;
        };
        pos = next;
        let Some((value, next)) = read_field_value(message, pos, &sig, big) else {
            return;
        };
        pos = next;
        visit(code, &sig, value);
    }
}

fn read_at_u32(message: &[u8], at: usize, big: bool) -> Option<u32> {
    let bytes: [u8; 4] = message.get(at..at + 4)?.try_into().ok()?;
    Some(if big {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    })
}

fn read_signature(message: &[u8], pos: usize) -> Option<(String, usize)> {
    let len = *message.get(pos)? as usize;
    let start = pos + 1;
    let end = start + len;
    let text = std::str::from_utf8(message.get(start..end)?).ok()?;
    if *message.get(end)? != 0 {
        return None;
    }
    Some((text.to_string(), end + 1))
}

fn read_field_value(
    message: &[u8],
    mut pos: usize,
    sig: &str,
    big: bool,
) -> Option<(FieldValue, usize)> {
    match sig {
        "s" | "o" => {
            pos = align(pos, 4);
            let len = read_at_u32(message, pos, big)? as usize;
            pos += 4;
            let text = std::str::from_utf8(message.get(pos..pos + len)?).ok()?;
            pos += len;
            if *message.get(pos)? != 0 {
                return None;
            }
            Some((FieldValue::Str(text.to_string()), pos + 1))
        }
        "u" => {
            pos = align(pos, 4);
            let number = read_at_u32(message, pos, big)?;
            Some((FieldValue::U32(number), pos + 4))
        }
        "g" => {
            let (text, next) = read_signature(message, pos)?;
            Some((FieldValue::Str(text), next))
        }
        _ => None,
    }
}

fn align(pos: usize, alignment: usize) -> usize {
    let rem = pos % alignment;
    if rem == 0 {
        pos
    } else {
        pos + (alignment - rem)
    }
}

fn notify_body(title: &str, body: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    write_str(&mut buf, "tekto");
    write_u32(&mut buf, 0);
    write_str(&mut buf, "");
    write_str(&mut buf, title);
    write_str(&mut buf, body);
    write_array(&mut buf, 4, |_| {});
    write_array(&mut buf, 8, |_| {});
    write_i32(&mut buf, -1);
    buf
}

fn build_method_call(
    serial: u32,
    destination: &str,
    path: &str,
    interface: &str,
    member: &str,
    body_sig: Option<&str>,
    body: &[u8],
) -> Vec<u8> {
    let mut fields = Vec::new();
    write_field_string(&mut fields, 1, "o", path);
    write_field_string(&mut fields, 2, "s", interface);
    write_field_string(&mut fields, 3, "s", member);
    write_field_string(&mut fields, 6, "s", destination);
    if let Some(sig) = body_sig {
        write_field_signature(&mut fields, sig);
    }
    finish_message(1, serial, &fields, body)
}

#[cfg(test)]
fn build_method_return(serial: u32, reply_to: u32, body_sig: &str, body: &[u8]) -> Vec<u8> {
    let mut fields = Vec::new();
    write_field_u32(&mut fields, 5, reply_to);
    write_field_signature(&mut fields, body_sig);
    finish_message(2, serial, &fields, body)
}

#[cfg(test)]
fn build_error(serial: u32, reply_to: u32, name: &str, message: &str) -> Vec<u8> {
    let mut fields = Vec::new();
    write_field_string(&mut fields, 4, "s", name);
    write_field_u32(&mut fields, 5, reply_to);
    write_field_signature(&mut fields, "s");
    let mut body = Vec::new();
    write_str(&mut body, message);
    finish_message(3, serial, &fields, &body)
}

fn finish_message(message_type: u8, serial: u32, fields: &[u8], body: &[u8]) -> Vec<u8> {
    let mut message = vec![0u8; 16];
    message[0] = b'l';
    message[1] = message_type;
    message[3] = 1;
    message[4..8].copy_from_slice(&(body.len() as u32).to_le_bytes());
    message[8..12].copy_from_slice(&serial.to_le_bytes());
    message[12..16].copy_from_slice(&(fields.len() as u32).to_le_bytes());
    message.extend_from_slice(fields);
    while !message.len().is_multiple_of(8) {
        message.push(0);
    }
    message.extend_from_slice(body);
    message
}

fn write_field_string(buf: &mut Vec<u8>, code: u8, sig: &str, value: &str) {
    pad(buf, 8);
    buf.push(code);
    write_signature(buf, sig);
    write_str(buf, value);
}

fn write_field_signature(buf: &mut Vec<u8>, sig: &str) {
    pad(buf, 8);
    buf.push(8);
    write_signature(buf, "g");
    write_signature(buf, sig);
}

#[cfg(test)]
fn write_field_u32(buf: &mut Vec<u8>, code: u8, value: u32) {
    pad(buf, 8);
    buf.push(code);
    write_signature(buf, "u");
    write_u32(buf, value);
}

fn write_signature(buf: &mut Vec<u8>, sig: &str) {
    buf.push(sig.len() as u8);
    buf.extend_from_slice(sig.as_bytes());
    buf.push(0);
}

fn write_array(buf: &mut Vec<u8>, element_align: usize, write_elements: impl FnOnce(&mut Vec<u8>)) {
    pad(buf, 4);
    let len_at = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());
    pad(buf, element_align);
    let start = buf.len();
    write_elements(buf);
    let len = (buf.len() - start) as u32;
    buf[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
}

fn write_str(buf: &mut Vec<u8>, text: &str) {
    pad(buf, 4);
    let bytes = text.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
    buf.push(0);
}

fn write_u32(buf: &mut Vec<u8>, value: u32) {
    pad(buf, 4);
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_i32(buf: &mut Vec<u8>, value: i32) {
    write_u32(buf, value as u32);
}

fn pad(buf: &mut Vec<u8>, alignment: usize) {
    while !buf.len().is_multiple_of(alignment) {
        buf.push(0);
    }
}

/// True when a live notification must not be sent.
///
/// Displays `:1` and `:2` are reserved in this repo. A real session check is
/// opt-in via `TEKTO_NOTIFY_LIVE`.
#[cfg(test)]
pub(crate) fn skip_live_notify() -> bool {
    if std::env::var_os("TEKTO_NOTIFY_LIVE").is_some() {
        let display = std::env::var("DISPLAY").unwrap_or_default();
        return matches!(display.as_str(), ":1" | ":2" | ":1.0" | ":2.0");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn missing_daemon_on_a_bus_is_unavailable() {
        let dir = std::env::temp_dir().join(format!("tekto-notify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("bus");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || serve_no_daemon(listener));
        let err = show_at(&format!("unix:path={}", socket.display()), "Hi", "There")
            .expect_err("no daemon");
        assert!(matches!(err, ShowError::Unavailable));
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notify_round_trip_against_a_fake_daemon() {
        let dir = std::env::temp_dir().join(format!("tekto-notify-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("bus");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || serve_ok(listener));
        show_at(&format!("unix:path={}", socket.display()), "Hi", "There").unwrap();
        let seen = server.join().unwrap();
        assert!(seen.contains("Notify"), "{seen}");
        assert!(seen.contains("Hi"), "{seen}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn serve_no_daemon(listener: UnixListener) {
        let (mut stream, _) = listener.accept().unwrap();
        expect_auth(&mut stream);
        let hello = read_message(&mut stream).unwrap();
        assert_eq!(header_string(&hello, 3).as_deref(), Some("Hello"));
        let reply = build_method_return(1, 1, "s", &string_body(":1.42"));
        stream.write_all(&reply).unwrap();
        let notify = read_message(&mut stream).unwrap();
        assert_eq!(header_string(&notify, 3).as_deref(), Some("Notify"));
        let err = build_error(
            2,
            2,
            "org.freedesktop.DBus.Error.ServiceUnknown",
            "The name org.freedesktop.Notifications was not provided by any .service files",
        );
        stream.write_all(&err).unwrap();
    }

    fn serve_ok(listener: UnixListener) -> String {
        let (mut stream, _) = listener.accept().unwrap();
        expect_auth(&mut stream);
        let _hello = read_message(&mut stream).unwrap();
        let reply = build_method_return(1, 1, "s", &string_body(":1.7"));
        stream.write_all(&reply).unwrap();
        let notify = read_message(&mut stream).unwrap();
        let text = String::from_utf8_lossy(&notify).into_owned();
        let id = {
            let mut body = Vec::new();
            write_u32(&mut body, 7);
            body
        };
        let reply = build_method_return(2, 2, "u", &id);
        stream.write_all(&reply).unwrap();
        text
    }

    fn expect_auth(stream: &mut UnixStream) {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).unwrap();
        assert_eq!(byte[0], 0);
        loop {
            stream.read_exact(&mut byte).unwrap();
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
        }
        let line = String::from_utf8_lossy(&buf);
        assert!(line.starts_with("AUTH EXTERNAL "), "{line}");
        stream
            .write_all(b"OK 0123456789abcdef0123456789abcdef\r\n")
            .unwrap();
        buf.clear();
        loop {
            stream.read_exact(&mut byte).unwrap();
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&buf).starts_with("BEGIN"));
    }

    fn string_body(text: &str) -> Vec<u8> {
        let mut body = Vec::new();
        write_str(&mut body, text);
        body
    }
}
