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
        /// A true push also moves the caret when the text is unchanged.
        push_text: bool,
        text: String,
        /// UTF-16 code units, for `EditText.setSelection`.
        caret: i32,
        rect: FieldRectPx,
    },
}

/// Memory of the last native sample and the last tree value we agreed on.
///
/// Carets here are Unicode scalar indexes, the same unit as [`tekto_tree::Node::caret`].
/// [`NativeFieldSample::caret`] and [`AndroidFieldCommand`] carets are UTF-16.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AndroidFieldSync {
    focus: Option<u64>,
    seen_native: String,
    seen_caret: i32,
    seen_tree: String,
    seen_tree_caret: i32,
    /// Tree text posted to the overlay and not yet seen in a poll.
    /// A poll of the previous native value is not user input while this is set.
    pending_push: Option<(String, i32)>,
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
    /// UTF-16 code units, from `EditText.getSelectionStart`.
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

fn clamp_scalar(text: &str, caret: i32) -> i32 {
    let len = text.chars().count() as i32;
    if caret < 0 {
        return len;
    }
    caret.min(len)
}

/// Map an `EditText` UTF-16 offset to a Unicode scalar index.
///
/// An offset inside a surrogate pair snaps to the scalar after that pair.
/// A negative offset means the end of `text`.
fn scalar_from_utf16(text: &str, utf16_index: i32) -> i32 {
    if utf16_index < 0 {
        return text.chars().count() as i32;
    }
    let target = utf16_index as usize;
    let mut units = 0usize;
    for (i, ch) in text.chars().enumerate() {
        if units >= target {
            return i as i32;
        }
        let next = units + ch.len_utf16();
        if target < next {
            return (i + 1) as i32;
        }
        units = next;
    }
    text.chars().count() as i32
}

/// Map a Unicode scalar index to an `EditText` UTF-16 offset.
fn utf16_from_scalar(text: &str, scalar: i32) -> i32 {
    let scalar = clamp_scalar(text, scalar);
    text.chars()
        .take(scalar as usize)
        .map(|ch| ch.len_utf16())
        .sum::<usize>() as i32
}

fn show(
    field: &FocusedField,
    push_text: bool,
    text: String,
    scalar_caret: i32,
    write: Option<(String, usize)>,
) -> AndroidFieldStep {
    AndroidFieldStep {
        command: AndroidFieldCommand::Show {
            id: field.id,
            push_text,
            caret: utf16_from_scalar(&text, scalar_caret),
            text,
            rect: field.rect,
        },
        write,
    }
}

/// Drop winit key events only while the overlay class is loaded and a text field is focused.
pub fn overlay_suppresses_keys(class_ready: bool, text_field_focused: bool) -> bool {
    class_ready && text_field_focused
}

