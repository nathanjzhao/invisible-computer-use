// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from "vitest";
import { sendControl } from "./send-control";

// The command form as index.html has it, wired the way main.ts wires it: the submit handler
// starts a task, and while one runs the same button reads Stop and a submit stops it.
function setup() {
  document.body.innerHTML = `<form id="chat-form"><input id="chat-input" /><button type="submit">Send</button></form>`;
  const form = document.querySelector("form")!;
  const input = document.querySelector("input")!;
  const button = document.querySelector("button")!;
  let running = false;
  const starts: string[] = [];
  const stops: number[] = [];
  form.addEventListener("submit", sendControl({
    ready: () => true, statusError: () => false, retryStatus: () => {},
    running: () => running,
    stop: () => { stops.push(Date.now()); running = false; button.textContent = "Send"; },
    start: () => {
      const goal = input.value.trim();
      if (!goal) return false;
      input.value = "";
      starts.push(goal);
      running = true;
      button.textContent = "Stop"; // updateRunControls flips it before the activation is over
      return true;
    },
  }));
  return { form, input, button, starts, stops };
}

afterEach(() => { vi.useRealTimers(); });

describe("one activation of Send starts exactly one run", () => {
  it("a press that lands as a click and then a submit starts the run and does not stop it", () => {
    vi.useFakeTimers();
    const { form, input, button, starts, stops } = setup();
    input.value = "in @Calculator compute 6x7";
    // AXPress / AT-SPI doAction on the button: WebKit's simulated click (which submits the
    // form) followed by a second dispatch of the same press.
    button.click();
    form.dispatchEvent(new Event("submit", { cancelable: true }));
    expect(starts).toEqual(["in @Calculator compute 6x7"]);
    expect(stops).toEqual([]);
    expect(button.textContent).toBe("Stop");
  });

  it("a doubled click (a double-click, or a press delivered twice) does not stop the run it started", () => {
    vi.useFakeTimers();
    const { input, button, starts, stops } = setup();
    input.value = "type hello";
    button.click();
    vi.advanceTimersByTime(150);
    button.click();
    expect(starts).toEqual(["type hello"]);
    expect(stops).toEqual([]);
  });

  it("Return in the command field starts one run", () => {
    vi.useFakeTimers();
    const { form, input, starts, stops } = setup();
    input.value = "type hello";
    form.requestSubmit();
    expect(starts).toEqual(["type hello"]);
    expect(stops).toEqual([]);
  });

  it("Stop pressed as its own activation stops the run", () => {
    vi.useFakeTimers();
    const { input, button, starts, stops } = setup();
    input.value = "type hello";
    button.click();
    vi.advanceTimersByTime(1500);
    button.click();
    expect(starts).toEqual(["type hello"]);
    expect(stops.length).toBe(1);
    expect(button.textContent).toBe("Send");
  });

  it("a task restored as running can be stopped at once", () => {
    vi.useFakeTimers();
    document.body.innerHTML = `<form><button type="submit">Stop</button></form>`;
    const stop = vi.fn();
    const form = document.querySelector("form")!;
    form.addEventListener("submit", sendControl({ ready: () => true, statusError: () => false, retryStatus: () => {},
      running: () => true, stop, start: () => false }));
    document.querySelector("button")!.click();
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
