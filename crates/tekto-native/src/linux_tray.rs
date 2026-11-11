//! Linux system tray over the StatusNotifierItem spec.
//!
//! Compiled only with the `tray` feature. Talks to
//! `org.kde.StatusNotifierWatcher` on the session bus and serves a small
//! D-Bus menu. No GTK, libappindicator, or extra crate. A missing bus or
//! watcher is [`NativeError::Unavailable`]. This module does not open an
//! X11 or Wayland display.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::{current_platform, NativeError, TrayMenuItem};

const SHOW: &str = "tray.show";
const REASON: &str = "no session bus or status notifier watcher";
const IO_TIMEOUT: Duration = Duration::from_millis(400);
const MAX_MESSAGE: usize = 1024 * 1024;
const MAX_ITEMS: usize = 16;
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/Menu";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const SNI_IFACE: &str = "org.kde.StatusNotifierItem";
const MENU_IFACE: &str = "com.canonical.dbusmenu";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

struct State {
    label: String,
    items: Vec<TrayMenuItem>,
    clicks: VecDeque<String>,
    on_click: Option<fn(&str)>,
    revision: u32,
    shown: bool,
    stream: Option<UnixStream>,
    thread: Option<JoinHandle<()>>,
}

struct Tray {
    stop: AtomicBool,
    next_serial: AtomicU32,
    state: Mutex<State>,
}

fn tray() -> &'static Tray {
    static TRAY: OnceLock<Tray> = OnceLock::new();
    TRAY.get_or_init(|| Tray {
        stop: AtomicBool::new(false),
        next_serial: AtomicU32::new(3),
        state: Mutex::new(State {
            label: String::new(),
            items: Vec::new(),
            clicks: VecDeque::new(),
            on_click: None,
            revision: 1,
            shown: false,
            stream: None,
            thread: None,
        }),
    })
}

fn lock_state() -> std::sync::MutexGuard<'static, State> {
    tray()
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

pub(crate) fn set_label(label: &str) -> Result<(), NativeError> {
    reject_nul(label, "tray.set_label")?;
    let mut state = lock_state();
    state.label = label.to_string();
    let stream = if state.shown {
        state
            .stream
            .as_ref()
            .and_then(|stream| stream.try_clone().ok())
    } else {
        None
    };
    drop(state);
    if let Some(mut stream) = stream {
        let serial = tray().next_serial.fetch_add(2, Ordering::Relaxed);
        let _ = write_title_signals(&mut stream, serial, label);
    }
    Ok(())
}

pub(crate) fn set_on_click(on_click: Option<fn(&str)>) -> Result<(), NativeError> {
    lock_state().on_click = on_click;
    Ok(())
}

pub(crate) fn take_click() -> Result<Option<String>, NativeError> {
    Ok(lock_state().clicks.pop_front())
}

pub(crate) fn hide_icon() -> Result<(), NativeError> {
    stop_icon();
    Ok(())
}

pub(crate) fn show_icon(label: &str, menu: &[TrayMenuItem]) -> Result<(), NativeError> {
    let addresses = session_addresses();
    if addresses.is_empty() {
        return Err(unavailable());
    }
    let mut last = unavailable();
    for address in &addresses {
        match show_at(address, label, menu) {
            Ok(()) => return Ok(()),
            Err(err) => last = err,
        }
    }
    Err(last)
}

fn show_at(address: &str, label: &str, menu: &[TrayMenuItem]) -> Result<(), NativeError> {
    reject_nul(label, SHOW)?;
    if menu.len() > MAX_ITEMS {
        return Err(NativeError::Message(format!(
            "{SHOW}: menu has more than {MAX_ITEMS} items"
        )));
    }
    let mut seen = Vec::new();
    for item in menu {
        reject_nul(&item.id, SHOW)?;
        reject_nul(&item.label, SHOW)?;
        if item.id.is_empty() {
            return Err(NativeError::Message(format!("{SHOW}: menu id is empty")));
        }
        if seen.iter().any(|id| id == &item.id) {
            return Err(NativeError::Message(format!(
                "{SHOW}: duplicate menu id {}",
                item.id
            )));
        }
        seen.push(item.id.as_str());
    }

    stop_icon();
    let mut stream = connect(address).map_err(|_| unavailable())?;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    authenticate(&mut stream).map_err(|_| unavailable())?;
    let hello = build_method_call(
        1,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "Hello",
        None,
        &[],
    );
    stream.write_all(&hello).map_err(|_| unavailable())?;
    let hello_reply = read_reply(&mut stream, 1).map_err(|_| unavailable())?;
    if hello_reply.message_type != 2 {
        return Err(unavailable());
    }
    let Some(unique) = body_string(&hello_reply.bytes) else {
        return Err(unavailable());
    };
    let mut body = Vec::new();
    write_str(&mut body, &unique);
    let register = build_method_call(
        2,
        WATCHER,
        "/StatusNotifierWatcher",
        WATCHER,
        "RegisterStatusNotifierItem",
        Some("s"),
        &body,
    );
    stream.write_all(&register).map_err(|_| unavailable())?;
    let registered = read_reply(&mut stream, 2).map_err(|_| unavailable())?;
    if registered.message_type != 2 {
        return Err(unavailable());
    }

    let reader = stream.try_clone().map_err(|_| unavailable())?;
    tray().stop.store(false, Ordering::SeqCst);
    tray().next_serial.store(3, Ordering::SeqCst);
    {
        let mut state = lock_state();
        state.label = label.to_string();
        state.items = menu.to_vec();
        state.revision = state.revision.wrapping_add(1).max(1);
        state.shown = true;
        state.stream = Some(stream);
    }
    let handle = match thread::Builder::new()
        .name("tekto-tray".into())
        .spawn(move || serve(reader))
    {
        Ok(handle) => handle,
        Err(_) => {
            stop_icon();
            return Err(unavailable());
        }
    };
    lock_state().thread = Some(handle);
    Ok(())
}

