// Auto-update UI: a quiet check at startup, then a toast that installs on confirm. The plugin
// (`@tauri-apps/plugin-updater`) does the signature check; this file is what the user sees of it.
// The functions take the plugin's `check` and the process `relaunch` as arguments so a test can
// drive them against a fake.

/** What an install that is run by this app (not the plugin) answers: the installer finished and restarts the app
 * itself (`installed`), or finished and the app has to restart (`relaunch`: a .deb or .rpm the plugin installed), or it did
 * not run and the app keeps going with `message` for the toast (a cancelled prompt on Windows; on Linux a package that
 * needs something this machine lacks). */
export type InstallResult = { status: "installed" } | { status: "relaunch" } | { status: "kept"; message: string };
export type UpdateLike = {
  version: string;
  /** The manifest the plugin read (`platforms` holds one entry per target). */
  rawJson?: Record<string, unknown>;
  downloadAndInstall(onEvent: (ev: UpdateEvent) => void): Promise<void | InstallResult>;
};
export type UpdateEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

function escapeHtml(s: string) {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}

/** An MSI install is offered an update only when the manifest has an MSI for it. The plugin looks for `windows-<arch>-msi` and
 * falls back to the bare `windows-<arch>`, which is the NSIS installer in the documented manifest: installing that on an MSI
 * install puts a second, NSIS copy beside the MSI one. `kind` is what the backend says this install is (`cua_update_install_kind`).
 * Every other install, and a manifest with an MSI entry, is offered as it is. */
export function fitsInstall(update: UpdateLike | null, kind: string): UpdateLike | null {
  if (!update || kind !== "msi") return update;
  const platforms = (update.rawJson as { platforms?: Record<string, unknown> } | undefined)?.platforms;
  const hasMsi = !!platforms && Object.keys(platforms).some((key) => /^windows-[a-z0-9_]+-msi$/.test(key));
  return hasMsi ? update : null;
}

/** An installed Windows copy is updated by the backend (`cua_install_update`): an all-users install asks for its elevation
 * itself and waits for the answer, so a cancelled prompt leaves this copy running (the plugin installs and exits, and a
 * cancelled prompt then left a windowless app); a per-user install is run without any elevation; an MSI install is handed to
 * an elevated `msiexec`, and a cancelled prompt keeps the app. Same shape as the plugin's update so the toast does not care
 * which it has. */
export async function withOwnInstall(
  update: UpdateLike,
  deps: {
    owns: () => Promise<boolean>;
    install: (onEvent: (ev: UpdateEvent) => void) => Promise<InstallResult>;
  },
): Promise<UpdateLike> {
  let owns = false;
  try { owns = await deps.owns(); } catch { owns = false; }
  if (!owns) return update;
  return { version: update.version, downloadAndInstall: (onEvent) => deps.install(onEvent) };
}

/** The update on offer, or null. Any failure (offline, no endpoint, no release yet, a refused
 * manifest, a server that is down) is silence: it is a background check and never nags. */
export async function findUpdate(check: () => Promise<UpdateLike | null>): Promise<UpdateLike | null> {
  try {
    return await check();
  } catch {
    return null;
  }
}

/** Show the "Update X is ready" toast. Install runs the plugin's download, signature check and
 * install, and relaunches only after all of that succeeded; a failure leaves the running app as
 * it was, says so in the toast, and goes to `log`. */
export function showUpdateToast(update: UpdateLike, deps: {
  relaunch: () => Promise<void>; log: (html: string) => void; busy?: () => boolean;
  /** Why this install cannot be updated by this user (a standard user and an all-users install on Windows), or null.
   * Asked before anything is downloaded: the installer's own elevation would fail and leave a per-user copy beside the old one. */
  blocked?: () => Promise<string | null> | string | null;
}) {
  if (document.querySelector(".update-toast")) return;
  const bar = document.createElement("div");
  bar.className = "update-toast";
  // A live region: a screen reader announces what the toast says (including why an update was refused), and the text is on the
  // accessibility tree, where WebKitGTK leaves a plain span out.
  bar.setAttribute("role", "status");
  bar.innerHTML =
    `<div class="up-msg">Update <b>${escapeHtml(update.version)}</b> is ready</div>` +
    `<button class="up-install">Install &amp; restart</button>` +
    `<button class="up-dismiss" title="Dismiss">✕</button>`;
  document.body.appendChild(bar);
  requestAnimationFrame(() => bar.classList.add("in"));
  bar.querySelector(".up-dismiss")!.addEventListener("click", () => bar.remove());
  const msg = bar.querySelector(".up-msg") as HTMLElement;
  // True when the update cannot be installed by this user: the message replaces the offer and there is nothing to click.
  const refuseIfBlocked = async (): Promise<boolean> => {
    let reason: string | null = null;
    try { reason = (await deps.blocked?.()) ?? null; } catch { reason = null; }
    if (!reason) return false;
    msg.textContent = reason;
    bar.querySelector(".up-install")?.remove();
    return true;
  };
  void refuseIfBlocked();
  bar.querySelector(".up-install")!.addEventListener("click", async () => {
    const btn = bar.querySelector(".up-install") as HTMLButtonElement;
    if (await refuseIfBlocked()) return;
    // Install ends in a relaunch, which kills a running task mid-action (measured on macOS 15.7.5: the old app process was gone
    // and the task with it). The offer never interrupts; the install waits until the user has stopped the task or it is done.
    if (deps.busy?.()) {
      msg.textContent = "A task is running. Install after it finishes or stop it.";
      return;
    }
    btn.disabled = true;
    try {
      let total = 0, got = 0;
      const result = await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        else if (ev.event === "Progress") { got += ev.data.chunkLength; msg.textContent = total ? `Downloading ${Math.round((got / total) * 100)}%` : "Downloading…"; }
        else if (ev.event === "Finished") msg.textContent = "Restarting…";
      });
      if (result && result.status === "kept") {
        // The installer did not run (a cancelled or refused prompt): nothing changed and the app goes on.
        btn.disabled = false;
        msg.textContent = result.message;
        return;
      }
      // An install run by the backend restarts the app itself, except a .deb or .rpm, which the app restarts.
      if (!result || result.status === "relaunch") await deps.relaunch();
    } catch (e) {
      btn.disabled = false;
      msg.textContent = "Update failed";
      deps.log(`<b>update</b> failed: ${escapeHtml(String(e)).slice(0, 300)}`);
    }
  });
}
