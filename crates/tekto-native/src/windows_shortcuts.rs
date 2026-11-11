//! Windows global shortcuts via `RegisterHotKey` / `UnregisterHotKey`.
//!
//! Compiled only with the `shortcuts` feature. No extra crate. A dedicated
//! thread pumps `WM_HOTKEY`, so a chord still fires when the app window is
//! in the background. A denied or reserved chord is
//! [`NativeError::Unavailable`], not a panic. This module does not open a
//! display.
//!
//! There is no Windows runtime test here. `cargo check --target
//! x86_64-pc-windows-gnu --features shortcuts` only type-checks the FFI.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::{current_platform, NativeError};

const MAX_ID_LEN: usize = 64;
const MAX_ACCEL_LEN: usize = 64;
const QUEUE_CAP: usize = 32;
const WAIT: Duration = Duration::from_secs(2);

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
const MOD_NOREPEAT: u32 = 0x4000;

const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_F1: u32 = 0x70;

const WM_HOTKEY: u32 = 0x0312;
const WM_APP: u32 = 0x8000;
const WM_SHORTCUT_CMD: u32 = WM_APP + 20;

const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

const REASON_THREAD: &str = "Windows hotkey thread is unavailable";
const REASON_TAKEN: &str = "Windows hotkey is already registered";
const REASON_DENIED: &str = "Windows hotkey registration was denied";

type Handle = *mut c_void;

#[repr(C)]
struct Msg {
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt_x: i32,
    pt_y: i32,
    private: u32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterHotKey(window: Handle, id: i32, modifiers: u32, vk: u32) -> i32;
    fn UnregisterHotKey(window: Handle, id: i32) -> i32;
    fn PeekMessageW(msg: *mut Msg, window: Handle, min: u32, max: u32, remove: u32) -> i32;
    fn GetMessageW(msg: *mut Msg, window: Handle, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostThreadMessageW(thread: u32, message: u32, wparam: usize, lparam: isize) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn GetLastError() -> u32;
}

struct Hotkey {
    atom: i32,
    vk: u32,
    mods: u32,
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
    thread_id: u32,
}

fn runtime() -> std::sync::MutexGuard<'static, Runtime> {
    static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
        queue: VecDeque::new(),
        hook: None,
        tx: None,
        thread_id: 0,
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
    let _ = parse_accelerator(accelerator)?;
    let (reply_tx, reply_rx) = mpsc::channel();
    dispatch(Cmd::Register {
        accelerator: accelerator.to_string(),
        id: id.to_string(),
        reply: reply_tx,
    })?;
    reply_rx
        .recv_timeout(WAIT)
        .unwrap_or_else(|_| Err(unavailable("shortcuts.register", REASON_THREAD)))
}

pub(crate) fn unregister(id: &str) -> Result<(), NativeError> {
    check_id(id)?;
    if runtime().tx.is_none() {
        return Ok(());
    }
    let (reply_tx, reply_rx) = mpsc::channel();
    dispatch(Cmd::Unregister {
        id: id.to_string(),
        reply: reply_tx,
    })?;
    reply_rx
        .recv_timeout(WAIT)
        .unwrap_or_else(|_| Err(unavailable("shortcuts.unregister", REASON_THREAD)))
}

pub(crate) fn take_activation() -> Result<Option<String>, NativeError> {
    Ok(runtime().queue.pop_front())
}

pub(crate) fn set_on_activation(on_activation: Option<fn(&str)>) -> Result<(), NativeError> {
    runtime().hook = on_activation;
    Ok(())
}

fn check_id(id: &str) -> Result<(), NativeError> {
    if id.is_empty() || id.len() > MAX_ID_LEN || id.contains('\0') {
        return Err(message(
            "shortcuts: id must be 1 to 64 bytes and must not contain NUL",
        ));
    }
    Ok(())
}

struct Parsed {
    vk: u32,
    mods: u32,
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
    let mut mods = 0u32;
    for part in &parts[..parts.len() - 1] {
        let bit = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "shift" => MOD_SHIFT,
            "alt" | "option" | "mod1" => MOD_ALT,
            "super" | "meta" | "win" | "mod4" => MOD_WIN,
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
        vk: vk_of(key)?,
        mods: mods | MOD_NOREPEAT,
    })
}

fn vk_of(key: &str) -> Result<u32, NativeError> {
    let named = match key.to_ascii_lowercase().as_str() {
        "space" => Some(VK_SPACE),
        "tab" => Some(VK_TAB),
        "enter" | "return" => Some(VK_RETURN),
        "esc" | "escape" => Some(VK_ESCAPE),
        "left" => Some(VK_LEFT),
        "right" => Some(VK_RIGHT),
        "up" => Some(VK_UP),
        "down" => Some(VK_DOWN),
        _ => None,
    };
    if let Some(vk) = named {
        return Ok(vk);
    }
    let bytes = key.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'f' || bytes[0] == b'F') {
        if let Ok(n) = key[1..].parse::<u32>() {
            if (1..=24).contains(&n) {
                return Ok(VK_F1 + n - 1);
            }
        }
    }
    let mut chars = key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphabetic() {
            return Ok(u32::from(ch.to_ascii_uppercase()));
        }
        if ch.is_ascii_digit() {
            return Ok(u32::from(ch));
        }
    }
    Err(message(format!("shortcuts.register: unknown key '{key}'")))
}

