//! Linux global shortcuts via an X11 key grab (`x11rb`, MIT OR Apache-2.0).
//!
//! Compiled only with the `shortcuts` feature. A missing display, a Wayland
//! session with no X11 display, or a grab the server refuses is
//! [`NativeError::Unavailable`], not a panic. This module never opens
//! a display whose number is 1 or 2, including `host:1.1` and
//! `/tmp/.X11-unix/X1`.
//!
//! Wayland has no equivalent of `XGrabKey` for a normal client. Compositors
//! that want global shortcuts expose the freedesktop GlobalShortcuts portal
//! and ask the user to bind a chord. This feature does not open that portal.
//! A Wayland-only session returns a clear error. An XWayland `DISPLAY` is
//! tried; if the compositor blocks the grab, the error says the grab was denied.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, Keycode, ModMask, Window};
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::{current_platform, NativeError};

const MAX_ID_LEN: usize = 64;
const MAX_ACCEL_LEN: usize = 64;
const QUEUE_CAP: usize = 32;

const XK_SPACE: u32 = 0x0020;
const XK_TAB: u32 = 0xFF09;
const XK_RETURN: u32 = 0xFF0D;
const XK_ESCAPE: u32 = 0xFF1B;
const XK_LEFT: u32 = 0xFF51;
const XK_UP: u32 = 0xFF52;
const XK_RIGHT: u32 = 0xFF53;
const XK_DOWN: u32 = 0xFF54;
const XK_F1: u32 = 0xFFBE;

const MOD_SHIFT: u16 = 1 << 0;
const MOD_LOCK: u16 = 1 << 1;
const MOD_CONTROL: u16 = 1 << 2;
const MOD_ALT: u16 = 1 << 3;
const MOD_NUM: u16 = 1 << 4;
const MOD_SUPER: u16 = 1 << 6;
const BUTTON_MASK: u16 = 0xFF00;

const REASON_NO_DISPLAY: &str = "no X11 display";
const REASON_REFUSED: &str = "X11 display refused the connection";
const REASON_WAYLAND: &str =
    "Wayland compositor does not allow an X11 key grab; the global shortcuts portal is not used";
const REASON_DENIED: &str = "X11 key grab was denied";
const REASON_RESERVED: &str = "refusing reserved display :1 or :2";

struct Grab {
    keycode: Keycode,
    mods: u16,
}

enum Cmd {
    Register {
        accelerator: String,
        id: String,
        reply: Sender<Result<(), NativeError>>,
    },
    Unregister {
        id: String,
        reply: Sender<Result<(), NativeError>>,
    },
}

struct Runtime {
    queue: VecDeque<String>,
    hook: Option<fn(&str)>,
    tx: Option<Sender<Cmd>>,
}

fn runtime() -> std::sync::MutexGuard<'static, Runtime> {
    static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
        queue: VecDeque::new(),
        hook: None,
        tx: None,
    });
    RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unavailable(capability: &'static str, reason: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability,
        platform: current_platform(),
        reason,
    }
}

fn message(text: impl Into<String>) -> NativeError {
    NativeError::Message(text.into())
}

pub(crate) fn register(accelerator: &str, id: &str) -> Result<(), NativeError> {
    check_id(id)?;
    // Parse before any display connection so a bad chord is a Message.
    let _ = parse_accelerator(accelerator)?;
    if let Some(reason) = session_block() {
        return Err(unavailable("shortcuts.register", reason));
    }
    let (reply_tx, reply_rx) = mpsc::channel();
    dispatch(Cmd::Register {
        accelerator: accelerator.to_string(),
        id: id.to_string(),
        reply: reply_tx,
    })?;
    reply_rx
        .recv()
        .unwrap_or_else(|_| Err(unavailable("shortcuts.register", REASON_NO_DISPLAY)))
}

pub(crate) fn unregister(id: &str) -> Result<(), NativeError> {
    check_id(id)?;
    let (reply_tx, reply_rx) = mpsc::channel();
    // Nothing registered yet: no display is required.
    if runtime().tx.is_none() {
        return Ok(());
    }
    dispatch(Cmd::Unregister {
        id: id.to_string(),
        reply: reply_tx,
    })?;
    reply_rx
        .recv()
        .unwrap_or_else(|_| Err(unavailable("shortcuts.unregister", REASON_NO_DISPLAY)))
}

