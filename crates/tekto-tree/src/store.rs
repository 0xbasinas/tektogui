//! Opt-in time-travel for a single app value.
//!
//! [`Store::new`] keeps only the current value. [`Store::recording`] keeps a
//! short list of earlier values so the app (or an agent) can undo, redo, or
//! jump. Apps that never call `recording` do not allocate that list.

use std::fmt;

/// Why a jump or step failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// This store was created with [`Store::new`] and does not keep history.
    HistoryOff,
    /// `index` is not in the recorded list.
    UnknownIndex(u64),
    /// Undo or redo has nowhere to go.
    AtEdge,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::HistoryOff => write!(
                f,
                "time travel is off — create the store with Store::recording"
            ),
            StoreError::UnknownIndex(i) => write!(f, "no recorded state at index {i}"),
            StoreError::AtEdge => write!(f, "no further recorded state in that direction"),
        }
    }
}

impl std::error::Error for StoreError {}

/// One recorded value. `index` is its place in the list (0 is the oldest kept).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateInfo {
    pub index: u64,
    pub label: Option<String>,
    pub current: bool,
}

#[derive(Clone, Debug)]
struct Frame<T> {
    value: T,
    label: Option<String>,
}

/// A value plus an optional history.
///
/// `T` is cloned when a frame is recorded. Pick a small `T` (a counter, a
/// `String`, or one JSON value). The store does not watch the scene tree.
#[derive(Clone, Debug)]
pub struct Store<T> {
    frames: Vec<Frame<T>>,
    cursor: usize,
    recording: bool,
    limit: usize,
}

impl<T> Store<T> {
    /// Current value only. `set` replaces it. Undo, redo, and jump do nothing useful.
    pub fn new(initial: T) -> Self {
        Self {
            frames: vec![Frame {
                value: initial,
                label: None,
            }],
            cursor: 0,
            recording: false,
            limit: 1,
        }
    }

    /// Keep up to `limit` values (at least 1), including the current one.
    /// A new `set` drops the redo branch. The oldest frame is dropped when
    /// the list is full.
    pub fn recording(initial: T, limit: usize) -> Self {
        Self {
            frames: vec![Frame {
                value: initial,
                label: None,
            }],
            cursor: 0,
            recording: true,
            limit: limit.max(1),
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn get(&self) -> &T {
        &self.frames[self.cursor].value
    }

    pub fn cursor(&self) -> u64 {
        self.cursor as u64
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn list(&self) -> Vec<StateInfo> {
        self.frames
            .iter()
            .enumerate()
            .map(|(i, frame)| StateInfo {
                index: i as u64,
                label: frame.label.clone(),
                current: i == self.cursor,
            })
            .collect()
    }
}

impl<T: Clone> Store<T> {
    /// Replace the current value.
    ///
    /// When history is off this stays a single frame. When it is on, the
    /// previous value stays in the list and any redo frames are dropped.
    /// `label` names this new frame (`None` leaves it unlabeled).
    pub fn set(&mut self, value: T, label: Option<String>) {
        if !self.recording {
            self.frames[0] = Frame { value, label };
            self.cursor = 0;
            return;
        }
        self.frames.truncate(self.cursor + 1);
        self.frames.push(Frame { value, label });
        self.cursor = self.frames.len() - 1;
        while self.frames.len() > self.limit {
            self.frames.remove(0);
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    /// Move to the previous recorded value.
    pub fn undo(&mut self) -> Result<&T, StoreError> {
        if !self.recording {
            return Err(StoreError::HistoryOff);
        }
        if self.cursor == 0 {
            return Err(StoreError::AtEdge);
        }
        self.cursor -= 1;
        Ok(self.get())
    }

    /// Move toward a newer recorded value after [`Store::undo`].
    pub fn redo(&mut self) -> Result<&T, StoreError> {
        if !self.recording {
            return Err(StoreError::HistoryOff);
        }
        if self.cursor + 1 >= self.frames.len() {
            return Err(StoreError::AtEdge);
        }
        self.cursor += 1;
        Ok(self.get())
    }

    /// Jump to a recorded index from [`Store::list`]. Does not drop frames.
    pub fn jump(&mut self, index: u64) -> Result<&T, StoreError> {
        if !self.recording {
            return Err(StoreError::HistoryOff);
        }
        let index = usize::try_from(index).map_err(|_| StoreError::UnknownIndex(index))?;
        if index >= self.frames.len() {
            return Err(StoreError::UnknownIndex(index as u64));
        }
        self.cursor = index;
        Ok(self.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_off_keeps_one_value() {
        let mut store = Store::new(0);
        for n in 1..20 {
            store.set(n, None);
        }
        assert_eq!(store.len(), 1);
        assert_eq!(*store.get(), 19);
        assert_eq!(store.undo(), Err(StoreError::HistoryOff));
        assert_eq!(store.jump(0), Err(StoreError::HistoryOff));
    }

    #[test]
    fn record_jump_back_and_restore_forward() {
        let mut store = Store::recording(0, 8);
        store.set(1, Some("one".into()));
        store.set(2, None);
        store.set(3, Some("three".into()));
        assert_eq!(*store.get(), 3);
        assert_eq!(store.list().len(), 4);

        assert_eq!(*store.undo().unwrap(), 2);
        assert_eq!(*store.undo().unwrap(), 1);
        assert_eq!(store.list().iter().find(|s| s.current).unwrap().index, 1);

        assert_eq!(*store.jump(3).unwrap(), 3);
        assert_eq!(*store.get(), 3);
        assert_eq!(*store.undo().unwrap(), 2);
        assert_eq!(*store.redo().unwrap(), 3);

        store.set(4, None);
        assert_eq!(*store.get(), 4);
        assert_eq!(store.redo(), Err(StoreError::AtEdge));
        let listed = store.list();
        assert!(listed.iter().any(|s| s.label.as_deref() == Some("three")));
        assert!(listed.last().unwrap().current);
    }

    #[test]
    fn limit_drops_the_oldest_frame() {
        let mut store = Store::recording(0, 3);
        store.set(1, None);
        store.set(2, None);
        store.set(3, None);
        assert_eq!(store.len(), 3);
        assert_eq!(*store.jump(0).unwrap(), 1);
        assert_eq!(*store.jump(2).unwrap(), 3);
    }
}