fn stop_icon() {
    tray().stop.store(true, Ordering::SeqCst);
    let (stream, thread) = {
        let mut state = lock_state();
        state.shown = false;
        (state.stream.take(), state.thread.take())
    };
    if let Some(stream) = stream {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    if let Some(thread) = thread {
        if thread.thread().id() != thread::current().id() {
            let _ = thread.join();
        }
    }
}

fn serve(mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    while !tray().stop.load(Ordering::SeqCst) {
        let message = match read_message(&mut stream) {
            Ok(message) => message,
            Err(ReadError::Timeout) => continue,
            Err(ReadError::Dead) => break,
        };
        if message.get(1) != Some(&1) {
            continue;
        }
        let call_serial = serial_of(&message);
        // Field 7 is SENDER. The session bus routes a reply only when DESTINATION
        // is that unique name. A peer on the same socket can ignore it.
        let sender = header_string(&message, 7);
        let path = header_string(&message, 1).unwrap_or_default();
        let interface = header_string(&message, 2).unwrap_or_default();
        let member = header_string(&message, 3).unwrap_or_default();
        let no_reply = message.get(2).is_some_and(|flags| flags & 1 != 0);
        let reply = dispatch(&message, &path, &interface, &member);
        if no_reply {
            continue;
        }
        let Some(bytes) = reply else {
            continue;
        };
        let serial = tray().next_serial.fetch_add(1, Ordering::Relaxed);
        let packet = if bytes.is_empty() {
            method_return(serial, call_serial, sender.as_deref(), None, &[])
        } else {
            bytes_with_serial(serial, call_serial, sender.as_deref(), &bytes)
        };
        if stream.write_all(&packet).is_err() {
            break;
        }
    }
}

/// Encoded reply body plus signature. Empty vec means an empty method return.
/// `None` means the message was ignored.
fn dispatch(message: &[u8], path: &str, interface: &str, member: &str) -> Option<Vec<u8>> {
    let body = message_body(message);
    if member == "Introspect" && (interface.is_empty() || interface.ends_with("Introspectable")) {
        let mut payload = Vec::new();
        write_str(&mut payload, INTROSPECT);
        return Some(tagged("s", &payload));
    }
    if member == "Ping" {
        return Some(Vec::new());
    }
    if path == ITEM_PATH && member_is(interface, SNI_IFACE, member, "Activate")
        || member_is(interface, SNI_IFACE, member, "SecondaryActivate")
        || member_is(interface, SNI_IFACE, member, "ContextMenu")
        || member_is(interface, SNI_IFACE, member, "Scroll")
    {
        return Some(Vec::new());
    }
    if path == ITEM_PATH && (interface.is_empty() || interface == PROPS_IFACE) {
        if member == "Get" {
            let mut cursor = Cursor::new(body);
            let iface = cursor.string().unwrap_or_default();
            let name = cursor.string().unwrap_or_default();
            let mut payload = Vec::new();
            if !write_property_variant(&mut payload, &iface, &name) {
                return Some(error_body(
                    "org.freedesktop.DBus.Error.UnknownProperty",
                    "unknown tray property",
                ));
            }
            return Some(tagged("v", &payload));
        }
        if member == "GetAll" {
            let mut cursor = Cursor::new(body);
            let iface = cursor.string().unwrap_or_else(|| SNI_IFACE.to_string());
            let mut payload = Vec::new();
            write_all_properties(&mut payload, &iface);
            return Some(tagged("a{sv}", &payload));
        }
    }
    if path == MENU_PATH
        && (interface.is_empty() || interface == MENU_IFACE || interface == PROPS_IFACE)
    {
        if member == "GetLayout" {
            return Some(tagged("u(ia{sv}av)", &layout_body(body)));
        }
        if member == "AboutToShow" {
            let mut payload = Vec::new();
            write_bool(&mut payload, false);
            return Some(tagged("b", &payload));
        }
        if member == "Event" {
            record_event(body);
            return Some(Vec::new());
        }
        if member == "GetGroupProperties" {
            let mut payload = Vec::new();
            write_array(&mut payload, 8, |_| {});
            return Some(tagged("a(ia{sv})", &payload));
        }
        if member == "Get" || member == "GetAll" {
            let iface = if member == "Get" {
                Cursor::new(body)
                    .string()
                    .unwrap_or_else(|| MENU_IFACE.to_string())
            } else {
                Cursor::new(body)
                    .string()
                    .unwrap_or_else(|| MENU_IFACE.to_string())
            };
            if member == "Get" {
                let name = {
                    let mut cursor = Cursor::new(body);
                    let _ = cursor.string();
                    cursor.string().unwrap_or_default()
                };
                let mut payload = Vec::new();
                if !write_property_variant(&mut payload, &iface, &name) {
                    return Some(error_body(
                        "org.freedesktop.DBus.Error.UnknownProperty",
                        "unknown menu property",
                    ));
                }
                return Some(tagged("v", &payload));
            }
            let mut payload = Vec::new();
            write_all_properties(&mut payload, MENU_IFACE);
            return Some(tagged("a{sv}", &payload));
        }
    }
    Some(error_body(
        "org.freedesktop.DBus.Error.UnknownMethod",
        "unknown tray method",
    ))
}

fn member_is(interface: &str, expected: &str, member: &str, name: &str) -> bool {
    member == name && (interface.is_empty() || interface == expected)
}

fn record_event(body: &[u8]) {
    let mut cursor = Cursor::new(body);
    let Some(id) = cursor.i32() else {
        return;
    };
    let Some(event) = cursor.string() else {
        return;
    };
    if event != "clicked" || id <= 0 {
        return;
    }
    let callback = {
        let mut state = lock_state();
        let Some(item) = state.items.get((id as usize) - 1) else {
            return;
        };
        let item_id = item.id.clone();
        state.clicks.push_back(item_id.clone());
        (state.on_click, item_id)
    };
    if let (Some(callback), id) = callback {
        callback(&id);
    }
}

fn layout_body(request: &[u8]) -> Vec<u8> {
    let mut cursor = Cursor::new(request);
    let parent = cursor.i32().unwrap_or(0);
    let depth = cursor.i32().unwrap_or(-1);
    let names = cursor.string_array().unwrap_or_default();
    let (label, items, revision) = {
        let state = lock_state();
        (state.label.clone(), state.items.clone(), state.revision)
    };
    let mut body = Vec::new();
    write_u32(&mut body, revision);
    if parent <= 0 {
        write_layout_node(&mut body, 0, &label, true, &names, depth, &items);
    } else {
        let index = (parent as usize) - 1;
        let item_label = items
            .get(index)
            .map(|item| item.label.as_str())
            .unwrap_or("");
        write_layout_node(&mut body, parent, item_label, false, &names, 0, &[]);
    }
    body
}

fn write_layout_node(
    buf: &mut Vec<u8>,
    id: i32,
    label: &str,
    root: bool,
    names: &[String],
    depth: i32,
    items: &[TrayMenuItem],
) {
    pad(buf, 8);
    write_i32(buf, id);
    write_array(buf, 8, |buf| {
        if want(names, "label") {
            write_dict_string(buf, "label", label);
        }
        if root && want(names, "children-display") {
            write_dict_string(buf, "children-display", "submenu");
        }
        if !root && want(names, "enabled") {
            write_dict_bool(buf, "enabled", true);
        }
    });
    let include_children = root && depth != 0;
    write_array(buf, 8, |buf| {
        if !include_children {
            return;
        }
        for (index, item) in items.iter().enumerate() {
            pad(buf, 8);
            write_signature(buf, "(ia{sv}av)");
            write_layout_node(buf, (index as i32) + 1, &item.label, false, names, 0, &[]);
        }
    });
}

fn want(names: &[String], key: &str) -> bool {
    names.is_empty() || names.iter().any(|name| name == key)
}

fn write_property_variant(buf: &mut Vec<u8>, interface: &str, name: &str) -> bool {
    let label = lock_state().label.clone();
    if interface == MENU_IFACE {
        match name {
            "Version" => {
                write_signature(buf, "u");
                write_u32(buf, 3);
                return true;
            }
            "TextDirection" => {
                write_variant_string(buf, "ltr");
                return true;
            }
            "Status" => {
                write_variant_string(buf, "normal");
                return true;
            }
            "IconThemePath" => {
                write_signature(buf, "as");
                write_array(buf, 4, |_| {});
                return true;
            }
            _ => {}
        }
    }
    match name {
        "Category" => write_variant_string(buf, "ApplicationStatus"),
        "Id" => write_variant_string(buf, "tekto"),
        "Title" => write_variant_string(buf, &label),
        "Status" => write_variant_string(buf, "Active"),
        "WindowId" => {
            write_signature(buf, "i");
            write_i32(buf, 0);
        }
        "IconName" => write_variant_string(buf, "tekto"),
        "IconPixmap" | "ToolTip" => {
            if name == "IconPixmap" {
                write_signature(buf, "a(iiay)");
                write_icon_pixmap(buf);
            } else {
                write_signature(buf, "(sa(iiay)ss)");
                pad(buf, 8);
                write_str(buf, "tekto");
                write_icon_pixmap(buf);
                write_str(buf, &label);
                write_str(buf, &label);
            }
        }
        "ItemIsMenu" => {
            write_signature(buf, "b");
            write_bool(buf, false);
        }
        "Menu" => {
            write_signature(buf, "o");
            write_str(buf, MENU_PATH);
        }
        _ => return false,
    }
    true
}

fn write_all_properties(buf: &mut Vec<u8>, interface: &str) {
    let names: &[&str] = if interface == MENU_IFACE {
        &["Version", "TextDirection", "Status", "IconThemePath"]
    } else {
        &[
            "Category",
            "Id",
            "Title",
            "Status",
            "WindowId",
            "IconName",
            "IconPixmap",
            "ItemIsMenu",
            "Menu",
            "ToolTip",
        ]
    };
    write_array(buf, 8, |buf| {
        for name in names {
            pad(buf, 8);
            write_str(buf, name);
            let before = buf.len();
            if !write_property_variant(buf, interface, name) {
                buf.truncate(before);
            }
        }
    });
}

fn write_title_signals(stream: &mut UnixStream, serial: u32, label: &str) -> std::io::Result<()> {
    let mut changed = Vec::new();
    write_str(&mut changed, SNI_IFACE);
    write_array(&mut changed, 8, |buf| {
        pad(buf, 8);
        write_str(buf, "Title");
        write_variant_string(buf, label);
        pad(buf, 8);
        write_str(buf, "ToolTip");
        write_signature(buf, "(sa(iiay)ss)");
        pad(buf, 8);
        write_str(buf, "tekto");
        write_icon_pixmap(buf);
        write_str(buf, label);
        write_str(buf, label);
    });
    write_array(&mut changed, 4, |_| {});
    let props = build_signal(
        serial,
        ITEM_PATH,
        PROPS_IFACE,
        "PropertiesChanged",
        "sa{sv}as",
        &changed,
    );
    stream.write_all(&props)?;
    let title = build_signal(serial + 1, ITEM_PATH, SNI_IFACE, "NewTitle", "", &[]);
    stream.write_all(&title)?;
    Ok(())
}

fn write_icon_pixmap(buf: &mut Vec<u8>) {
    const SIZE: i32 = 16;
    write_array(buf, 8, |buf| {
        pad(buf, 8);
        write_i32(buf, SIZE);
        write_i32(buf, SIZE);
        write_array(buf, 1, |buf| {
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let border = x == 0 || y == 0 || x == SIZE - 1 || y == SIZE - 1;
                    if border {
                        buf.extend_from_slice(&[0xff, 0x33, 0x33, 0x33]);
                    } else {
                        buf.extend_from_slice(&[0xff, 0xf4, 0xef, 0xe4]);
                    }
                }
            }
        });
    });
}

