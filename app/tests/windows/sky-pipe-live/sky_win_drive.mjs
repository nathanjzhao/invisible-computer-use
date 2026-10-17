// Drives `icu sky-pipe` over a Windows named pipe with Codex's own client code (client.js + native-pipe.js,
// copied beside this file by sky-pipe-live.ps1 with their tslib import pointed at a shim).
// Usage: node sky_win_drive.mjs <\\.\pipe\name> <app>
// The `nodeRepl` global is the host runtime those files expect: `nativePipe.createConnection` returns a socket.
// node's net.createConnection takes a Windows named pipe path exactly as it takes a unix socket path.
import net from "node:net";

const [pipe, APP] = process.argv.slice(2);
globalThis.nodeRepl = {
  nativePipe: { createConnection: (path) => new Promise((resolve, reject) => { const s = net.createConnection(path); s.once("connect", () => resolve(s)); s.once("error", reject); }) },
  env: { SKY_CUA_SERVICE_NATIVE_PIPE_PATH: pipe },
  requestMeta: { "x-codex-turn-metadata": JSON.stringify({ call_id: "call-1", model: "gpt-5.5" }) },
};
const { MacComputerUseClient } = await import("./client.js");

const err = (e) => ({ name: e.name, code: e.code ?? null, errorName: e.errorName ?? null, message: e.message });
const attempt = async (f) => { try { return { ok: await f() }; } catch (e) { return { error: err(e) }; } };

const c = new MacComputerUseClient({ timeoutSeconds: 30 });
const out = { app: APP };
out.apps = await c.listApps();
out.policy = await c.getAppPolicy(APP);
out.state1 = await c.getAppState(APP);
const text = out.state1.skyshot.text;
// element ids by label: the tree lists one element per line, `<index> <role> <label>, Secondary Actions: ...`
const esc = (x) => x.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const find = (label) => {
  const m = text.match(new RegExp(`^\\t*(\\d+) button ${esc(label)}(,|$)`, "m"));
  return m ? Number(m[1]) : null;
};
out.ids = { "6": find("6"), "×": find("×"), "7": find("7"), "=": find("=") };
out.clicks = [];
for (const label of ["6", "×", "7", "="]) {
  const id = out.ids[label];
  if (id == null) { out.clicks.push({ label, error: "no element id for this label in the tree" }); break; }
  out.clicks.push({ label, id, ...(await attempt(() => c.click({ app: APP, elementIndex: id }))) });
}
out.state2 = await c.getAppState(APP);
out.invalidElement = await attempt(() => c.click({ app: APP, elementIndex: 99999 }));
out.invalidApp = await attempt(() => c.getAppPolicy("icu.does-not-exist.exe"));
out.audio = await attempt(() => c.startAudioRecording({}));
const old = new MacComputerUseClient({ apiVersion: "CodexComputerUseIPC-1", timeoutSeconds: 5 });
out.oldVersion = await attempt(() => old.listApps());
console.log(JSON.stringify(out));
process.exit(0);
