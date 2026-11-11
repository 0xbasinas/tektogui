# Keyboard navigation

Tab moves through attached buttons and text fields in tree order; Shift+Tab moves backwards. Both wrap. If focus is elsewhere, Tab starts at the first control and Shift+Tab at the last. With no controls, focus stays where it is. Enter and Space activate a focused button. With a retained desktop QuickJS session, this also runs the button's `onClick` (see `docs/tsx-events.md`). A focus outline marks the focused control.

Traversal includes the currently attached tree only. Virtualized lists materialize only part of their rows, so this does not provide navigation through every list item. Directional navigation and custom tab order are not supported.
