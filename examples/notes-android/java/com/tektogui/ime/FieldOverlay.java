package com.tektogui.ime;

import android.app.Activity;
import android.content.Context;
import android.graphics.Insets;
import android.os.Build;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.DisplayCutout;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
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

    /**
     * Physical pixels: safe left, top, right, bottom, then IME left, top, right, bottom.
     * Safe covers system bars and the display cutout. The IME bottom is how far the
     * keyboard overlaps this window. A resized window that no longer overlaps the
     * keyboard reports 0, so layout does not pad twice.
     */
    public static int[] insets(Activity activity) {
        int[] out = new int[] {0, 0, 0, 0, 0, 0, 0, 0};
        if (activity == null || activity.getWindow() == null) {
            return out;
        }
        View decor = activity.getWindow().getDecorView();
        if (decor == null) {
            return out;
        }
        WindowInsets wi = decor.getRootWindowInsets();
        if (wi == null) {
            return out;
        }
        if (Build.VERSION.SDK_INT >= 30) {
            int bars = WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout();
            Insets safe = wi.getInsets(bars);
            Insets ime = wi.getInsets(WindowInsets.Type.ime());
            out[0] = safe.left;
            out[1] = safe.top;
            out[2] = safe.right;
            out[3] = safe.bottom;
            out[4] = ime.left;
            out[5] = ime.top;
            out[6] = ime.right;
            out[7] = ime.bottom;
            return out;
        }
        int left = Math.max(wi.getSystemWindowInsetLeft(), wi.getStableInsetLeft());
        int top = Math.max(wi.getSystemWindowInsetTop(), wi.getStableInsetTop());
        int right = Math.max(wi.getSystemWindowInsetRight(), wi.getStableInsetRight());
        int sysBottom = wi.getSystemWindowInsetBottom();
        int stableBottom = wi.getStableInsetBottom();
        int safeBottom = Math.min(sysBottom, stableBottom);
        int imeBottom = Math.max(0, sysBottom - stableBottom);
        if (Build.VERSION.SDK_INT >= 28) {
            DisplayCutout cut = wi.getDisplayCutout();
            if (cut != null) {
                left = Math.max(left, cut.getSafeInsetLeft());
                top = Math.max(top, cut.getSafeInsetTop());
                right = Math.max(right, cut.getSafeInsetRight());
                safeBottom = Math.max(safeBottom, cut.getSafeInsetBottom());
            }
        }
        out[0] = left;
        out[1] = top;
        out[2] = right;
        out[3] = safeBottom;
        out[7] = imeBottom;
        return out;
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
        if (push) {
            boolean textChanged = !current.equals(value);
            int c = snap(value, caretIn);
            boolean caretChanged = field.getSelectionStart() != c || field.getSelectionEnd() != c;
            if (textChanged || caretChanged) {
                updating = true;
                if (textChanged) {
                    field.setText(value);
                    c = snap(value, caretIn);
                }
                field.setSelection(c);
                updating = false;
                text = value;
                caret = c;
            }
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

    /// `caretIn` is a UTF-16 index. Snap an index that lands between a surrogate pair.
    private static int snap(String value, int caretIn) {
        int c = clamp(caretIn, value.length());
        if (c > 0
                && c < value.length()
                && Character.isHighSurrogate(value.charAt(c - 1))
                && Character.isLowSurrogate(value.charAt(c))) {
            c += 1;
        }
        return c;
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