pub(crate) fn take_activation() -> Result<Option<String>, NativeError> {
    Ok(runtime().queue.pop_front())
}

pub(crate) fn set_on_activation(on_activation: Option<fn(&str)>) -> Result<(), NativeError> {
    runtime().hook = on_activation;
    Ok(())
}

/// True when `name` is an X11 display string or Unix socket for display 1 or 2.
///
/// Accepts `host:display.screen` (screen optional) and socket paths such as
/// `/tmp/.X11-unix/X1` or `unix:/tmp/.X11-unix/X2`. This does not connect.
pub(crate) fn display_is_reserved(name: &str) -> bool {
    matches!(x11_display_number(name), Some(1 | 2))
}

/// Display index from an X11 name, or `None` when the string is not a display.
///
/// Socket paths are classified from the `X<number>` file name. `x11rb` reports
/// those paths as display 0 after a successful `stat`, which would hide `:1`
/// and `:2`.
fn x11_display_number(name: &str) -> Option<u16> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Some(number) = socket_display_number(name) {
        return Some(number);
    }
    let remaining = match name.rfind('/') {
        Some(pos) => &name[pos + 1..],
        None => name,
    };
    let pos = remaining.rfind(':')?;
    let rest = &remaining[pos + 1..];
    let display = match rest.find('.') {
        Some(dot) => &rest[..dot],
        None => rest,
    };
    if display.is_empty() || !display.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if let Some(dot) = rest.find('.') {
        let screen = &rest[dot + 1..];
        if screen.is_empty() || !screen.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }
    display.parse().ok()
}

/// Display index encoded in an X11 Unix socket path, such as `X1` or `X2.0`.
fn socket_display_number(name: &str) -> Option<u16> {
    let path = name.strip_prefix("unix:").unwrap_or(name);
    let looks_like_path =
        path.starts_with('/') || path.starts_with('@') || name.starts_with("unix:");
    if !looks_like_path {
        return None;
    }
    let path = path.trim_end_matches('/');
    let base = path.rsplit('/').next().unwrap_or(path);
    let base = base.strip_prefix('@').unwrap_or(base);
    let rest = base.strip_prefix('X')?;
    let (number, screen) = match rest.split_once('.') {
        Some((number, screen)) => (number, Some(screen)),
        None => (rest, None),
    };
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if let Some(screen) = screen {
        if screen.is_empty() || !screen.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }
    number.parse().ok()
}

fn check_id(id: &str) -> Result<(), NativeError> {
    if id.is_empty() || id.len() > MAX_ID_LEN || id.contains('\0') {
        return Err(message(
            "shortcuts: id must be 1 to 64 bytes and must not contain NUL",
        ));
    }
    Ok(())
}

/// `None` when an X11 display may be opened. Display numbers 1 and 2 are
/// refused here, in every form `x11rb` would open, so this process never
/// connects to them.
fn session_block() -> Option<&'static str> {
    let display = std::env::var("DISPLAY").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if display_is_reserved(&display) {
        return Some(REASON_RESERVED);
    }
    if display.is_empty() && wayland.is_empty() {
        return Some(REASON_NO_DISPLAY);
    }
    if display.is_empty() {
        return Some(REASON_WAYLAND);
    }
    None
}

struct Parsed {
    keysym: u32,
    mods: u16,
}

