// The command form's one button is Send while no task runs and Stop while one
// does. What a submit of the form means is decided here.
//
// The button turns into Stop as soon as a task starts, inside the activation
// that started it. An activation that reaches the form twice (an accessibility
// press that WebKitGTK delivers as a click and then another dispatch, a press
// sent twice, a double-click) used to land its second half on Stop and end the
// task it had just started: "on it", then "stopped by request" with no model
// call (study/audit/linux-final-2026-10-01, bug 2). A Stop that arrives within
// STOP_GRACE_MS of the start is that same activation and is ignored; a later
// press of Stop stops as before, and a task restored as already running can be
// stopped at once.

/** How long after a start a submit is taken as part of the activation that started it. */
export const STOP_GRACE_MS = 800;

export interface SendControlDeps {
  /** Whether the task status is known (a submit before it only retries the status check). */
  ready(): boolean;
  statusError(): boolean;
  retryStatus(): void;
  /** Whether a task is running (or starting); a submit then means Stop. */
  running(): boolean;
  stop(): void;
  /** Start a task from the command field; true when one started. */
  start(): boolean;
  /** A monotonic clock in ms (performance.now); a wall clock that steps back would keep Stop dead. */
  now?(): number;
  /** A Stop pressed inside the grace window was ignored; tell the user to press it again. */
  ignored?(): void;
}

/** The form's submit handler. */
export function sendControl(deps: SendControlDeps): (e: Event) => void {
  const now = deps.now ?? (() => performance.now());
  let startedAt = -Infinity;
  return (e: Event) => {
    e.preventDefault();
    if (!deps.ready()) { if (deps.statusError()) deps.retryStatus(); return; }
    if (deps.running()) {
      if (now() - startedAt < STOP_GRACE_MS) { deps.ignored?.(); return; }
      deps.stop();
      return;
    }
    if (deps.start()) startedAt = now();
  };
}
