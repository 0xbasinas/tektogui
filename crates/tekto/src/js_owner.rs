//! Retained desktop QuickJS session and its serialized lifecycle.
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle, ThreadId};
use tekto_js::{apply_ops, Host, HostOp, JsError, JsSession};
use tekto_tree::{NodeId, Tree};

use crate::{layout_tree, SharedTreeHandle, Viewport};

type Reply<T> = mpsc::Sender<Result<T, String>>;
enum Command {
    Mount(String, Reply<u64>),
    Click(u64, u64, bool, Reply<bool>),
    Input(u64, u64, String, Reply<bool>),
    Forget(u64, Reply<()>),
    Generation(Reply<u64>),
    Shutdown(Reply<()>),
}
struct Inner {
    tx: mpsc::Sender<Command>,
    thread: ThreadId,
    join: Mutex<Option<JoinHandle<()>>>,
    gate: Arc<Mutex<()>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if thread::current().id() == self.thread {
            return;
        }
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Command::Shutdown(tx)).is_ok() {
            let _ = rx.recv();
        }
        if let Ok(join) = self.join.get_mut() {
            if let Some(join) = join.take() {
                let _ = join.join();
            }
        }
    }
}
/// A cloneable handle to a single QuickJS owner thread. Calls must be made
/// without holding the tree or server dispatch locks.
#[derive(Clone)]
pub struct JsAppOwner(Arc<Inner>);