fn parse_accelerator(accelerator: &str) -> Result<Parsed, NativeError> {
    if accelerator.is_empty() || accelerator.len() > MAX_ACCEL_LEN || accelerator.contains('\0') {
        return Err(message(
            "shortcuts.register: accelerator must be 1 to 64 bytes, like Ctrl+K",
        ));
    }
    let parts: Vec<&str> = accelerator.split('+').map(str::trim).collect();
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(message(
            "shortcuts.register: accelerator must look like Ctrl+K or Ctrl+Shift+F1",
        ));
    }
    let key = parts[parts.len() - 1];
    let mut mods = 0u16;
    for part in &parts[..parts.len() - 1] {
        let bit = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "shift" => MOD_SHIFT,
            "alt" | "option" | "mod1" => MOD_ALT,
            "super" | "meta" | "win" | "mod4" => MOD_SUPER,
            _ => {
                return Err(message(format!(
                    "shortcuts.register: unknown modifier '{part}'"
                )));
            }
        };
        if mods & bit != 0 {
            return Err(message(format!(
                "shortcuts.register: modifier '{part}' is repeated"
            )));
        }
        mods |= bit;
    }
    Ok(Parsed {
        keysym: keysym_of(key)?,
        mods,
    })
}

fn keysym_of(key: &str) -> Result<u32, NativeError> {
    let named = match key.to_ascii_lowercase().as_str() {
        "space" => Some(XK_SPACE),
        "tab" => Some(XK_TAB),
        "enter" | "return" => Some(XK_RETURN),
        "esc" | "escape" => Some(XK_ESCAPE),
        "left" => Some(XK_LEFT),
        "right" => Some(XK_RIGHT),
        "up" => Some(XK_UP),
        "down" => Some(XK_DOWN),
        _ => None,
    };
    if let Some(keysym) = named {
        return Ok(keysym);
    }
    let bytes = key.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'f' || bytes[0] == b'F') {
        if let Ok(n) = key[1..].parse::<u32>() {
            if (1..=24).contains(&n) {
                return Ok(XK_F1 + n - 1);
            }
        }
    }
    let mut chars = key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphanumeric() {
            return Ok(u32::from(ch.to_ascii_lowercase()));
        }
    }
    Err(message(format!("shortcuts.register: unknown key '{key}'")))
}

fn ensure_tx(capability: &'static str) -> Result<Sender<Cmd>, NativeError> {
    let mut rt = runtime();
    if let Some(tx) = &rt.tx {
        return Ok(tx.clone());
    }
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("tekto-shortcuts".into())
        .spawn(move || worker(rx))
        .map_err(|_| unavailable(capability, REASON_NO_DISPLAY))?;
    rt.tx = Some(tx.clone());
    Ok(tx)
}

fn dispatch(cmd: Cmd) -> Result<(), NativeError> {
    let capability = match &cmd {
        Cmd::Register { .. } => "shortcuts.register",
        Cmd::Unregister { .. } => "shortcuts.unregister",
    };
    let tx = ensure_tx(capability)?;
    if tx.send(cmd).is_err() {
        runtime().tx = None;
        return Err(unavailable(capability, REASON_NO_DISPLAY));
    }
    Ok(())
}

fn worker(rx: Receiver<Cmd>) {
    let mut conn: Option<(RustConnection, Window)> = None;
    let mut grabs: HashMap<String, Grab> = HashMap::new();
    loop {
        let cmd = if conn.is_some() {
            match rx.try_recv() {
                Ok(cmd) => Some(cmd),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => return,
            }
        };
        if let Some(cmd) = cmd {
            if conn.is_none() {
                if matches!(cmd, Cmd::Unregister { .. }) {
                    if let Cmd::Unregister { reply, .. } = cmd {
                        let _ = reply.send(Ok(()));
                    }
                    continue;
                }
                match connect_display() {
                    Ok(pair) => conn = Some(pair),
                    Err(err) => {
                        if let Cmd::Register { reply, .. } = cmd {
                            let _ = reply.send(Err(err));
                        }
                        continue;
                    }
                }
            }
            let Some((connection, root)) = conn.as_ref() else {
                continue;
            };
            match cmd {
                Cmd::Register {
                    accelerator,
                    id,
                    reply,
                } => {
                    let result = install(connection, *root, &mut grabs, &accelerator, &id);
                    let _ = reply.send(result);
                }
                Cmd::Unregister { id, reply } => {
                    if let Some(grab) = grabs.remove(&id) {
                        ungrab(connection, *root, &grab);
                    }
                    let _ = reply.send(Ok(()));
                }
            }
        }
        if let Some((connection, _)) = conn.as_ref() {
            match connection.poll_for_event() {
                Ok(Some(event)) => on_event(&grabs, event),
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    conn = None;
                    grabs.clear();
                }
            }
        }
    }
}