fn unavailable() -> NativeError {
    NativeError::Unavailable {
        capability: SHOW,
        platform: current_platform(),
        reason: REASON,
    }
}

fn reject_nul(text: &str, capability: &str) -> Result<(), NativeError> {
    if text.contains('\0') {
        Err(NativeError::Message(format!(
            "{capability}: text contains a NUL byte"
        )))
    } else {
        Ok(())
    }
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
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[index + 1]), from_hex(bytes[index + 2])) {
                out.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
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

fn authenticate(stream: &mut UnixStream) -> Result<(), ()> {
    let uid = unsafe { libc::getuid() };
    let auth = format!("\0AUTH EXTERNAL {}\r\n", hex_encode(&uid.to_string()));
    stream.write_all(auth.as_bytes()).map_err(|_| ())?;
    let line = read_auth_line(stream)?;
    if !line.starts_with("OK") {
        return Err(());
    }
    stream.write_all(b"BEGIN\r\n").map_err(|_| ())?;
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

fn read_auth_line(stream: &mut UnixStream) -> Result<String, ()> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        if byte[0] == b'\n' {
            break;
        }
        if buf.len() > 512 {
            return Err(());
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
    bytes: Vec<u8>,
}

#[derive(Debug)]
enum ReadError {
    Timeout,
    Dead,
}

fn read_reply(stream: &mut UnixStream, serial: u32) -> Result<Reply, ReadError> {
    for _ in 0..8 {
        let message = read_message(stream)?;
        if reply_serial(&message) != Some(serial) {
            continue;
        }
        return Ok(Reply {
            message_type: message[1],
            bytes: message,
        });
    }
    Err(ReadError::Dead)
}

fn read_message(stream: &mut UnixStream) -> Result<Vec<u8>, ReadError> {
    let mut header = [0u8; 16];
    if let Err(err) = stream.read_exact(&mut header) {
        return Err(classify(err));
    }
    if header[0] != b'l' && header[0] != b'B' {
        return Err(ReadError::Dead);
    }
    let big = header[0] == b'B';
    let body_len = read_raw_u32(&header[4..8], big) as usize;
    let fields_len = read_raw_u32(&header[12..16], big) as usize;
    if body_len.saturating_add(fields_len) > MAX_MESSAGE {
        return Err(ReadError::Dead);
    }
    let mut message = Vec::with_capacity(16 + fields_len + body_len + 8);
    message.extend_from_slice(&header);
    let mut fields = vec![0u8; fields_len];
    stream.read_exact(&mut fields).map_err(classify)?;
    message.extend_from_slice(&fields);
    let mut pad = [0u8; 1];
    while !message.len().is_multiple_of(8) {
        stream.read_exact(&mut pad).map_err(classify)?;
        message.push(pad[0]);
    }
    let mut body = vec![0u8; body_len];
    stream.read_exact(&mut body).map_err(classify)?;
    message.extend_from_slice(&body);
    Ok(message)
}

fn classify(err: std::io::Error) -> ReadError {
    match err.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => ReadError::Timeout,
        _ => ReadError::Dead,
    }
}

fn read_raw_u32(bytes: &[u8], big: bool) -> u32 {
    let raw = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if big {
        u32::from_be_bytes(raw)
    } else {
        u32::from_le_bytes(raw)
    }
}

fn serial_of(message: &[u8]) -> u32 {
    read_raw_u32(&message[8..12], message[0] == b'B')
}

fn reply_serial(message: &[u8]) -> Option<u32> {
    header_u32(message, 5)
}

fn body_string(message: &[u8]) -> Option<String> {
    Cursor::new(message_body(message)).string()
}

fn message_body(message: &[u8]) -> &[u8] {
    if message.len() < 16 {
        return &[];
    }
    let big = message[0] == b'B';
    let body_len = read_raw_u32(&message[4..8], big) as usize;
    let fields_len = read_raw_u32(&message[12..16], big) as usize;
    let start = align_len(16 + fields_len, 8);
    message.get(start..start + body_len).unwrap_or(&[])
}

fn header_string(message: &[u8], code: u8) -> Option<String> {
    let mut found = None;
    walk_headers(message, |field, sig, value| {
        if field == code && (sig == "s" || sig == "o") {
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
    Some(read_raw_u32(&bytes, big))
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
            pos = align_len(pos, 4);
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
            pos = align_len(pos, 4);
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

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn string(&mut self) -> Option<String> {
        self.pos = align_len(self.pos, 4);
        let len = self.u32()? as usize;
        let start = self.pos;
        let end = start + len;
        let text = std::str::from_utf8(self.data.get(start..end)?).ok()?;
        if *self.data.get(end)? != 0 {
            return None;
        }
        self.pos = end + 1;
        Some(text.to_string())
    }

    fn string_array(&mut self) -> Option<Vec<String>> {
        self.pos = align_len(self.pos, 4);
        let len = self.u32()? as usize;
        self.pos = align_len(self.pos, 4);
        let end = self.pos + len;
        if end > self.data.len() {
            return None;
        }
        let mut out = Vec::new();
        while self.pos < end {
            out.push(self.string()?);
        }
        Some(out)
    }

    fn i32(&mut self) -> Option<i32> {
        self.pos = align_len(self.pos, 4);
        Some(self.u32()? as i32)
    }

    fn u32(&mut self) -> Option<u32> {
        let bytes: [u8; 4] = self.data.get(self.pos..self.pos + 4)?.try_into().ok()?;
        self.pos += 4;
        Some(u32::from_le_bytes(bytes))
    }
}

fn align_len(pos: usize, alignment: usize) -> usize {
    let rem = pos % alignment;
    if rem == 0 {
        pos
    } else {
        pos + (alignment - rem)
    }
}

/// `payload` is a D-Bus body. The vec starts with a marker the writer understands:
/// we store signature separately via [`tagged`].
fn tagged(signature: &str, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + signature.len() + 8);
    out.push(signature.len() as u8);
    out.extend_from_slice(signature.as_bytes());
    out.push(0);
    out.extend_from_slice(body);
    out
}

fn bytes_with_serial(
    serial: u32,
    reply_to: u32,
    destination: Option<&str>,
    tagged_body: &[u8],
) -> Vec<u8> {
    if tagged_body.first().is_some_and(|len| *len == 0xff) {
        let name_len = tagged_body[1] as usize;
        let name = std::str::from_utf8(&tagged_body[2..2 + name_len])
            .unwrap_or("org.freedesktop.DBus.Error.Failed");
        let text_at = 2 + name_len;
        let text = std::str::from_utf8(&tagged_body[text_at..]).unwrap_or("tray error");
        return build_error(serial, reply_to, destination, name, text);
    }
    let sig_len = tagged_body[0] as usize;
    let signature = std::str::from_utf8(&tagged_body[1..1 + sig_len]).unwrap_or("");
    let body = &tagged_body[1 + sig_len + 1..];
    method_return(serial, reply_to, destination, Some(signature), body)
}

fn error_body(name: &str, message: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0xff);
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(message.as_bytes());
    out
}

fn method_return(
    serial: u32,
    reply_to: u32,
    destination: Option<&str>,
    signature: Option<&str>,
    body: &[u8],
) -> Vec<u8> {
    let mut fields = Vec::new();
    write_field_u32(&mut fields, 5, reply_to);
    if let Some(destination) = destination.filter(|name| !name.is_empty()) {
        write_field_string(&mut fields, 6, "s", destination);
    }
    if let Some(signature) = signature.filter(|sig| !sig.is_empty()) {
        write_field_signature(&mut fields, signature);
    }
    finish_message(2, serial, &fields, body)
}

fn build_error(
    serial: u32,
    reply_to: u32,
    destination: Option<&str>,
    name: &str,
    message: &str,
) -> Vec<u8> {
    let mut fields = Vec::new();
    write_field_string(&mut fields, 4, "s", name);
    write_field_u32(&mut fields, 5, reply_to);
    if let Some(destination) = destination.filter(|name| !name.is_empty()) {
        write_field_string(&mut fields, 6, "s", destination);
    }
    write_field_signature(&mut fields, "s");
    let mut body = Vec::new();
    write_str(&mut body, message);
    finish_message(3, serial, &fields, &body)
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

fn build_signal(
    serial: u32,
    path: &str,
    interface: &str,
    member: &str,
    signature: &str,
    body: &[u8],
) -> Vec<u8> {
    let mut fields = Vec::new();
    write_field_string(&mut fields, 1, "o", path);
    write_field_string(&mut fields, 2, "s", interface);
    write_field_string(&mut fields, 3, "s", member);
    if !signature.is_empty() {
        write_field_signature(&mut fields, signature);
    }
    finish_message(4, serial, &fields, body)
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

fn write_variant_string(buf: &mut Vec<u8>, text: &str) {
    write_signature(buf, "s");
    write_str(buf, text);
}

fn write_dict_string(buf: &mut Vec<u8>, key: &str, value: &str) {
    pad(buf, 8);
    write_str(buf, key);
    write_variant_string(buf, value);
}

fn write_dict_bool(buf: &mut Vec<u8>, key: &str, value: bool) {
    pad(buf, 8);
    write_str(buf, key);
    write_signature(buf, "b");
    write_bool(buf, value);
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

fn write_bool(buf: &mut Vec<u8>, value: bool) {
    write_u32(buf, u32::from(value));
}

fn pad(buf: &mut Vec<u8>, alignment: usize) {
    while !buf.len().is_multiple_of(alignment) {
        buf.push(0);
    }
}

const INTROSPECT: &str = concat!(
    "<node>",
    "<interface name=\"org.kde.StatusNotifierItem\">",
    "<property name=\"Category\" type=\"s\" access=\"read\"/>",
    "<property name=\"Id\" type=\"s\" access=\"read\"/>",
    "<property name=\"Title\" type=\"s\" access=\"read\"/>",
    "<property name=\"Status\" type=\"s\" access=\"read\"/>",
    "<property name=\"IconPixmap\" type=\"a(iiay)\" access=\"read\"/>",
    "<property name=\"Menu\" type=\"o\" access=\"read\"/>",
    "<method name=\"Activate\"><arg name=\"x\" type=\"i\" direction=\"in\"/><arg name=\"y\" type=\"i\" direction=\"in\"/></method>",
    "</interface>",
    "<interface name=\"com.canonical.dbusmenu\">",
    "<method name=\"GetLayout\">",
    "<arg type=\"i\" direction=\"in\"/><arg type=\"i\" direction=\"in\"/><arg type=\"as\" direction=\"in\"/>",
    "<arg type=\"u\" direction=\"out\"/><arg type=\"(ia{sv}av)\" direction=\"out\"/>",
    "</method>",
    "<method name=\"Event\">",
    "<arg type=\"i\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"v\" direction=\"in\"/><arg type=\"u\" direction=\"in\"/>",
    "</method>",
    "<method name=\"AboutToShow\"><arg type=\"i\" direction=\"in\"/><arg type=\"b\" direction=\"out\"/></method>",
    "</interface>",
    "</node>"
);

#[cfg(test)]
pub(crate) fn skip_live_tray() -> bool {
    if std::env::var_os("TEKTO_TRAY_LIVE").is_some() {
        let display = std::env::var("DISPLAY").unwrap_or_default();
        return matches!(display.as_str(), ":1" | ":2" | ":1.0" | ":2.0");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::Mutex as StdMutex;

    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[test]
    fn missing_bus_is_unavailable_and_does_not_panic() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if std::env::var_os("TEKTO_TRAY_HEADLESS_CHILD").is_some() {
            let err = show_icon("Tekto", &[item("quit", "Quit")]).expect_err("no bus");
            assert!(
                matches!(
                    err,
                    NativeError::Unavailable {
                        capability: SHOW,
                        reason: REASON,
                        ..
                    }
                ),
                "{err}"
            );
            assert!(take_click().unwrap().is_none());
            hide_icon().unwrap();
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let empty =
            std::env::temp_dir().join(format!("tekto-tray-empty-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "linux_tray::tests::missing_bus_is_unavailable_and_does_not_panic",
                "--exact",
            ])
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env("XDG_RUNTIME_DIR", &empty)
            .env("TEKTO_TRAY_HEADLESS_CHILD", "1")
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&empty);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "headless tray child failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    #[test]
    fn missing_watcher_is_unavailable() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let (address, server) = spawn_bus(BusMode::NoWatcher);
        let err = show_at(&address, "Tekto", &[item("quit", "Quit")]).expect_err("no watcher");
        assert!(matches!(err, NativeError::Unavailable { .. }), "{err}");
        server.join().unwrap();
    }

    #[test]
    fn show_hide_and_menu_click_callback() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let (address, server) = spawn_bus(BusMode::Click);
        set_on_click(Some(record_click)).unwrap();
        CLICKS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
        show_at(
            &address,
            "Tekto",
            &[item("open", "Open"), item("quit", "Quit")],
        )
        .unwrap();
        set_label("Notes").unwrap();
        let seen = server.join().unwrap();
        assert!(seen.contains("RegisterStatusNotifierItem"), "{seen}");
        assert!(seen.contains("clicked"), "{seen}");
        let id = wait_click();
        assert_eq!(id, "quit");
        assert!(take_click().unwrap().is_none());
        hide_icon().unwrap();
        set_on_click(None).unwrap();
        assert_eq!(lock_state().label, "Notes");
        assert!(!lock_state().shown);
    }

    #[test]
    fn layout_names_the_menu_label() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        {
            let mut state = lock_state();
            state.label = "Tekto".into();
            state.items = vec![item("quit", "Quit")];
            state.revision = 4;
        }
        let body = layout_body(&{
            let mut request = Vec::new();
            write_i32(&mut request, 0);
            write_i32(&mut request, -1);
            write_array(&mut request, 4, |_| {});
            request
        });
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("Quit"), "{text}");
        assert!(text.contains("submenu"), "{text}");
    }

    fn item(id: &str, label: &str) -> TrayMenuItem {
        TrayMenuItem {
            id: id.into(),
            label: label.into(),
        }
    }

    static CLICKS: StdMutex<Vec<String>> = StdMutex::new(Vec::new());

    fn record_click(id: &str) {
        CLICKS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(id.to_string());
    }

    fn wait_click() -> String {
        for _ in 0..50 {
            if let Some(id) = take_click().unwrap() {
                let from_callback = CLICKS
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .first()
                    .cloned();
                assert_eq!(from_callback.as_deref(), Some(id.as_str()));
                return id;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("menu click was not delivered");
    }

    enum BusMode {
        NoWatcher,
        Click,
    }

    fn spawn_bus(mode: BusMode) -> (String, thread::JoinHandle<String>) {
        let dir = std::env::temp_dir().join(format!(
            "tekto-tray-{}-{}",
            std::process::id(),
            next_bus_id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("bus");
        let listener = UnixListener::bind(&socket).unwrap();
        let address = format!("unix:path={}", socket.display());
        let handle = thread::spawn(move || match mode {
            BusMode::NoWatcher => {
                serve_no_watcher(listener);
                let _ = std::fs::remove_dir_all(&dir);
                String::new()
            }
            BusMode::Click => {
                let seen = serve_click(listener);
                let _ = std::fs::remove_dir_all(&dir);
                seen
            }
        });
        (address, handle)
    }

    fn next_bus_id() -> u32 {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    fn serve_no_watcher(listener: UnixListener) {
        let (mut stream, _) = listener.accept().unwrap();
        expect_auth(&mut stream);
        let _ = read_message(&mut stream).unwrap();
        stream
            .write_all(&method_return(1, 1, None, Some("s"), &string_body(":1.8")))
            .unwrap();
        let register = read_message(&mut stream).unwrap();
        assert_eq!(
            header_string(&register, 3).as_deref(),
            Some("RegisterStatusNotifierItem")
        );
        stream
            .write_all(&build_error(
                2,
                2,
                None,
                "org.freedesktop.DBus.Error.ServiceUnknown",
                "no watcher",
            ))
            .unwrap();
    }

    fn serve_click(listener: UnixListener) -> String {
        let (mut stream, _) = listener.accept().unwrap();
        expect_auth(&mut stream);
        let _ = read_message(&mut stream).unwrap();
        stream
            .write_all(&method_return(1, 1, None, Some("s"), &string_body(":1.9")))
            .unwrap();
        let register = read_message(&mut stream).unwrap();
        let mut seen = String::from_utf8_lossy(&register).into_owned();
        stream
            .write_all(&method_return(2, 2, None, None, &[]))
            .unwrap();

        let call = event_call(9, 2, "clicked");
        seen.push_str(&String::from_utf8_lossy(&call));
        stream.write_all(&call).unwrap();
        let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
        for _ in 0..4 {
            let Ok(incoming) = read_message(&mut stream) else {
                break;
            };
            seen.push_str(&String::from_utf8_lossy(&incoming));
            if incoming.get(1) == Some(&2) {
                break;
            }
        }
        seen
    }

    fn event_call(serial: u32, id: i32, event: &str) -> Vec<u8> {
        let mut body = Vec::new();
        write_i32(&mut body, id);
        write_str(&mut body, event);
        write_signature(&mut body, "s");
        write_str(&mut body, "");
        write_u32(&mut body, 0);
        let mut fields = Vec::new();
        write_field_string(&mut fields, 1, "o", MENU_PATH);
        write_field_string(&mut fields, 2, "s", MENU_IFACE);
        write_field_string(&mut fields, 3, "s", "Event");
        write_field_string(&mut fields, 6, "s", ":1.9");
        write_field_signature(&mut fields, "isvu");
        finish_message(1, serial, &fields, &body)
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
    }

    fn string_body(text: &str) -> Vec<u8> {
        let mut body = Vec::new();
        write_str(&mut body, text);
        body
    }

    #[test]
    fn reply_header_names_the_caller() {
        let body = string_body("Routed");
        let ok = method_return(7, 3, Some(":1.42"), Some("s"), &body);
        assert_eq!(ok[1], 2);
        assert_eq!(header_u32(&ok, 5), Some(3));
        assert_eq!(header_string(&ok, 6).as_deref(), Some(":1.42"));
        assert!(message_body(&ok).windows(6).any(|part| part == b"Routed"));

        let err = build_error(
            8,
            4,
            Some(":1.42"),
            "org.freedesktop.DBus.Error.Failed",
            "nope",
        );
        assert_eq!(err[1], 3);
        assert_eq!(header_u32(&err, 5), Some(4));
        assert_eq!(header_string(&err, 6).as_deref(), Some(":1.42"));
        assert_eq!(
            header_string(&err, 4).as_deref(),
            Some("org.freedesktop.DBus.Error.Failed")
        );

        let direct = method_return(1, 1, None, None, &[]);
        assert!(header_string(&direct, 6).is_none());
    }

    /// Proves the session bus can deliver a property reply. Skips when
    /// `dbus-daemon` is not installed. Uses a private address, never the
    /// desktop bus and never displays :1 or :2.
    #[test]
    fn property_reply_routes_on_private_bus() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let bus = match PrivateBus::start() {
            Ok(bus) => bus,
            Err(err) if err == "missing" => {
                eprintln!("skipped property_reply_routes_on_private_bus: dbus-daemon missing");
                return;
            }
            Err(err) => panic!("private session bus: {err}"),
        };
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let host_address = bus.address.clone();
        let host = thread::spawn(move || {
            let result = host_get_title(&host_address, ready_tx);
            let _ = done_tx.send(result);
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("watcher did not claim the name");
        show_at(&bus.address, "Routed", &[item("quit", "Quit")]).unwrap();
        let title = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("host dropped")
            .expect("Get Title was not routed");
        assert_eq!(title, "Routed");
        hide_icon().unwrap();
        host.join().unwrap();
    }

    struct PrivateBus {
        address: String,
        child: std::process::Child,
        dir: std::path::PathBuf,
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl PrivateBus {
        fn start() -> Result<Self, String> {
            if std::process::Command::new("dbus-daemon")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_err()
            {
                return Err("missing".into());
            }
            let dir = std::env::temp_dir().join(format!(
                "tekto-tray-dbus-{}-{}",
                std::process::id(),
                next_bus_id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let socket = dir.join("bus");
            let config_path = dir.join("session.conf");
            let config = format!(
                concat!(
                    "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN\"\n",
                    " \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n",
                    "<busconfig>\n",
                    "  <type>session</type>\n",
                    "  <listen>unix:path={}</listen>\n",
                    "  <auth>EXTERNAL</auth>\n",
                    "  <policy context=\"default\">\n",
                    "    <allow own=\"*\"/>\n",
                    "    <allow send_destination=\"*\"/>\n",
                    "    <allow receive_sender=\"*\"/>\n",
                    "    <allow send_type=\"method_call\"/>\n",
                    "    <allow send_type=\"method_return\"/>\n",
                    "    <allow send_type=\"error\"/>\n",
                    "    <allow send_type=\"signal\"/>\n",
                    "  </policy>\n",
                    "</busconfig>\n"
                ),
                socket.display()
            );
            std::fs::write(&config_path, config).unwrap();
            let child = std::process::Command::new("dbus-daemon")
                .args(["--nofork", "--nopidfile", "--config-file"])
                .arg(&config_path)
                .spawn()
                .map_err(|err| err.to_string())?;
            let address = format!("unix:path={}", socket.display());
            let mut bus = Self {
                address,
                child,
                dir,
            };
            for _ in 0..50 {
                if UnixStream::connect(&socket).is_ok() {
                    return Ok(bus);
                }
                if bus.child.try_wait().ok().flatten().is_some() {
                    return Err("dbus-daemon exited before the socket was ready".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
            let _ = bus.child.kill();
            Err("dbus-daemon did not open the socket".into())
        }
    }

    fn host_get_title(address: &str, ready: std::sync::mpsc::Sender<()>) -> Result<String, String> {
        let mut stream = connect(address).map_err(|err| err.to_string())?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        authenticate(&mut stream).map_err(|_| "auth failed".to_string())?;
        stream
            .write_all(&build_method_call(
                1,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "Hello",
                None,
                &[],
            ))
            .map_err(|err| err.to_string())?;
        let hello = read_reply(&mut stream, 1).map_err(|_| "hello failed".to_string())?;
        if hello.message_type != 2 {
            return Err("hello was not a method return".into());
        }
        let mut name_body = Vec::new();
        write_str(&mut name_body, WATCHER);
        write_u32(&mut name_body, 0);
        stream
            .write_all(&build_method_call(
                2,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some("su"),
                &name_body,
            ))
            .map_err(|err| err.to_string())?;
        let owned = read_reply(&mut stream, 2).map_err(|_| "RequestName failed".to_string())?;
        if owned.message_type != 2 {
            return Err("watcher name was not claimed".into());
        }
        ready
            .send(())
            .map_err(|_| "ready channel closed".to_string())?;

        let register = {
            let mut found = None;
            for _ in 0..8 {
                let message =
                    read_message(&mut stream).map_err(|_| "no register call".to_string())?;
                if message.get(1) == Some(&1)
                    && header_string(&message, 3).as_deref() == Some("RegisterStatusNotifierItem")
                {
                    found = Some(message);
                    break;
                }
            }
            found.ok_or("no RegisterStatusNotifierItem")?
        };
        let caller = header_string(&register, 7).ok_or("register call has no sender")?;
        let service = body_string(&register).ok_or("register body is not a string")?;
        stream
            .write_all(&method_return(
                3,
                serial_of(&register),
                Some(&caller),
                None,
                &[],
            ))
            .map_err(|err| err.to_string())?;

        let mut get_body = Vec::new();
        write_str(&mut get_body, SNI_IFACE);
        write_str(&mut get_body, "Title");
        stream
            .write_all(&build_method_call(
                4,
                &service,
                ITEM_PATH,
                PROPS_IFACE,
                "Get",
                Some("ss"),
                &get_body,
            ))
            .map_err(|err| err.to_string())?;
        let reply =
            read_reply(&mut stream, 4).map_err(|_| "Get Title was not delivered".to_string())?;
        if reply.message_type != 2 {
            return Err(format!("Get Title type {}", reply.message_type));
        }
        let text = String::from_utf8_lossy(message_body(&reply.bytes));
        if !text.contains("Routed") {
            return Err(format!("title body {text:?}"));
        }
        Ok("Routed".into())
    }
}
