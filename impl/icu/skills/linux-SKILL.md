---
name: computer-use-linux-desktop
description: Control the whole Linux desktop by screenshot pixels: look at the screen, click, drag a path, hold a drag open, move the pointer, scroll, press keys and type. Use when the task has no single window to name, or the window verbs (get_window_state and friends) cannot see what you need.
---

# Full-desktop verbs

This is the Linux full-desktop contract of Sky's `linux-SKILL.md` and `sky-full-desktop-api.md`,
served by this layer over cua-driver's desktop scope. There is no window argument anywhere in it:
every coordinate is a pixel of the picture `desktop_screenshot` returns, and the same pixel is the
one the pointer goes to. The window verbs (`list_windows`, `get_window_state`, `click`, ...) are a
different surface with their own coordinates; do not mix a window pixel into a desktop verb.

Sky's verb names map one to one, with a `desktop_` prefix because `click`, `drag`, `press_key`,
`scroll` and `type_text` already belong to the window verbs:

| Sky | here |
| --- | --- |
| `get_screenshot()` | `desktop_screenshot` |
| `click(input)` | `desktop_click` |
| `drag(input)` | `desktop_drag` |
| `drag_handle()` | `desktop_drag_handle` |
| `move(input)` | `desktop_move` |
| `press_key(input)` | `desktop_press_key` |
| `scroll(input)` | `desktop_scroll` |
| `type_text(input)` | `desktop_type_text` |

## The loop

1. `desktop_screenshot` and read the picture. It is the whole screen at the screen's own size.
2. Act on pixels of that picture with `desktop_click`, `desktop_drag`, `desktop_drag_handle`,
   `desktop_move` or `desktop_scroll`, and on the keyboard focus with `desktop_press_key` and
   `desktop_type_text`.
3. `desktop_screenshot` again to see the effect. The agent cursor is drawn into the picture at the
   last pixel you acted on (12 pixels tall, `mouse_size_px` changes it, 0 hides it), so you can see
   where the last action landed. A screenshot never races the redraw of your last action: each verb
   waits out `post_action_sleep_ms` (100 by default) before the next one runs.

## Inputs

`desktop_click`: `x`, `y` (required, pixels); `click_count` (1 to 3, default 1); `mouse_button`
(`left`, `right`, `middle`, or `l`, `r`, `m`; default left); `key` (a chord to hold during the click,
`+`-separated keysym names such as `Control_L` or `Shift_L+Alt_L`; whitespace around `+` is ignored);
`duration` (milliseconds to hold the button down for each click, 0 to 10000).

`desktop_drag`: `path` (required, an array of at least two `{x, y}` points: the button goes down at
the first, the pointer visits every point in order, the button comes up at the last); `key` (a chord
held for the whole drag).

`desktop_drag_handle`: `action` (required) is `start`, `move_to` or `end`. `start` and `move_to` take
`x` and `y`. `start` presses the left button at the point and leaves it down; `move_to` moves the
held button to another point; `end` lets go where the button is. Take a `desktop_screenshot` between
the steps to see the drag in flight. One handle at a time: `start` while one is open, and `move_to`
or `end` with none open, are refused. A button you leave held stays held, so end the handle.

`desktop_move`: `x`, `y` (required); `key` (a chord held while the pointer moves).

`desktop_scroll`: `direction` (required: `up`, `down`, `left`, `right`, or `u`, `d`, `l`, `r`);
`pixels` (how far, default 120); `x` and `y` (the scroll origin; give both or neither, and with
neither the scroll happens where the pointer last was, or in the middle of the screen); `key` (a
chord held during the scroll). A wheel moves in whole clicks of about 40 pixels, so `pixels` is
approximate: the reply says how many clicks went (`_scroll_clicks`).

`desktop_press_key`: `key` (required): a key or a `+`-separated chord in X keysym-style names,
`a`, `space`, `Return`, `Tab`, `Control_L+a`, `Super_L+d`; the last name is the key and the names
before it are held. `duration` holds the key down that many milliseconds (0 to 10000).

`desktop_type_text`: `text` (required, not empty): typed into whatever has the keyboard focus. Click
the field first.

`desktop_screenshot`: no required input; `mouse_size_px`.

All of them but the screenshot take `post_action_sleep_ms` to change the settle window for that call.

## What is refused, and what that means

- A point that is not on the screen (x or y below 0, x at or past the screen width, y at or past
  the height) is refused with `invalidArgument` and `reason: point_off_screen`. Nothing was sent.
  The driver itself would clamp such a point to the screen edge and click there, so the check is
  made first. Take the pixel from the screenshot you just read.
- A malformed argument (a non-number, `click_count` outside 1 to 3, a path of fewer than two points,
  an unknown button or direction, an empty chord or text, a half scroll origin) is refused with
  `invalidArgument`. Nothing was sent. A drag handle used out of order says `drag_already_started`
  or `drag_not_started` in `reason`.
- `desktopUnavailable`: the driver in use does not support what the call needs. `needs` names the
  field (`click.hold_ms`, `drag.path`, `move_cursor.modifier`, `press_key.hold_ms`, `scroll.modifier`,
  `type_text.scope`, or `drag.hold` for a drag handle). A click `duration`, a key `duration`, a
  drag through more than two points, a drag handle and any held `key` need the native driver this
  layer ships; a stock cua-driver would silently ignore the extra field and do a different
  gesture, so nothing is sent instead. A plain click, a two-point drag, a plain move, a scroll, a
  key press and typing work on any driver with the desktop scope.
- `appApprovalRequired` with `app: icu.desktop`: the operator has not approved acting on the whole
  desktop. It is approved once, like an app, and can be denied outright. Taking a screenshot only
  observes and is not gated. Ending a drag handle is never refused for approval, and works on a
  stopped session: it only lets go.
- A held `key` on a native Wayland compositor is held on the seat's virtual keyboard, because a
  virtual pointer carries no modifier state: the window that has the keyboard focus sees the key, which is
  where a toolkit reads it for a click or a wheel turn. A compositor with no virtual keyboard (GNOME
  Shell, KWin) refuses the call (`driverError`) with nothing delivered.

## Notes

- Which screen the picture is. On X11 it is the whole root window: every monitor in one picture, at
  the root's own pixels. On a native Wayland compositor it is the first output the compositor lists
  (the driver's `primary` display): a second output is not in the picture and no desktop verb reaches
  it, though the window verbs reach windows on it. An output with a scale (sway `scale 2`) is
  pictured at its physical pixels, and every pixel you give a verb is one of those.
- A drag handle's button is held by the driver process. With a driver that lives between calls
  (`icu mcp`, or `ICU_SOCKET` to a running driver) the button stays down for as long as you take. A
  driver that exits after each call (`icu call`) takes a native Wayland pointer, and its button, with it, so there
  `desktop_drag_handle start` is refused (`unavailable`, `needs: driver_session`) and nothing is pressed; X11 keeps the
  press in the server and `icu call` works. A handle nobody touches for two minutes is released by the driver.

- A click at the same pixel twice in a row is a double click only if you ask for `click_count: 2`;
  two calls are two separate clicks.
- `desktop_type_text` and `desktop_press_key` go to the keyboard focus, which is not always the
  window under the pointer. Click where you want the text first.
- Nothing here reads the accessibility tree. When the target app has one, the window verbs
  (`get_window_state`, then `click` with an `element_index`) are more reliable than pixels and work
  on windows that are behind others.