fn connect_display() -> Result<(RustConnection, Window), NativeError> {
    if let Some(reason) = session_block() {
        return Err(unavailable("shortcuts.register", reason));
    }
    let (conn, screen) = RustConnection::connect(None).map_err(|_| {
        let reason = if std::env::var("DISPLAY").unwrap_or_default().is_empty() {
            REASON_NO_DISPLAY
        } else {
            REASON_REFUSED
        };
        unavailable("shortcuts.register", reason)
    })?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .map(|screen| screen.root)
        .ok_or_else(|| unavailable("shortcuts.register", REASON_NO_DISPLAY))?;
    Ok((conn, root))
}

fn install(
    conn: &RustConnection,
    root: Window,
    grabs: &mut HashMap<String, Grab>,
    accelerator: &str,
    id: &str,
) -> Result<(), NativeError> {
    let parsed = parse_accelerator(accelerator)?;
    let keycode = keycode_for_keysym(conn, parsed.keysym)?;
    // The same client can replace its own passive grab. A second id on this
    // chord would drop the first id's grab when either id is released.
    if duplicate_chord(grabs, id, keycode, parsed.mods).is_some() {
        return Err(message("shortcuts.register: chord is already registered"));
    }
    if let Some(previous) = grabs.remove(id) {
        ungrab(conn, root, &previous);
    }
    let grab = Grab {
        keycode,
        mods: parsed.mods,
    };
    grab_variants(conn, root, &grab)?;
    grabs.insert(id.to_string(), grab);
    Ok(())
}

/// Another id that already owns this keycode and modifier pair.
fn duplicate_chord<'a>(
    grabs: &'a HashMap<String, Grab>,
    id: &str,
    keycode: Keycode,
    mods: u16,
) -> Option<&'a str> {
    grabs.iter().find_map(|(other, grab)| {
        if other != id && grab.keycode == keycode && grab.mods == mods {
            Some(other.as_str())
        } else {
            None
        }
    })
}

fn grab_variants(conn: &RustConnection, root: Window, grab: &Grab) -> Result<(), NativeError> {
    let extras = [0, MOD_LOCK, MOD_NUM, MOD_LOCK | MOD_NUM];
    let mut held = Vec::with_capacity(extras.len());
    for extra in extras {
        let mods = ModMask::from(grab.mods | extra);
        let cookie = conn
            .grab_key(
                false,
                root,
                mods,
                grab.keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(|_| unavailable("shortcuts.register", REASON_NO_DISPLAY))?;
        match cookie.check() {
            Ok(()) => held.push(mods),
            Err(err) => {
                for mods in &held {
                    let _ = conn.ungrab_key(grab.keycode, root, *mods);
                }
                let _ = conn.flush();
                return Err(map_grab_error(err));
            }
        }
    }
    conn.flush()
        .map_err(|_| unavailable("shortcuts.register", REASON_NO_DISPLAY))?;
    Ok(())
}

fn ungrab(conn: &RustConnection, root: Window, grab: &Grab) {
    let extras = [0, MOD_LOCK, MOD_NUM, MOD_LOCK | MOD_NUM];
    for extra in extras {
        let _ = conn.ungrab_key(grab.keycode, root, ModMask::from(grab.mods | extra));
    }
    let _ = conn.flush();
}

fn map_grab_error(err: ReplyError) -> NativeError {
    match err {
        ReplyError::X11Error(error) if error.error_kind == ErrorKind::Access => {
            unavailable("shortcuts.register", REASON_DENIED)
        }
        ReplyError::ConnectionError(_) => unavailable("shortcuts.register", REASON_NO_DISPLAY),
        other => message(format!("shortcuts.register: {other}")),
    }
}

fn keycode_for_keysym(conn: &RustConnection, keysym: u32) -> Result<Keycode, NativeError> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max.saturating_sub(min).saturating_add(1);
    let reply = conn
        .get_keyboard_mapping(min, count)
        .map_err(|_| message("shortcuts.register: cannot read the keymap"))?
        .reply()
        .map_err(|_| message("shortcuts.register: cannot read the keymap"))?;
    let per = reply.keysyms_per_keycode as usize;
    if per == 0 {
        return Err(message("shortcuts.register: unknown key"));
    }
    for (index, chunk) in reply.keysyms.chunks(per).enumerate() {
        if chunk.contains(&keysym) {
            return Ok(min + index as Keycode);
        }
    }
    Err(message("shortcuts.register: unknown key"))
}

