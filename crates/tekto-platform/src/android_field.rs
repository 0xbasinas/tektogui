#![cfg_attr(not(target_os = "android"), allow(dead_code))]
//! Host-testable plan for the Android text-field overlay.
//!
//! The device path paints a native `EditText` over the focused field and
//! polls its string. This module decides when that string replaces the
//! tree value, and when the tree value (an agent patch, for example) is
//! pushed back into the native field. JNI stays in `android_overlay`.

use tekto_tree::{NodeKind, Tree};

/// Pixel rectangle of the overlay, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldRectPx {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// What the native overlay should do this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AndroidFieldCommand {
    Hide,
    Show {
        id: u64,
        /// Write `text` into the `EditText`. False keeps the user's typing.
        push_text: bool,
        text: String,
        caret: i32,
        rect: FieldRectPx,
    },
}

/// Memory of the last native sample and the last tree value we agreed on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AndroidFieldSync {
    focus: Option<u64>,
    seen_native: String,
    seen_caret: i32,
    seen_tree: String,
    seen_tree_caret: i32,
}

/// Focused text field, already read out of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusedField {
    pub id: u64,
    pub text: String,
    pub caret: usize,
    pub rect: FieldRectPx,
}

/// Latest string and caret from the native field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFieldSample {
    pub text: String,
    pub caret: i32,
}

/// One planner step: a command for the overlay, and an optional tree write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidFieldStep {
    pub command: AndroidFieldCommand,
    /// `(text, caret)` to store on the focused field. `None` leaves the tree.
    pub write: Option<(String, usize)>,
}

/// Logical field bounds to physical pixels. Scale below 0.01 is treated as 1.
pub fn field_rect_px(x: f32, y: f32, w: f32, h: f32, scale: f32) -> FieldRectPx {
    let scale = if scale >= 0.01 { scale } else { 1.0 };
    let px = |v: f32| (v * scale).round() as i32;
    FieldRectPx {
        x: px(x),
        y: px(y),
        w: px(w).max(1),
        h: px(h).max(1),
    }
}

/// Read the focused text field, if focus is one.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn focused_text_field(tree: &Tree, scale: f32) -> Option<FocusedField> {
    let id = tree.focus?;
    let node = tree.get(id)?;
    if node.kind != NodeKind::TextField {
        return None;
    }
    let b = node.bounds;
    Some(FocusedField {
        id: id.raw(),
        text: node.text.clone().unwrap_or_default(),
        caret: node.caret,
        rect: field_rect_px(b.x, b.y, b.width, b.height, scale),
    })
}

fn plain_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn caret_of(text: &str, caret: i32) -> usize {
    let len = text.chars().count();
    if caret < 0 {
        return len;
    }
    (caret as usize).min(len)
}

/// Decide the overlay command and whether the tree should take the native string.
///
/// A new focus pushes the tree value and ignores a stale native sample.
/// After that, a native change wins (the user typed). A tree change that
/// the native field has not shown yet is pushed (an agent patch).
pub fn step_android_field(
    sync: &mut AndroidFieldSync,
    focused: Option<&FocusedField>,
    native: &NativeFieldSample,
) -> AndroidFieldStep {
    let Some(field) = focused else {
        *sync = AndroidFieldSync::default();
        return AndroidFieldStep {
            command: AndroidFieldCommand::Hide,
            write: None,
        };
    };
    let tree_text = plain_text(&field.text);
    let tree_caret = caret_of(&tree_text, field.caret as i32) as i32;
    if sync.focus != Some(field.id) {
        *sync = AndroidFieldSync {
            focus: Some(field.id),
            seen_native: tree_text.clone(),
            seen_caret: tree_caret,
            seen_tree: tree_text.clone(),
            seen_tree_caret: tree_caret,
        };
        return AndroidFieldStep {
            command: AndroidFieldCommand::Show {
                id: field.id,
                push_text: true,
                text: tree_text,
                caret: tree_caret,
                rect: field.rect,
            },
            write: None,
        };
    }
    let native_text = plain_text(&native.text);
    let native_caret = caret_of(&native_text, native.caret) as i32;
    if native_text != sync.seen_native || native_caret != sync.seen_caret {
        sync.seen_native = native_text.clone();
        sync.seen_caret = native_caret;
        sync.seen_tree = native_text.clone();
        sync.seen_tree_caret = native_caret;
        return AndroidFieldStep {
            command: AndroidFieldCommand::Show {
                id: field.id,
                push_text: false,
                text: native_text.clone(),
                caret: native_caret,
                rect: field.rect,
            },
            write: Some((native_text, native_caret as usize)),
        };
    }
    if tree_text != sync.seen_tree || tree_caret != sync.seen_tree_caret {
        sync.seen_native = tree_text.clone();
        sync.seen_caret = tree_caret;
        sync.seen_tree = tree_text.clone();
        sync.seen_tree_caret = tree_caret;
        return AndroidFieldStep {
            command: AndroidFieldCommand::Show {
                id: field.id,
                push_text: true,
                text: tree_text,
                caret: tree_caret,
                rect: field.rect,
            },
            write: None,
        };
    }
    AndroidFieldStep {
        command: AndroidFieldCommand::Show {
            id: field.id,
            push_text: false,
            text: tree_text,
            caret: tree_caret,
            rect: field.rect,
        },
        write: None,
    }
}