fn ensure_worker(capability: &'static str) -> Result<(Sender<Cmd>, u32), NativeError> {
    let mut rt = runtime();
    if let Some(tx) = &rt.tx {
        if rt.thread_id != 0 {
            return Ok((tx.clone(), rt.thread_id));
        }
    }
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::Builder::new()
        .name("tekto-shortcuts".into())
        .spawn(move || worker(rx, ready_tx))
        .map_err(|_| unavailable(capability, REASON_THREAD))?;
    let thread_id = ready_rx
        .recv_timeout(WAIT)
        .map_err(|_| unavailable(capability, REASON_THREAD))?;
    rt.tx = Some(tx.clone());
    rt.thread_id = thread_id;
    Ok((tx, thread_id))
}

fn dispatch(cmd: Cmd) -> Result<(), NativeError> {
    let capability = match &cmd {
        Cmd::Register { .. } => "shortcuts.register",
        Cmd::Unregister { .. } => "shortcuts.unregister",
    };
    let (tx, thread_id) = ensure_worker(capability)?;
    if tx.send(cmd).is_err() {
        let mut rt = runtime();
        rt.tx = None;
        rt.thread_id = 0;
        return Err(unavailable(capability, REASON_THREAD));
    }
    let posted = unsafe { PostThreadMessageW(thread_id, WM_SHORTCUT_CMD, 0, 0) };
    if posted == 0 {
        let mut rt = runtime();
        rt.tx = None;
        rt.thread_id = 0;
        return Err(unavailable(capability, REASON_THREAD));
    }
    Ok(())
}

fn worker(rx: Receiver<Cmd>, ready: Sender<u32>) {
    let mut scratch = Msg {
        window: ptr::null_mut(),
        message: 0,
        wparam: 0,
        lparam: 0,
        time: 0,
        pt_x: 0,
        pt_y: 0,
        private: 0,
    };
    // Creates this thread's queue before the parent posts to it.
    unsafe {
        PeekMessageW(&mut scratch, ptr::null_mut(), 0, 0, 0);
    }
    let thread_id = unsafe { GetCurrentThreadId() };
    if ready.send(thread_id).is_err() {
        return;
    }
    let mut hotkeys: HashMap<String, Hotkey> = HashMap::new();
    let mut next_atom = 1i32;
    loop {
        let status = unsafe { GetMessageW(&mut scratch, ptr::null_mut(), 0, 0) };
        if status == 0 || status == -1 {
            break;
        }
        if scratch.message == WM_HOTKEY {
            let atom = scratch.wparam as i32;
            if let Some(id) = hotkeys
                .iter()
                .find(|(_, hotkey)| hotkey.atom == atom)
                .map(|(id, _)| id.clone())
            {
                note_activation(&id);
            }
        } else if scratch.message == WM_SHORTCUT_CMD && drain(&rx, &mut hotkeys, &mut next_atom) {
            break;
        }
        unsafe {
            TranslateMessage(&scratch);
            DispatchMessageW(&scratch);
        }
    }
    for hotkey in hotkeys.values() {
        unsafe {
            UnregisterHotKey(ptr::null_mut(), hotkey.atom);
        }
    }
}

fn drain(rx: &Receiver<Cmd>, hotkeys: &mut HashMap<String, Hotkey>, next_atom: &mut i32) -> bool {
    loop {
        match rx.try_recv() {
            Ok(Cmd::Register {
                accelerator,
                id,
                reply,
            }) => {
                let result = install(hotkeys, next_atom, &accelerator, &id);
                let _ = reply.send(result);
            }
            Ok(Cmd::Unregister { id, reply }) => {
                if let Some(hotkey) = hotkeys.remove(&id) {
                    unsafe {
                        UnregisterHotKey(ptr::null_mut(), hotkey.atom);
                    }
                }
                let _ = reply.send(Ok(()));
            }
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => return true,
        }
    }
}

fn install(
    hotkeys: &mut HashMap<String, Hotkey>,
    next_atom: &mut i32,
    accelerator: &str,
    id: &str,
) -> Result<(), NativeError> {
    let parsed = parse_accelerator(accelerator)?;
    if hotkeys
        .iter()
        .any(|(other, hotkey)| other != id && hotkey.vk == parsed.vk && hotkey.mods == parsed.mods)
    {
        return Err(message("shortcuts.register: chord is already registered"));
    }
    if let Some(previous) = hotkeys.get(id) {
        if previous.vk == parsed.vk && previous.mods == parsed.mods {
            return Ok(());
        }
    }
    let atom = match fresh_atom(hotkeys, next_atom) {
        Some(atom) => atom,
        None => return Err(unavailable("shortcuts.register", REASON_DENIED)),
    };
    let ok = unsafe { RegisterHotKey(ptr::null_mut(), atom, parsed.mods, parsed.vk) };
    if ok == 0 {
        let reason = if unsafe { GetLastError() } == ERROR_HOTKEY_ALREADY_REGISTERED {
            REASON_TAKEN
        } else {
            REASON_DENIED
        };
        return Err(unavailable("shortcuts.register", reason));
    }
    if let Some(previous) = hotkeys.remove(id) {
        unsafe {
            UnregisterHotKey(ptr::null_mut(), previous.atom);
        }
    }
    hotkeys.insert(
        id.to_string(),
        Hotkey {
            atom,
            vk: parsed.vk,
            mods: parsed.mods,
        },
    );
    Ok(())
}

fn fresh_atom(hotkeys: &HashMap<String, Hotkey>, next_atom: &mut i32) -> Option<i32> {
    for _ in 0..0xBFFF {
        let atom = *next_atom;
        *next_atom = if *next_atom >= 0xBFFF {
            1
        } else {
            *next_atom + 1
        };
        if !hotkeys.values().any(|hotkey| hotkey.atom == atom) {
            return Some(atom);
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_accelerator_does_not_touch_the_os() {
        let err = register("Nope", "search").expect_err("bad chord");
        assert!(matches!(err, NativeError::Message(_)), "{err}");
        assert!(runtime().tx.is_none());
    }
}
