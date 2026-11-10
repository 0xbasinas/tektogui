package com.tektogui.ime;

import android.app.Activity;
import android.content.Context;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;
import android.widget.FrameLayout;

/**
 * Single-line EditText drawn over a focused Tekto text field.
 *
 * The Rust loop calls {@link #sync} from the native thread. View work runs
 * on the UI thread. {@link #text} and {@link #caret} are safe to read from
 * the native thread between those calls.
 */
public final class FieldOverlay {
    public static volatile String text = "";
    public static volatile int caret = 0;

    private static OverlayEdit field;
    private static boolean updating;

    private FieldOverlay() {}

    public static String text() {
        String value = text;
        return value == null ? "" : value;
    }

    public static int caret() {
        return caret;
    }

    public static void sync(
            final Activity activity,
            final boolean show,
            final boolean push,
            final String value,
            final int caretIn,
            final int x,
            final int y,
            final int w,
            final int h) {
        if (activity == null) {
            return;
        }
        final String safe = value == null ? "" : value;
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                apply(activity, show, push, safe, caretIn, x, y, w, h);
            }
        });
    }

    private static void apply(
            Activity activity,
            boolean show,
            boolean push,
            String value,
            int caretIn,
            int x,
            int y,
            int w,
            int h) {
        ViewGroup content = activity.findViewById(android.R.id.content);
        if (content == null) {
            return;
        }
        if (!show) {
            if (field != null) {
                field.clearFocus();
                hideIme(activity, field);
                field.setVisibility(View.GONE);
            }
            return;
        }
        if (field == null) {
            field = new OverlayEdit(activity);
            field.setSingleLine(true);
            field.setInputType(InputType.TYPE_CLASS_TEXT);
            field.setBackgroundColor(0xFFFFFFFF);
            field.setTextColor(0xFF1A1A1A);
            field.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
            field.setPadding(8, 0, 8, 0);
            field.addTextChangedListener(new TextWatcher() {
                @Override
                public void beforeTextChanged(CharSequence s, int start, int count, int after) {}

                @Override
                public void onTextChanged(CharSequence s, int start, int before, int count) {}

                @Override
                public void afterTextChanged(Editable s) {
                    if (updating || field == null) {
                        return;
                    }
                    publish();
                }
            });
            content.addView(field, new FrameLayout.LayoutParams(Math.max(w, 1), Math.max(h, 1)));
        }
        FrameLayout.LayoutParams lp = new FrameLayout.LayoutParams(Math.max(w, 1), Math.max(h, 1));
        lp.gravity = Gravity.TOP | Gravity.START;
        lp.leftMargin = x;
        lp.topMargin = y;
        field.setLayoutParams(lp);
        field.setVisibility(View.VISIBLE);
        field.bringToFront();
        String current = field.getText() == null ? "" : field.getText().toString();
        if (push && !current.equals(value)) {
            updating = true;
            field.setText(value);
            int c = clamp(caretIn, value.length());
            field.setSelection(c);
            updating = false;
            text = value;
            caret = c;
        }
        if (!field.hasFocus()) {
            field.requestFocus();
            InputMethodManager imm = (InputMethodManager)
                    activity.getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) {
                imm.showSoftInput(field, InputMethodManager.SHOW_IMPLICIT);
            }
        }
        publish();
    }

    private static void publish() {
        if (field == null || field.getText() == null) {
            text = "";
            caret = 0;
            return;
        }
        text = field.getText().toString();
        int c = field.getSelectionStart();
        caret = c < 0 ? text.length() : c;
    }

    private static int clamp(int caretIn, int length) {
        if (caretIn < 0) {
            return 0;
        }
        return Math.min(caretIn, length);
    }

    private static void hideIme(Activity activity, View view) {
        InputMethodManager imm = (InputMethodManager)
                activity.getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm != null && view.getWindowToken() != null) {
            imm.hideSoftInputFromWindow(view.getWindowToken(), 0);
        }
    }

    static final class OverlayEdit extends EditText {
        OverlayEdit(Context context) {
            super(context);
        }

        @Override
        protected void onSelectionChanged(int selStart, int selEnd) {
            super.onSelectionChanged(selStart, selEnd);
            if (!updating && this == field) {
                caret = Math.max(selStart, 0);
            }
        }
    }
}
