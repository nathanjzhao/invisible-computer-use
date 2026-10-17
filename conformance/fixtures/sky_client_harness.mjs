// Drives `icu sky-pipe` with Codex's own client code (reference/openai-sky/sky_js_src/targets/mac/
// client.js + native-pipe.js, copied beside this file with their tslib import pointed at a shim).
// Usage: node sky_client_harness.mjs <socket path> <app id>
// The `nodeRepl` global is the host runtime those files expect: a `nativePipe.createConnection`
// that returns a socket, an `env` and the request metadata Codex attaches to every request.
import net from "node:net";

const [sock, APP] = process.argv.slice(2);
globalThis.nodeRepl = {
  nativePipe: { createConnection: (path) => new Promise((resolve, reject) => { const s = net.createConnection(path); s.once("connect", () => resolve(s)); s.once("error", reject); }) },
  env: { SKY_CUA_SERVICE_NATIVE_PIPE_PATH: sock },
  requestMeta: { "x-codex-turn-metadata": JSON.stringify({ call_id: "call-1", model: "gpt-5.5" }) },
};
const { MacComputerUseClient, SkyComputerUseError } = await import("./client.js");

const err = (e) => ({ name: e.name, code: e.code ?? null, errorName: e.errorName ?? null, message: e.message });
const attempt = async (f) => { try { return { ok: await f() }; } catch (e) { return { error: err(e) }; } };

const c = new MacComputerUseClient({ timeoutSeconds: 20 });
const out = {};
out.apps = await c.listApps();
out.policy = await c.getAppPolicy(APP);
out.state1 = await c.getAppState(APP);
const ids = [...out.state1.skyshot.text.matchAll(/^\t*(\d+) /gm)].map((m) => Number(m[1]));
out.ids = ids;
out.click = null;
for (const id of ids) {
  const r = await attempt(() => c.click({ app: APP, elementIndex: id }));
  if (r.ok) { out.click = { id, result: r.ok }; break; }
}
out.state2 = await c.getAppState(APP);
out.state3 = await c.getAppState({ app: APP, disableDiff: true });
out.typed = await attempt(() => c.typeText({ app: APP, text: "hello from the real client" }));
out.key = await attempt(() => c.pressKey({ app: APP, key: "Return" }));
out.scroll = await attempt(() => c.scroll({ app: APP, direction: "down", pages: 1, x: 10, y: 10 }));
out.drag = await attempt(() => c.drag({ app: APP, fromX: 1, fromY: 2, toX: 30, toY: 40 }));
out.pasteHtml = await attempt(() => c.paste({ app: APP, text: "<b>hello</b>", format: "html" }));
out.pasteMd = await attempt(() => c.paste({ app: APP, text: "**hello**", format: "md" }));
out.pasteText = await attempt(() => c.paste({ app: APP, text: "plain client paste", format: "text" }));
out.selectionBefore = await attempt(() => c.selectText({ app: APP, elementIndex: ids[0], text: "cat", selection: "cursor_before" }));
out.selectionAfter = await attempt(() => c.selectText({ app: APP, elementIndex: ids[0], text: "cat", selection: "cursor_after" }));
out.selectedText = await attempt(() => c.selectText({ app: APP, elementIndex: ids[0], text: "cat", prefix: "dog ", suffix: " bird", selection: "text" }));
out.invalidElement = await attempt(() => c.click({ app: APP, elementIndex: 99999 }));
out.invalidApp = await attempt(() => c.getAppPolicy("dev.icu.does-not-exist"));
out.audio = await attempt(() => c.startAudioRecording({}));
const old = new MacComputerUseClient({ apiVersion: "CodexComputerUseIPC-1", timeoutSeconds: 5 });
out.oldVersion = await attempt(() => old.listApps());
console.log(JSON.stringify(out));
process.exit(0);