fn on_event(grabs: &HashMap<String, Grab>, event: Event) {
    let Event::KeyPress(press) = event else {
        return;
    };
    let ignore = MOD_LOCK | MOD_NUM | BUTTON_MASK;
    let state = u16::from(press.state) & !ignore;
    for (id, grab) in grabs {
        if press.detail == grab.keycode && state == grab.mods {
            note_activation(id);
        }
    }
}

fn note_activation(id: &str) {
    let hook = {
        let mut rt = runtime();
        if rt.queue.len() >= QUEUE_CAP {
            rt.queue.pop_front();
        }
        rt.queue.push_back(id.to_string());
        rt.hook
    };
    if let Some(hook) = hook {
        hook(id);
    }
}

/// True when a live grab must not run. `TEKTO_SHORTCUTS_LIVE=1` opts in,
/// except when `DISPLAY` refers to display number 1 or 2.
#[cfg(test)]
pub(crate) fn skip_live_shortcuts() -> bool {
    if std::env::var_os("TEKTO_SHORTCUTS_LIVE").is_some() {
        return session_block() == Some(REASON_RESERVED);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ctrl_shift_letter() {
        let parsed = parse_accelerator("Ctrl+Shift+K").unwrap();
        assert_eq!(parsed.keysym, u32::from(b'k'));
        assert_eq!(parsed.mods, MOD_CONTROL | MOD_SHIFT);
    }

    #[test]
    fn rejects_a_bare_key_and_unknown_modifier() {
        assert!(parse_accelerator("K").is_err());
        assert!(parse_accelerator("Foo+K").is_err());
        assert!(parse_accelerator("Ctrl+").is_err());
    }

    #[test]
    fn refuses_reserved_display_names_without_connecting() {
        for name in [
            ":1",
            ":2",
            ":1.0",
            ":2.0",
            ":1.1",
            ":2.5",
            "localhost:1",
            "localhost:2",
            "localhost:1.1",
            "127.0.0.1:2.0",
            "unix/localhost:1",
            "/tmp/.X11-unix/X1",
            "/tmp/.X11-unix/X2",
            "/tmp/.X11-unix/X1.0",
            "unix:/tmp/.X11-unix/X1",
            "unix:/tmp/.X11-unix/X2",
            "@/tmp/.X11-unix/X1",
        ] {
            assert!(display_is_reserved(name), "{name}");
        }
        for name in [
            "",
            ":0",
            ":0.0",
            ":10",
            ":11",
            ":12",
            ":21",
            "localhost:0",
            "localhost:10",
            "localhost:11.1",
            "/tmp/.X11-unix/X0",
            "/tmp/.X11-unix/X10",
            "/tmp/.X11-unix/X11",
            "unix:/tmp/.X11-unix/X0",
            "unix:/tmp/.X11-unix/X12",
        ] {
            assert!(!display_is_reserved(name), "{name}");
        }
    }

    #[test]
    fn rejects_a_duplicate_chord_for_another_id() {
        let mut grabs = HashMap::new();
        grabs.insert(
            "search".into(),
            Grab {
                keycode: 45,
                mods: MOD_CONTROL,
            },
        );
        assert_eq!(
            duplicate_chord(&grabs, "other", 45, MOD_CONTROL),
            Some("search")
        );
        assert_eq!(duplicate_chord(&grabs, "search", 45, MOD_CONTROL), None);
        assert_eq!(
            duplicate_chord(&grabs, "other", 45, MOD_CONTROL | MOD_SHIFT),
            None
        );
        assert_eq!(duplicate_chord(&grabs, "other", 46, MOD_CONTROL), None);
    }
}
