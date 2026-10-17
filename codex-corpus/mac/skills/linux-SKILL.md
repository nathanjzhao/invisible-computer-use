---
name: computer-use
description: Control the Linux desktop through @oai/sky from node_repl. Use for tasks that require screenshots, clicking, dragging, moving the pointer, pressing keys, scrolling, or typing text.
---

# Sky Full Desktop API

## API Reference

Use this as the supported `sky` full desktop API surface.

```ts
import { sky } from "@oai/sky";

const screenshots = await sky.get_screenshot();
await nodeRepl.emitImage(screenshots[0].data_url);

interface FullDesktopComputerUseClient {
  get_screenshot(): Promise<Array<Screenshot>>; // Capture screenshots for the full desktop target.
  click(input: ClickInput): Promise<void>; // Click at a desktop coordinate.
  drag(input: DragInput): Promise<void>; // Drag through an ordered path of desktop coordinates.
  drag_handle(): DragHandle; // Create a drag handle for observing screenshots before releasing the mouse button.
  move(input: MoveInput): Promise<void>; // Move the pointer to a desktop coordinate.
  press_key(input: PressKeyInput): Promise<void>; // Press a `+`-separated keyboard chord on the desktop.
  scroll(input: ScrollInput): Promise<void>; // Scroll the desktop by direction, optionally from a coordinate.
  type_text(input: TypeTextInput): Promise<void>; // Type text into the current focus on the desktop.
  target: "linux";
}

type Screenshot = {
  bytes: Uint8Array; // Raw bytes
  data_url: string; // Base64-encoded JPEG data URL
  filepath: string; // Local file path
};

type ClickInput = {
  click_count?: number; // Number of clicks to perform.
  duration?: number; // Milliseconds to hold the mouse button down for each click.
  key?: string; // Optional key chord to hold during the click, using the same format as `press_key()`.
  mouse_button?: MouseButton; // Mouse button to click.
  x: number; // X coordinate on the desktop screenshot.
  y: number; // Y coordinate on the desktop screenshot.
};

type DragInput = {
  key?: string; // Optional key chord to hold during the drag, using the same format as `press_key()`.
  path: Array<Point>; // At least two desktop coordinates to visit in order during the drag.
};

type DragHandle = {
  end(): Promise<void>; // Release the mouse button and finish the drag.
  move_to(point: Point): Promise<void>; // Move the pressed mouse button to another desktop coordinate.
  start(point: Point): Promise<void>; // Press the mouse button at the starting desktop coordinate.
};

type MoveInput = {
  key?: string; // Optional key chord to hold while moving, using the same format as `press_key()`.
  x: number; // X coordinate on the desktop screenshot.
  y: number; // Y coordinate on the desktop screenshot.
};

type PressKeyInput = {
  duration?: number; // Milliseconds to hold the key or chord before releasing it.
  key: string; // Key or `+`-separated key chord using X Window System keysym-style names, such as `a`, `space`, `Return`, `Tab`, `Control_L+a`, or `Super_L+d`; whitespace around `+` is ignored and common aliases such as `Ctrl`, `Alt`, and `Shift` are accepted.
};

type ScrollInput = {
  direction: Direction; // Direction to scroll.
  key?: string; // Optional key chord to hold during the scroll, using the same format as `press_key()`.
  pixels?: number; // Distance to scroll in pixels.
  x?: number; // Optional X coordinate for the scroll origin.
  y?: number; // Optional Y coordinate for the scroll origin.
};

type TypeTextInput = {
  text: string; // Text to type into the current focus.
};

type MouseButton = "left" | "right" | "middle" | "l" | "r" | "m";

type Point = {
  x: number; // X coordinate on the desktop screenshot.
  y: number; // Y coordinate on the desktop screenshot.
};

type Direction = "up" | "down" | "left" | "right" | "u" | "d" | "l" | "r";
```

For a drag, pass at least two points in `path`. Use `drag_handle()` when you need
to inspect a screenshot before releasing the mouse button. Call `start` once,
then `move_to` as needed, and always call `end` in a `finally` block:

```js
var drag = sky.drag_handle();
await drag.start({ x: 200, y: 300 });
try {
  await drag.move_to({ x: 400, y: 300 });
  await nodeRepl.emitImage((await sky.get_screenshot())[0].data_url);
} finally {
  await drag.end();
}
```