/// Decide the overlay command and whether the tree should take the native string.
///
/// A new focus pushes the tree value. That push stays pending until a poll
/// shows it: `FieldOverlay.sync` runs on the UI thread, so the next poll can
/// still be the previous field. A pending push is not user input.
/// After the push lands, a native change wins (the user typed). A tree change
/// the native field has not shown yet is pushed (an agent patch) the same way.
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
    let tree_caret = clamp_scalar(&tree_text, field.caret as i32);
    if sync.focus != Some(field.id) {
        let seen_native = sync.seen_native.clone();
        let seen_caret = sync.seen_caret;
        *sync = AndroidFieldSync {
            focus: Some(field.id),
            seen_native,
            seen_caret,
            seen_tree: tree_text.clone(),
            seen_tree_caret: tree_caret,
            pending_push: Some((tree_text.clone(), tree_caret)),
        };
        return show(field, true, tree_text, tree_caret, None);
    }
    if tree_text != sync.seen_tree || tree_caret != sync.seen_tree_caret {
        sync.pending_push = Some((tree_text.clone(), tree_caret));
        sync.seen_tree = tree_text.clone();
        sync.seen_tree_caret = tree_caret;
    }
    let native_text = plain_text(&native.text);
    let native_caret = scalar_from_utf16(&native_text, native.caret);
    if let Some((pending_text, pending_caret)) = sync.pending_push.clone() {
        if native_text == pending_text && native_caret == pending_caret {
            sync.pending_push = None;
            sync.seen_native = native_text.clone();
            sync.seen_caret = native_caret;
            sync.seen_tree = native_text.clone();
            sync.seen_tree_caret = native_caret;
            return show(field, false, native_text, native_caret, None);
        }
        if native_text == sync.seen_native && native_caret == sync.seen_caret {
            return show(field, true, pending_text, pending_caret, None);
        }
        // The push and a later edit can both land before the next poll.
        sync.pending_push = None;
    }
    if native_text != sync.seen_native || native_caret != sync.seen_caret {
        sync.seen_native = native_text.clone();
        sync.seen_caret = native_caret;
        sync.seen_tree = native_text.clone();
        sync.seen_tree_caret = native_caret;
        return show(
            field,
            false,
            native_text.clone(),
            native_caret,
            Some((native_text, native_caret as usize)),
        );
    }
    show(field, false, tree_text, tree_caret, None)
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

    fn show_text(step: &AndroidFieldStep) -> (bool, &str, i32) {
        match &step.command {
            AndroidFieldCommand::Show {
                push_text,
                text,
                caret,
                ..
            } => (*push_text, text.as_str(), *caret),
            AndroidFieldCommand::Hide => panic!("expected show"),
        }
    }

    #[test]
    fn poll_before_push_lands_does_not_replace_the_new_field() {
        let mut sync = AndroidFieldSync::default();
        let old = field("old", 3);
        let _ = step_android_field(&mut sync, Some(&old), &sample("", 0));
        let ack = step_android_field(&mut sync, Some(&old), &sample("old", 3));
        assert!(ack.write.is_none());

        let mut next = field("new", 3);
        next.id = 8;
        let step = step_android_field(&mut sync, Some(&next), &sample("old", 3));
        assert!(step.write.is_none());
        let (push, text, caret) = show_text(&step);
        assert!(push);
        assert_eq!(text, "new");
        assert_eq!(caret, 3);
        let step = step_android_field(&mut sync, Some(&next), &sample("old", 3));
        assert!(step.write.is_none());
        assert!(show_text(&step).0);
        let step = step_android_field(&mut sync, Some(&next), &sample("new", 3));
        assert!(step.write.is_none());
        assert!(!show_text(&step).0);
    }

    #[test]
    fn poll_before_patch_lands_does_not_restore_the_old_value() {
        let mut sync = AndroidFieldSync::default();
        let focused = field("Hi", 2);
        let _ = step_android_field(&mut sync, Some(&focused), &sample("", 0));
        let ack = step_android_field(&mut sync, Some(&focused), &sample("Hi", 2));
        assert!(ack.write.is_none());
        let patched = field("Agent", 5);
        let step = step_android_field(&mut sync, Some(&patched), &sample("Hi", 2));
        assert!(step.write.is_none());
        assert_eq!(show_text(&step).1, "Agent");
        let step = step_android_field(&mut sync, Some(&patched), &sample("Hi", 2));
        assert!(step.write.is_none());
        let (push, text, _) = show_text(&step);
        assert!(push);
        assert_eq!(text, "Agent");
        let step = step_android_field(&mut sync, Some(&patched), &sample("Agent", 5));
        assert!(step.write.is_none());
        assert!(!show_text(&step).0);
    }

    #[test]
    fn supplementary_caret_converts_utf16_both_ways() {
        let mut sync = AndroidFieldSync::default();
        let focused = field("😀a", 2);
        let step = step_android_field(&mut sync, Some(&focused), &sample("", 0));
        assert!(step.write.is_none());
        let (push, text, caret) = show_text(&step);
        assert!(push);
        assert_eq!(text, "😀a");
        assert_eq!(caret, 3, "end of 😀a is three UTF-16 units");
        let ack = step_android_field(&mut sync, Some(&focused), &sample("😀a", 3));
        assert!(ack.write.is_none());
        let moved = field("😀a", 1);
        let step = step_android_field(&mut sync, Some(&moved), &sample("😀a", 3));
        assert!(step.write.is_none());
        let (push, text, caret) = show_text(&step);
        assert!(push, "a caret-only tree change is still a push");
        assert_eq!(text, "😀a");
        assert_eq!(caret, 2);
        let step = step_android_field(&mut sync, Some(&moved), &sample("😀a", 2));
        assert!(step.write.is_none(), "the caret push landed");

        let mut sync = AndroidFieldSync::default();
        let focused = field("😀a", 0);
        let _ = step_android_field(&mut sync, Some(&focused), &sample("😀a", 0));
        let step = step_android_field(&mut sync, Some(&focused), &sample("😀a", 2));
        assert_eq!(step.write, Some(("😀a".into(), 1)));
        assert_eq!(show_text(&step).2, 2);
    }

    #[test]
    fn keys_are_suppressed_only_while_a_text_field_is_focused() {
        assert!(!overlay_suppresses_keys(false, false));
        assert!(!overlay_suppresses_keys(false, true));
        assert!(!overlay_suppresses_keys(true, false));
        assert!(overlay_suppresses_keys(true, true));
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