struct SharedHost(Arc<Mutex<Tree>>);
impl Host for SharedHost {
    fn apply(&mut self, ops: &[HostOp]) -> Result<Vec<u64>, JsError> {
        let mut tree = self.0.lock().map_err(|e| JsError::Message(e.to_string()))?;
        apply_ops(&mut tree, ops)
    }
}
struct Session {
    js: JsSession,
    host: SharedHost,
}
fn dispatch(
    current: &mut Option<Session>,
    generation: u64,
    expected: u64,
    id: u64,
    value: Option<&str>,
    allow_removed: bool,
    tree: &Arc<Mutex<Tree>>,
) -> Result<bool, String> {
    match current.as_mut() {
        Some(session) if expected == generation => {
            let live = tree.lock().map_err(|e| e.to_string())?;
            if !allow_removed && live.get(NodeId::from_raw(id)).is_none() {
                return Ok(false);
            }
            drop(live);
            match value {
                Some(value) => session.js.dispatch_input(id, value, &mut session.host),
                None => session.js.dispatch_click(id, &mut session.host),
            }
            .map_err(|e| e.to_string())
        }
        _ => Err("stale JS generation".into()),
    }
}
fn finish_event(
    result: Result<bool, String>,
    kind: &str,
    reply: Reply<bool>,
    logs: &Arc<Mutex<Vec<String>>>,
    viewport: &Arc<Mutex<Viewport>>,
    tree: &Arc<Mutex<Tree>>,
    notify: &std::sync::Weak<SharedTreeHandle>,
) {
    if let Err(error) = &result {
        if let Ok(mut logs) = logs.lock() {
            logs.push(format!("js {kind} failed: {error}"));
        }
    }
    if matches!(result, Ok(true) | Err(_)) {
        let (w, h) = viewport
            .lock()
            .map(|v| (v.width, v.height))
            .unwrap_or((640, 400));
        if let Ok(mut tree) = tree.lock() {
            layout_tree(&mut tree, w as f32, h as f32);
        }
        if let Some(handle) = notify.upgrade() {
            if let Ok(callback) = handle.on_change.lock() {
                if let Some(callback) = callback.as_ref() {
                    callback();
                }
            }
        }
    }
    let _ = reply.send(result);
}
impl JsAppOwner {
    pub fn new(handle: &Arc<SharedTreeHandle>, viewport: &Arc<Mutex<Viewport>>) -> Self {
        let tree = Arc::clone(&handle.tree);
        let viewport = Arc::clone(viewport);
        let logs = Arc::clone(&handle.logs);
        let notify = Arc::downgrade(handle);
        let (tx, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let mut current: Option<Session> = None;
            let mut generation = 0u64;
            while let Ok(command) = rx.recv() {
                match command {
                    Command::Mount(source, reply) => {
                        let result = (|| {
                            let next_generation =
                                generation.checked_add(1).ok_or("JS generation exhausted")?;
                            let snapshot = match current.as_mut() {
                                Some(previous) => previous
                                    .js
                                    .snapshot_signals(&mut previous.host)
                                    .map_err(|e| e.to_string())?,
                                None => "{}".to_string(),
                            };
                            let mut candidate = Session {
                                js: JsSession::new().map_err(|e| e.to_string())?,
                                host: SharedHost(Arc::new(Mutex::new(Tree::new()))),
                            };
                            candidate
                                .js
                                .restore_signals(&snapshot, &mut candidate.host)
                                .map_err(|e| e.to_string())?;
                            if let Err(e) = candidate.js.eval(&source, &mut candidate.host) {
                                let _ = candidate.js.dispose(&mut candidate.host);
                                return Err(e.to_string());
                            }
                            let signal_report = candidate
                                .js
                                .signal_report(&mut candidate.host)
                                .map_err(|e| e.to_string())?;
                            if current.is_some() {
                                if let Ok(mut logs) = logs.lock() {
                                    logs.push(format!("js signal reload: {signal_report}"));
                                }
                            }
                            let (w, h) = viewport
                                .lock()
                                .map(|v| (v.width, v.height))
                                .unwrap_or((640, 400));
                            let mut next =
                                candidate.host.0.lock().map_err(|e| e.to_string())?.clone();
                            layout_tree(&mut next, w as f32, h as f32);
                            let mut live = tree.lock().map_err(|e| e.to_string())?;
                            next.inherit_event_handler(&live);
                            let old = std::mem::replace(&mut *live, next);
                            drop(live);
                            if let Some(mut previous) = current.take() {
                                previous.host = SharedHost(Arc::new(Mutex::new(old)));
                                if let Err(e) = previous.js.dispose(&mut previous.host) {
                                    if let Ok(mut logs) = logs.lock() {
                                        logs.push(format!("js cleanup failed: {e}"));
                                    }
                                }
                            }
                            candidate.host = SharedHost(Arc::clone(&tree));
                            current = Some(candidate);
                            generation = next_generation;
                            Ok(generation)
                        })();
                        let _ = reply.send(result);
                    }
                    Command::Click(expected, id, allow_removed, reply) => {
                        let result = dispatch(
                            &mut current,
                            generation,
                            expected,
                            id,
                            None,
                            allow_removed,
                            &tree,
                        );
                        finish_event(result, "click", reply, &logs, &viewport, &tree, &notify);
                    }
                    Command::Input(expected, id, value, reply) => {
                        let result = dispatch(
                            &mut current,
                            generation,
                            expected,
                            id,
                            Some(&value),
                            false,
                            &tree,
                        );
                        finish_event(result, "input", reply, &logs, &viewport, &tree, &notify);
                    }
                    Command::Forget(id, reply) => {
                        let result = match current.as_mut() {
                            Some(session) => session
                                .js
                                .forget_node(id, &mut session.host)
                                .map_err(|e| e.to_string()),
                            None => Ok(()),
                        };
                        let _ = reply.send(result);
                    }
                    Command::Generation(reply) => {
                        let _ = reply.send(if current.is_some() {
                            Ok(generation)
                        } else {
                            Err("no JS session".into())
                        });
                    }
                    Command::Shutdown(reply) => {
                        let result = current
                            .take()
                            .map(|mut s| s.js.dispose(&mut s.host).map_err(|e| e.to_string()))
                            .unwrap_or(Ok(()));
                        let _ = reply.send(result);
                        break;
                    }
                }
            }
        });
        let thread = join.thread().id();
        Self(Arc::new(Inner {
            tx,
            thread,
            join: Mutex::new(Some(join)),
            gate: Arc::clone(&handle.action_gate),
        }))
    }
    fn request<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
        if thread::current().id() == self.0.thread {
            return Err("reentrant JS owner call".into());
        }
        let (tx, rx) = mpsc::channel();
        self.0
            .tx
            .send(make(tx))
            .map_err(|_| "JS owner stopped".to_string())?;
        rx.recv().map_err(|_| "JS owner stopped".to_string())?
    }
    fn check_thread(&self) -> Result<(), String> {
        if thread::current().id() == self.0.thread {
            Err("reentrant JS owner call".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn dispatch_click_locked(&self, id: u64) -> Result<bool, String> {
        let generation = self.generation()?;
        self.request(|reply| Command::Click(generation, id, true, reply))
    }
    pub(crate) fn forget_node_locked(&self, id: u64) -> Result<(), String> {
        self.request(|reply| Command::Forget(id, reply))
    }
    pub(crate) fn dispatch_input_locked(&self, id: u64, value: String) -> Result<bool, String> {
        let generation = self.generation()?;
        self.request(|reply| Command::Input(generation, id, value, reply))
    }
    /// Deliver input to an existing node in the specified generation.
    pub fn dispatch_input(&self, generation: u64, id: u64, value: String) -> Result<bool, String> {
        self.check_thread()?;
        let _gate = self.0.gate.lock().map_err(|e| e.to_string())?;
        self.request(|reply| Command::Input(generation, id, value, reply))
    }
    /// Mount a candidate; on failure the current session and tree remain live.
    pub fn mount(&self, source: &str) -> Result<u64, String> {
        self.check_thread()?;
        let _gate = self.0.gate.lock().map_err(|e| e.to_string())?;
        self.request(|reply| Command::Mount(source.to_owned(), reply))
    }
    /// Deliver a click only to the specified current generation and existing id.
    pub fn dispatch_click(&self, generation: u64, id: u64) -> Result<bool, String> {
        self.check_thread()?;
        let _gate = self.0.gate.lock().map_err(|e| e.to_string())?;
        self.request(|reply| Command::Click(generation, id, false, reply))
    }
    /// Capture the generation for a later click handoff.
    pub fn generation(&self) -> Result<u64, String> {
        self.request(Command::Generation)
    }
    /// Stop the owner and dispose the current session. Further calls fail.
    pub fn shutdown(&self) -> Result<(), String> {
        self.check_thread()?;
        let _gate = self.0.gate.lock().map_err(|e| e.to_string())?;
        self.request(Command::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppHandle;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn handle() -> Arc<SharedTreeHandle> {
        Arc::new(SharedTreeHandle {
            tree: Arc::new(Mutex::new(Tree::new())),
            render: Arc::new(|_| Ok(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            on_change: Mutex::new(None),
            pending_js: Mutex::new(None),
            apply_js: Mutex::new(None),
            last_writeback: Mutex::new(None),
            action_gate: Arc::new(Mutex::new(())),
            click_js: Mutex::new(None),
            input_js: Mutex::new(None),
            forget_js: Mutex::new(None),
            history: Mutex::new(None),
        })
    }
    #[test]
    fn select_selection_delivers_retained_input_and_preserves_options() {
        let handle = handle();
        let viewport = Arc::new(Mutex::new(Viewport {
            width: 320,
            height: 240,
            scale: 1.0,
        }));
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner.mount(r#"
            const root = tekto.create('window');
            const label = tekto.create('text'); tekto.setText(label, 'waiting'); tekto.append(root, label);
            const select = tekto.create('select'); tekto.setProp(select, 'name', 'Size'); tekto.append(root, select);
            const small = tekto.create('menu_item'); tekto.setText(small, 'Small'); tekto.setProp(small, 'checked', 'true'); tekto.append(select, small);
            const large = tekto.create('menu_item'); tekto.setText(large, 'Large'); tekto.append(select, large);
            tekto.onInput(select, value => tekto.setText(label, value));
        "#).unwrap();
        handle.click(NodeId::from_raw(3)).unwrap();
        handle.click(NodeId::from_raw(5)).unwrap();
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(2))
                .unwrap()
                .text
                .as_deref(),
            Some("Large")
        );
        assert!(handle.tree().get(NodeId::from_raw(5)).is_some());
        handle.click(NodeId::from_raw(3)).unwrap();
        handle.click(NodeId::from_raw(4)).unwrap();
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(2))
                .unwrap()
                .text
                .as_deref(),
            Some("Small")
        );
    }

    #[test]
    fn tab_arrow_selection_delivers_retained_callback() {
        let handle = handle();
        let viewport = Arc::new(Mutex::new(Viewport {
            width: 320,
            height: 240,
            scale: 1.0,
        }));
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner.mount(r#"
            const root = tekto.create('window');
            const label = tekto.create('text'); tekto.setText(label, 'waiting'); tekto.append(root, label);
            const list = tekto.create('tab_list'); tekto.append(root, list);
            const first = tekto.create('tab'); tekto.setText(first, 'First'); tekto.setProp(first, 'checked', 'true'); tekto.append(list, first);
            const second = tekto.create('tab'); tekto.setText(second, 'Second'); tekto.append(list, second);
            tekto.onClick(second, () => tekto.setText(label, 'selected'));
        "#).unwrap();
        handle.click(NodeId::from_raw(4)).unwrap();
        handle.key(tekto_tree::NamedActionKey::ArrowRight).unwrap();
        let tree = handle.tree();
        assert_eq!(tree.focus, Some(NodeId::from_raw(5)));
        assert_eq!(
            tree.get(NodeId::from_raw(2)).unwrap().text.as_deref(),
            Some("selected")
        );
    }

    #[test]
    fn context_menu_callbacks_survive_reopening() {
        let handle = handle();
        let viewport = Arc::new(Mutex::new(Viewport {
            width: 320,
            height: 240,
            scale: 1.0,
        }));
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner.mount(r#"
            const root = tekto.create('window');
            const target = tekto.create('button'); tekto.setText(target, 'Target'); tekto.append(root, target);
            const label = tekto.create('text'); tekto.append(root, label);
            const menu = tekto.create('menu'); tekto.setProp(menu, 'name', 'Actions');
            const item = tekto.create('menu_item'); tekto.setText(item, 'Open'); tekto.append(menu, item);
            let count = 0; tekto.onClick(item, () => tekto.setText(label, String(++count)));
            tekto.bindContextMenu(target, menu);
        "#).unwrap();
        for expected in ["1", "2"] {
            handle
                .route_action(|tree| {
                    crate::layout_tree(tree, 320.0, 240.0);
                    (tree.context_menu_at(10.0, 10.0).is_some(), None)
                })
                .unwrap();
            handle.click(NodeId::from_raw(5)).unwrap();
            assert_eq!(
                handle
                    .tree()
                    .get(NodeId::from_raw(3))
                    .unwrap()
                    .text
                    .as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn menu_callback_runs_before_removed_item_cleanup() {
        let handle = handle();
        let viewport = Arc::new(Mutex::new(Viewport {
            width: 320,
            height: 240,
            scale: 1.0,
        }));
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner.mount(r#"
            const root = tekto.create('window');
            const label = tekto.create('text'); tekto.setText(label, 'waiting'); tekto.append(root, label);
            const anchor = tekto.create('button'); tekto.append(root, anchor);
            const menu = tekto.create('menu'); tekto.setProp(menu, 'name', 'Actions');
            const item = tekto.create('menu_item'); tekto.setText(item, 'Run'); tekto.append(menu, item);
            let result = '';
            tekto.onClick(item, () => { result += 'clicked'; tekto.setText(label, result); });
            tekto.onDispose(item, () => { result += ',disposed'; tekto.setText(label, result); });
            tekto.openOverlay(menu, anchor, 'below');
        "#).unwrap();
        handle.click(NodeId::from_raw(5)).unwrap();
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(2))
                .unwrap()
                .text
                .as_deref(),
            Some("clicked,disposed")
        );
        assert!(handle.tree().overlay().is_none());
        assert!(handle.click(NodeId::from_raw(5)).is_err());
    }

    #[test]
    fn host_dismissal_runs_overlay_cleanup_without_tree_lock() {
        let handle = handle();
        let viewport = Arc::new(Mutex::new(Viewport {
            width: 100,
            height: 100,
            scale: 1.0,
        }));
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner
            .mount(
                r#"
            const root = tekto.create('window');
            const label = tekto.create('text');
            const anchor = tekto.create('button');
            const popup = tekto.create('rect');
            const child = tekto.create('button');
            tekto.append(root, label); tekto.append(root, anchor);
            tekto.append(popup, child);
            tekto.onDispose(child, () => tekto.setText(label, 'cleaned'));
            tekto.openOverlay(popup, anchor, 'below');
        "#,
            )
            .unwrap();
        assert!(handle
            .route_action(|tree| tekto_tree::route_named_key(
                tree,
                tekto_tree::NamedActionKey::Escape,
                false
            ))
            .unwrap());
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(2))
                .unwrap()
                .text
                .as_deref(),
            Some("cleaned")
        );
        assert!(handle.tree().overlay().is_none());
        owner
            .mount(
                r#"
            const root = tekto.create('window');
            const label = tekto.create('text');
            const anchor = tekto.create('button');
            const popup = tekto.create('rect');
            tekto.append(root, label); tekto.append(root, anchor);
            tekto.onDispose(popup, () => tekto.setText(label, 'outside'));
            tekto.openOverlay(popup, anchor, 'below');
        "#,
            )
            .unwrap();
        assert!(handle
            .route_action(|tree| {
                tree.click_at(99.0, 99.0);
                (true, None)
            })
            .unwrap());
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(2))
                .unwrap()
                .text
                .as_deref(),
            Some("outside")
        );
    }

    #[test]
    fn keyed_signals_survive_reload_and_report_incompatible_keys() {
        let handle = handle();
        let viewport = Arc::new(Mutex::new(Viewport {
            width: 100,
            height: 100,
            scale: 1.0,
        }));
        let owner = crate::install_js_runtime(&handle, &viewport);
        let core = include_str!("../../../packages/core/src/index.js")
            .replace("export function ", "function ");
        let first = format!(
            r#"{core}
            const root = tekto.create('window');
            const label = tekto.create('text'); tekto.append(root, label);
            const [count, setCount] = createSignal(1, {{ key: 'count' }});
            const [old, setOld] = createSignal('old', {{ key: 'old' }});
            setCount(7); setOld('saved'); tekto.setText(label, String(count()));
        "#
        );
        owner.mount(&first).unwrap();
        let failed = format!(
            r#"{core}
            createSignal(0, {{ key: 'duplicate' }});
            createSignal(1, {{ key: 'duplicate' }});
        "#
        );
        assert!(owner.mount(&failed).is_err());
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(2))
                .unwrap()
                .text
                .as_deref(),
            Some("7")
        );
        let second = format!(
            r#"{core}
            const root = tekto.create('window');
            const label = tekto.create('text'); tekto.append(root, label);
            const [count] = createSignal(0, {{ key: 'count' }});
            const [old] = createSignal(42, {{ key: 'old' }});
            const [fresh] = createSignal('new', {{ key: 'fresh' }});
            const [plain] = createSignal(3);
            tekto.setText(label, [count(), old(), fresh(), plain()].join(','));
        "#
        );
        owner.mount(&second).unwrap();
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(2))
                .unwrap()
                .text
                .as_deref(),
            Some("7,42,new,3")
        );
        let logs = handle.logs();
        assert!(
            logs.iter()
                .any(|s| s.contains("\"count\"") && s.contains("\"old\"")),
            "{logs:?}"
        );
    }

    const COUNTER: &str = r#"
        const root = tekto.create('window');
        const label = tekto.create('text');
        const button = tekto.create('button');
        tekto.append(root, label); tekto.append(root, button);
        let n = 0; tekto.setText(label, '0');
        tekto.onClick(button, () => tekto.setText(label, String(++n)));
        tekto.onDispose(button, () => tekto.setText(label, 'retired'));
    "#;
    fn text(handle: &SharedTreeHandle) -> String {
        handle
            .tree()
            .get(NodeId::from_raw(2))
            .unwrap()
            .text
            .clone()
            .unwrap()
    }
    #[test]
    fn execution_budget_lifecycle_with_outer_timeout() {
        const CHILD: &str = "TEKTO_BUDGET_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "js_owner::tests::execution_budget_lifecycle_with_outer_timeout",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("budget lifecycle subprocess timed out");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        let handle = handle();
        let owner =
            crate::install_js_runtime(&handle, &Arc::new(Mutex::new(Viewport::new(640, 400))));
        assert!(owner
            .mount("for (;;) {} ")
            .unwrap_err()
            .contains("budget exceeded"));
        let generation = owner.mount(r#"
            const root = tekto.create('window');
            const label = tekto.create('text');
            const button = tekto.create('button');
            tekto.append(root, label); tekto.append(root, button);
            let first = true;
            tekto.onClick(button, () => { if (first) { first = false; for (;;) {} } tekto.setText(label, 'recovered'); });
            tekto.onDispose(button, () => { for (;;) {} });
        "#).unwrap();
        assert!(handle
            .click(NodeId::from_raw(3))
            .unwrap_err()
            .to_string()
            .contains("budget exceeded"));
        assert!(handle
            .logs()
            .iter()
            .any(|line| line.contains("budget exceeded")));
        assert!(handle.tree().get(NodeId::from_raw(3)).is_some());
        handle
            .patch(NodeId::from_raw(2), tekto_tree::NodePatch::default())
            .unwrap();
        assert!(owner
            .mount("for (;;) {} ")
            .unwrap_err()
            .contains("budget exceeded"));
        assert_eq!(owner.generation().unwrap(), generation);
        handle.click(NodeId::from_raw(3)).unwrap();
        assert_eq!(text(&handle), "recovered");
        owner.mount(COUNTER).unwrap();
        assert!(handle
            .logs()
            .iter()
            .any(|line| line.contains("js cleanup failed") && line.contains("budget exceeded")));
        handle.click(NodeId::from_raw(3)).unwrap();
        assert_eq!(text(&handle), "1");
        owner
            .mount("tekto.onDispose(1, () => { for (;;) {} });")
            .unwrap();
        assert!(owner.shutdown().unwrap_err().contains("budget exceeded"));
        assert!(owner.generation().is_err());
    }
    #[test]
    fn owner_mount_click_reload_and_shutdown() {
        let handle = handle();
        let calls = Arc::new(AtomicUsize::new(0));
        let rust_calls = calls.clone();
        handle.tree.lock().unwrap().set_event_handler(move |_, _| {
            rust_calls.fetch_add(1, Ordering::SeqCst);
        });
        let owner =
            crate::install_js_runtime(&handle, &Arc::new(Mutex::new(Viewport::new(640, 400))));
        let first = owner.mount(COUNTER).unwrap();
        assert_eq!(text(&handle), "0");
        for value in ["1", "2"] {
            assert!(owner.dispatch_click(first, 3).unwrap());
            assert_eq!(text(&handle), value);
            assert!(handle.tree().get(NodeId::from_raw(3)).is_some());
        }
        assert!(!owner.dispatch_click(first, 99).unwrap());
        assert!(owner
            .mount("tekto.create('window'); throw new Error('bad mount')")
            .is_err());
        handle
            .enqueue_js("throw new Error('bad reload')".into())
            .unwrap();
        assert!(!handle.apply_pending_js());
        assert_eq!(owner.generation().unwrap(), first);
        assert!(owner.dispatch_click(first, 3).unwrap());
        assert_eq!(text(&handle), "3");
        handle.enqueue_js(COUNTER.to_string()).unwrap();
        assert!(handle.apply_pending_js());
        let second = owner.generation().unwrap();
        assert!(second > first);
        assert_eq!(text(&handle), "0"); // old cleanup touched only the retired tree
        assert!(owner.dispatch_click(first, 3).is_err());
        assert!(owner.dispatch_click(second, 3).unwrap());
        assert_eq!(text(&handle), "1");
        let before_click = calls.load(Ordering::SeqCst);
        handle
            .tree
            .lock()
            .unwrap()
            .click(NodeId::from_raw(3))
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), before_click + 1);
        owner.shutdown().unwrap();
        assert_eq!(text(&handle), "retired");
        assert!(owner.dispatch_click(second, 3).is_err());
    }
    #[test]
    fn callback_error_and_removed_node_do_not_break_owner() {
        let handle = handle();
        let changes = Arc::new(AtomicUsize::new(0));
        let observed = changes.clone();
        *handle.on_change.lock().unwrap() = Some(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        let owner =
            crate::install_js_runtime(&handle, &Arc::new(Mutex::new(Viewport::new(640, 400))));
        let generation = owner
            .mount(
                r#"
            const root = tekto.create('window');
            const button = tekto.create('button'); tekto.append(root, button);
            tekto.onClick(button, () => { throw new Error('callback failed'); });
        "#,
            )
            .unwrap();
        assert!(owner
            .dispatch_click(generation, 2)
            .unwrap_err()
            .contains("callback failed"));
        assert!(handle
            .logs()
            .iter()
            .any(|line| line.contains("callback failed")));
        assert_eq!(changes.load(Ordering::SeqCst), 1);
        handle
            .tree
            .lock()
            .unwrap()
            .remove(NodeId::from_raw(2))
            .unwrap();
        assert!(!owner.dispatch_click(generation, 2).unwrap());
        assert_eq!(changes.load(Ordering::SeqCst), 1);
        owner.shutdown().unwrap();
    }
    #[test]
    fn bundled_counter_example_updates_existing_node() {
        if tekto_js::which_esbuild().is_none() {
            assert_ne!(
                std::env::var("TEKTO_REQUIRE_ESBUILD").as_deref(),
                Ok("1"),
                "esbuild is required"
            );
            return;
        }
        let entry =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/counter/app.tsx");
        let bundle = tekto_js::bundle(&entry).expect("bundle counter example");
        let runtime = crate::AppRuntime::new(Tree::new(), "counter");
        let (handle, viewport) = crate::create_handle(&runtime, 640, 400);
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner.mount(&bundle).expect("mount counter example");
        let label = NodeId::from_raw(3);
        let button = NodeId::from_raw(4);
        {
            let tree = handle.tree();
            assert_eq!(tree.get(label).unwrap().text.as_deref(), Some("0"));
            assert_eq!(tree.get(button).unwrap().name.as_deref(), Some("Increment"));
        }
        for expected in ["1", "2"] {
            handle.click(button).expect("click Increment");
            assert_eq!(
                handle.tree().get(label).unwrap().text.as_deref(),
                Some(expected)
            );
        }
        owner.shutdown().unwrap();
    }

    #[test]
    fn input_updates_signal_for_committed_actions() {
        use tekto_platform::key_route::{deliver_key, ElementState, Key, ModifiersState};
        use tekto_tree::ImeEvent;
        let runtime = crate::AppRuntime::new(Tree::new(), "input");
        let (handle, viewport) = crate::create_handle(&runtime, 640, 400);
        let owner = crate::install_js_runtime(&handle, &viewport);
        let core = include_str!("../../../packages/core/src/index.js")
            .replace("export function ", "function ");
        let jsx = include_str!("../../../packages/jsx-runtime/src/index.js")
            .replace(
                "import schema from \"./components.json\" with { type: \"json\" };",
                &format!(
                    "const schema = {};",
                    include_str!("../../../packages/jsx-runtime/src/components.json")
                ),
            )
            .replace("export function ", "function ")
            .replace("export const ", "const ");
        let source = format!(
            r#"{core} {jsx}
            const [value, setValue] = createSignal('');
            let calls = [];
            mount(jsxs('window', {{children: [jsx('text', {{text: () => value()}}),
                jsx('text_field', {{name: 'Name', onInput: v => {{ calls.push(v); setValue(calls.join('|')); }}}})]}}), tekto, {{createEffect}});
        "#
        );
        let generation = owner.mount(&source).unwrap();
        let field = NodeId::from_raw(3);
        handle.type_text(field, "ab").unwrap();
        assert_eq!(text(&handle), "ab");
        handle
            .route_action(|tree| {
                (
                    tree.apply_ime(ImeEvent::Preedit {
                        text: "x".into(),
                        cursor: None,
                    })
                    .unwrap(),
                    None,
                )
            })
            .unwrap();
        assert_eq!(text(&handle), "ab");
        handle
            .route_action(|tree| (tree.apply_ime(ImeEvent::Commit("é".into())).unwrap(), None))
            .unwrap();
        assert_eq!(text(&handle), "ab|abé");
        deliver_key(
            &handle,
            &Key::Character("z".into()),
            Some("z"),
            ElementState::Pressed,
            false,
            ModifiersState::empty(),
            &mut None,
        )
        .unwrap();
        assert_eq!(text(&handle), "ab|abé|abéz");
        use tekto_tree::{FieldKey, NodePatch};
        handle
            .patch(
                field,
                NodePatch {
                    text: Some("xy".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(text(&handle), "ab|abé|abéz");
        handle
            .route_action(|tree| (tree.apply_field_key(FieldKey::Backspace).unwrap(), None))
            .unwrap();
        assert_eq!(text(&handle), "ab|abé|abéz|x");
        handle
            .route_action(|tree| (tree.apply_field_key(FieldKey::Home).unwrap(), None))
            .unwrap();
        handle
            .route_action(|tree| (tree.apply_field_key(FieldKey::Delete).unwrap(), None))
            .unwrap();
        assert_eq!(text(&handle), "ab|abé|abéz|x|");
        handle
            .route_action(|tree| (tree.apply_field_key(FieldKey::Backspace).unwrap(), None))
            .unwrap();
        assert_eq!(text(&handle), "ab|abé|abéz|x|");
        handle
            .patch(
                field,
                NodePatch {
                    disabled: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        let _ = handle.type_text(field, "ignored");
        handle
            .route_action(|tree| (tree.apply_field_key(FieldKey::Insert('q')).unwrap(), None))
            .unwrap();
        assert_eq!(text(&handle), "ab|abé|abéz|x|");
        handle.tree.lock().unwrap().remove(field).unwrap();
        assert!(handle.type_text(field, "stale").is_err());
        assert_eq!(text(&handle), "ab|abé|abéz|x|");
        assert!(!owner
            .dispatch_input(generation, 999, "stale".into())
            .unwrap());
        let next = owner.mount(&source).unwrap();
        assert!(owner.dispatch_input(generation, 3, "stale".into()).is_err());
        assert!(owner.dispatch_input(next, 3, "new".into()).unwrap());
        assert_eq!(text(&handle), "new");
        owner.shutdown().unwrap();
    }

    #[test]
    fn input_errors_and_budget_are_reported_and_session_recovers() {
        let runtime = crate::AppRuntime::new(Tree::new(), "input");
        let (handle, viewport) = crate::create_handle(&runtime, 640, 400);
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner.mount(r#"const root = tekto.create('window'); const field = tekto.create('text_field'); tekto.append(root, field); const label = tekto.create('text'); tekto.append(root, label); let count = 0; tekto.onInput(field, () => { tekto.setText(label, String(++count)); if (count === 1) throw new Error('input failed'); if (count === 2) for (;;) {} });"#).unwrap();
        let field = NodeId::from_raw(2);
        handle
            .route_action(|tree| {
                (
                    tree.apply_field_key(tekto_tree::FieldKey::Left).unwrap(),
                    None,
                )
            })
            .unwrap();
        assert!(handle
            .type_text(field, "a")
            .unwrap_err()
            .to_string()
            .contains("input failed"));
        assert!(handle
            .type_text(field, "b")
            .unwrap_err()
            .to_string()
            .contains("budget"));
        handle.type_text(field, "c").unwrap();
        assert_eq!(
            handle
                .tree()
                .get(NodeId::from_raw(3))
                .unwrap()
                .text
                .as_deref(),
            Some("3")
        );
        assert!(handle
            .logs
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("js input failed")));
        owner.shutdown().unwrap();
    }

    #[test]
    fn desktop_routes_deliver_counter_once_and_order_lifecycle() {
        use tekto_platform::key_route::{deliver_key, ElementState, Key, ModifiersState, NamedKey};
        use tekto_platform::pointer_route::{deliver_pointer, FingerPhase, PointerSession};
        let runtime = crate::AppRuntime::new(Tree::new(), "counter");
        let (handle, viewport) = crate::create_handle(&runtime, 640, 400);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        handle
            .tree
            .lock()
            .unwrap()
            .set_event_handler(move |_, event| {
                if matches!(event, tekto_tree::TreeEvent::Click(_)) {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            });
        let owner = crate::install_js_runtime(&handle, &viewport);
        let core = include_str!("../../../packages/core/src/index.js")
            .replace("export function ", "function ");
        let jsx = include_str!("../../../packages/jsx-runtime/src/index.js")
            .replace(
                "import schema from \"./components.json\" with { type: \"json\" };",
                &format!(
                    "const schema = {};",
                    include_str!("../../../packages/jsx-runtime/src/components.json")
                ),
            )
            .replace("export function ", "function ")
            .replace("export const ", "const ");
        let source = format!(
            r#"{core}
            {jsx}
            const [count, setCount] = createSignal(0);
            mount(jsxs('window', {{children: [jsx('text', {{text: () => count()}}),
              jsx('button', {{onClick: () => setCount(n => n + 1), children: '+'}})]}}), tekto, {{createEffect}});
        "#
        );
        owner.mount(&source).unwrap();
        let button = NodeId::from_raw(3);
        let bounds = handle.tree().get(button).unwrap().bounds;
        let (x, y) = (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        let mut pointer = PointerSession::default();
        for time in [0.0, 100.0] {
            let op = pointer.on_touch(1, FingerPhase::Started, x, y);
            deliver_pointer(&handle, &mut pointer, op, time).unwrap();
            let op = pointer.on_touch(1, FingerPhase::Ended, x, y);
            deliver_pointer(&handle, &mut pointer, op, time + 20.0).unwrap();
        }
        assert_eq!(text(&handle), "2");
        // A swipe and cancellation do not activate the button.
        let op = pointer.on_touch(2, FingerPhase::Started, x, y);
        deliver_pointer(&handle, &mut pointer, op, 220.0).unwrap();
        let op = pointer.on_touch(2, FingerPhase::Moved, x + 20.0, y);
        deliver_pointer(&handle, &mut pointer, op, 230.0).unwrap();
        let op = pointer.on_touch(2, FingerPhase::Ended, x + 20.0, y);
        deliver_pointer(&handle, &mut pointer, op, 240.0).unwrap();
        let op = pointer.on_touch(3, FingerPhase::Started, x, y);
        deliver_pointer(&handle, &mut pointer, op, 250.0).unwrap();
        let op = pointer.on_touch(3, FingerPhase::Cancelled, x, y);
        deliver_pointer(&handle, &mut pointer, op, 260.0).unwrap();
        assert_eq!(text(&handle), "2");
        assert_eq!(handle.tree().focus, Some(button));
        assert_eq!(
            tekto_a11y::tree_update(&handle.tree()).unwrap().focus.0,
            button.raw()
        );
        let mut echo = None;
        for _ in 0..2 {
            deliver_key(
                &handle,
                &Key::Named(NamedKey::Enter),
                None,
                ElementState::Pressed,
                false,
                ModifiersState::empty(),
                &mut echo,
            )
            .unwrap();
        }
        deliver_key(
            &handle,
            &Key::Named(NamedKey::Enter),
            None,
            ElementState::Pressed,
            true,
            ModifiersState::empty(),
            &mut echo,
        )
        .unwrap();
        deliver_key(
            &handle,
            &Key::Named(NamedKey::Enter),
            None,
            ElementState::Released,
            false,
            ModifiersState::empty(),
            &mut echo,
        )
        .unwrap();
        handle.click(button).unwrap();
        assert_eq!(text(&handle), "5");
        assert_eq!(calls.load(Ordering::SeqCst), 5);
        assert!(owner.mount("throw new Error('failed')").is_err());
        handle.click(button).unwrap();
        assert_eq!(text(&handle), "6");
        owner.mount(&source).unwrap();
        handle
            .patch(NodeId::from_raw(2), tekto_tree::NodePatch::default())
            .unwrap();
        handle.click(NodeId::from_raw(2)).unwrap();
        assert_eq!(text(&handle), "0");
        handle.click(button).unwrap();
        assert_eq!(text(&handle), "1");
        owner.mount(r#"const root = tekto.create('window'); const label = tekto.create('text'); const button = tekto.create('button'); tekto.append(root,label); tekto.append(root,button); tekto.onClick(button, () => { throw new Error('boom'); });"#).unwrap();
        assert!(handle.click(button).is_err());
        assert!(handle.logs().iter().any(|line| line.contains("boom")));
        owner.shutdown().unwrap();
        assert!(handle.click(button).is_err());
    }
    #[test]
    fn http_and_handle_click_deliver_signal_and_errors() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let handle = handle();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        handle
            .tree
            .lock()
            .unwrap()
            .set_event_handler(move |_, event| {
                if matches!(event, tekto_tree::TreeEvent::Click(_)) {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            });
        let owner =
            crate::install_js_runtime(&handle, &Arc::new(Mutex::new(Viewport::new(640, 400))));
        let core = include_str!("../../../packages/core/src/index.js")
            .replace("export function ", "function ");
        let source = format!(
            r#"{core}
            const root = tekto.create('window');
            const label = tekto.create('text');
            const button = tekto.create('button');
            tekto.append(root, label); tekto.append(root, button);
            const [count, setCount] = createSignal(0);
            createEffect(() => tekto.setText(label, String(count())));
            tekto.onClick(button, () => setCount(count() + 1));
        "#
        );
        owner.mount(&source).unwrap();
        let endpoint = match tekto_mcp::start_dev_server(handle.clone()) {
            Ok(endpoint) => Some(endpoint),
            Err(error) if error.to_string().contains("Operation not permitted") => None,
            Err(error) => panic!("could not start HTTP test server: {error}"),
        };
        let post = |body: &str| {
            let Some(endpoint) = &endpoint else {
                let id = if body.contains("3") { 3 } else { 2 };
                return match handle.click(NodeId::from_raw(id)) {
                    Ok(()) => "200 OK".to_string(),
                    Err(error) => format!("400 Bad Request {error}"),
                };
            };
            let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port)).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            write!(
                stream,
                "POST /click HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };
        assert_eq!(text(&handle), "0");
        assert!(post(r#"{"id":3}"#).contains("200 OK"));
        assert_eq!(text(&handle), "1");
        handle.click(NodeId::from_raw(3)).unwrap();
        assert_eq!(text(&handle), "2");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(handle.tree().get(NodeId::from_raw(3)).is_some());
        assert!(owner.mount("throw new Error('failed reload')").is_err());
        assert!(post(r#"{"id":3}"#).contains("200 OK"));
        assert_eq!(text(&handle), "3");
        owner.mount(r#"const root = tekto.create('window'); const button = tekto.create('button'); tekto.append(root, button); tekto.onClick(button, () => { throw new Error('click exploded'); });"#).unwrap();
        let response = post(r#"{"id":2}"#);
        assert!(
            response.contains("400 Bad Request") && response.contains("click exploded"),
            "{response}"
        );
        assert!(handle
            .logs()
            .iter()
            .any(|line| line.contains("click exploded")));
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn typed_agent_actions_reach_retained_tsx_and_reject_unrealized_nodes() {
        use tekto_mcp::{SemanticAction, SemanticValue};
        let handle = handle();
        let viewport = Arc::new(Mutex::new(Viewport::new(320, 240)));
        let owner = crate::install_js_runtime(&handle, &viewport);
        owner
            .mount(
                r#"
            const root = tekto.create('window');
            const save = tekto.create('button'); tekto.setProp(save, 'name', 'Save'); tekto.append(root, save);
            const label = tekto.create('text'); tekto.setText(label, 'waiting'); tekto.append(root, label);
            const ghost = tekto.create('button'); tekto.setProp(ghost, 'name', 'Ghost');
            const field = tekto.create('text_field'); tekto.setProp(field, 'name', 'Name'); tekto.append(root, field);
            tekto.onClick(save, () => tekto.setText(label, 'saved'));
            tekto.onInput(field, value => tekto.setText(label, value));
        "#,
            )
            .unwrap();
        let find = |role: &str, name: &str| {
            tekto_a11y::dump(&handle.tree())
                .unwrap()
                .nodes
                .into_iter()
                .find(|node| node.role == role && node.label.as_deref() == Some(name))
                .map(|node| node.id)
        };
        let label_text = || {
            handle
                .tree()
                .get(NodeId::from_raw(3))
                .and_then(|node| node.text.clone())
                .unwrap_or_default()
        };
        assert_eq!(find("Button", "Save"), Some(2));
        // The ghost button was created in JS but never attached, so it is not
        // in the AccessKit tree an agent searches.
        assert_eq!(find("Button", "Ghost"), None);
        assert_eq!(label_text(), "waiting");

        handle
            .semantic_action(SemanticAction::Activate { id: 2 })
            .unwrap();
        assert_eq!(label_text(), "saved");

        handle
            .semantic_action(SemanticAction::SetValue {
                id: 5,
                value: SemanticValue::Text("Ada".into()),
            })
            .unwrap();
        assert_eq!(label_text(), "Ada");

        let error = handle
            .semantic_action(SemanticAction::Activate { id: 4 })
            .unwrap_err();
        assert!(
            error.to_string().contains("stale or not realized"),
            "{error}"
        );
    }
}