/// Apply a planner write to the focused field. Unknown ids are ignored.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn apply_field_write(tree: &mut Tree, text: &str, caret: usize) -> bool {
    tree.set_focused_field_value(text, caret).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(text: &str, caret: usize) -> FocusedField {
        FocusedField {
            id: 7,
            text: text.into(),
            caret,
            rect: FieldRectPx {
                x: 1,
                y: 2,
                w: 30,
                h: 10,
            },
        }
    }

    fn sample(text: &str, caret: i32) -> NativeFieldSample {
        NativeFieldSample {
            text: text.into(),
            caret,
        }
    }

    #[test]
    fn focus_pushes_tree_text_and_hides_when_blurred() {
        let mut sync = AndroidFieldSync::default();
        let focused = field("Milk", 4);
        let step = step_android_field(&mut sync, Some(&focused), &sample("stale", 0));
        match step.command {
            AndroidFieldCommand::Show {
                push_text,
                text,
                caret,
                ..
            } => {
                assert!(push_text);
                assert_eq!(text, "Milk");
                assert_eq!(caret, 4);
            }
            AndroidFieldCommand::Hide => panic!("expected show"),
        }
        assert!(step.write.is_none());
        let step = step_android_field(&mut sync, None, &sample("Milk", 4));
        assert_eq!(step.command, AndroidFieldCommand::Hide);
        assert!(step.write.is_none());
    }

    #[test]
    fn native_typing_writes_the_tree_and_a_patch_pushes_back() {
        let mut sync = AndroidFieldSync::default();
        let focused = field("Hi", 2);
        let _ = step_android_field(&mut sync, Some(&focused), &sample("", 0));
        let step = step_android_field(&mut sync, Some(&focused), &sample("Hi你", 3));
        assert_eq!(step.write, Some(("Hi你".into(), 3)));
        match &step.command {
            AndroidFieldCommand::Show { push_text, .. } => assert!(!push_text),
            AndroidFieldCommand::Hide => panic!("expected show"),
        }
        let patched = field("Agent", 5);
        let step = step_android_field(&mut sync, Some(&patched), &sample("Hi你", 3));
        match step.command {
            AndroidFieldCommand::Show {
                push_text,
                text,
                caret,
                ..
            } => {
                assert!(push_text);
                assert_eq!(text, "Agent");
                assert_eq!(caret, 5);
            }
            AndroidFieldCommand::Hide => panic!("expected show"),
        }
        assert!(step.write.is_none());
    }

    #[test]
    fn caret_moves_and_control_chars_are_stripped() {
        let mut sync = AndroidFieldSync::default();
        let focused = field("ab", 2);
        let _ = step_android_field(&mut sync, Some(&focused), &sample("ab", 2));
        let step = step_android_field(&mut sync, Some(&focused), &sample("ab", 1));
        assert_eq!(step.write, Some(("ab".into(), 1)));
        let mut fresh = AndroidFieldSync::default();
        let dirty = field("a\nb", 3);
        let step = step_android_field(&mut fresh, Some(&dirty), &sample("other", 0));
        match step.command {
            AndroidFieldCommand::Show {
                text,
                push_text,
                caret,
                ..
            } => {
                assert!(push_text);
                assert_eq!(text, "ab");
                assert_eq!(caret, 2);
            }
            AndroidFieldCommand::Hide => panic!("expected show"),
        }
        let step = step_android_field(&mut fresh, Some(&dirty), &sample("a\u{1}b", 1));
        assert_eq!(step.write.as_ref().map(|(t, _)| t.as_str()), Some("ab"));
    }

    #[test]
    fn rect_scales_to_physical_pixels() {
        let rect = field_rect_px(10.0, 20.0, 40.0, 8.0, 2.0);
        assert_eq!(
            rect,
            FieldRectPx {
                x: 20,
                y: 40,
                w: 80,
                h: 16
            }
        );
    }
}
