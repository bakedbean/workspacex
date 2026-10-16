#!/usr/bin/env node
// Screenshot every frame in <rec-dir>/frames.js through one headless Chrome,
// over the DevTools protocol (Node's built-in WebSocket, no npm deps).
// Usage: shoot.mjs <rec-dir> <chrome-binary> [scale]
// Writes <rec-dir>/png/NNNNNN.png at 1280x720 CSS px times scale (default 1.5).
import { spawn } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const [rec, chrome, scaleArg] = process.argv.slice(2);
const scale = Number(scaleArg || 1.5);
const here = path.dirname(fileURLToPath(import.meta.url));
copyFileSync(path.join(here, "frame.html"), path.join(rec, "frame.html"));
const pngDir = path.join(rec, "png");
rmSync(pngDir, { recursive: true, force: true });
mkdirSync(pngDir);

const profile = mkdtempSync(path.join(tmpdir(), "wsx-shoot-"));
const proc = spawn(chrome, ["--headless=new", "--disable-gpu", "--no-first-run",
  "--no-default-browser-check", "--hide-scrollbars", `--user-data-dir=${profile}`,
  "--remote-debugging-port=0", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
const wsUrl = await new Promise((resolve, reject) => {
  let buf = "";
  proc.stderr.on("data", (d) => {
    buf += d;
    const m = buf.match(/DevTools listening on (ws:\/\/\S+)/);
    if (m) resolve(m[1]);
  });
  proc.on("exit", () => reject(new Error("chrome exited before DevTools came up")));
});

const ws = new WebSocket(wsUrl);
await new Promise((r) => ws.addEventListener("open", r, { once: true }));
let seq = 0;
const pending = new Map();
ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
  }
});
const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
  const id = ++seq;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params, sessionId }));
});

try {
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const s = (m, p) => send(m, p, sessionId);
  await s("Emulation.setDeviceMetricsOverride", { width: 1280, height: 720, deviceScaleFactor: scale, mobile: false });
  await s("Page.enable");
  await s("Page.navigate", { url: pathToFileURL(path.join(rec, "frame.html")).href });
  const evalJs = async (expr) => (await s("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true })).result.value;
  for (let t = 0; t < 100 && !(await evalJs("!!(window.FRAMES && window.show)")); t++) {
    await new Promise((r) => setTimeout(r, 100));
  }
  const n = await evalJs("show(0)");
  await evalJs("document.fonts.ready.then(() => true)");
  for (let i = 0; i < n; i++) {
    await evalJs(`show(${i})`);
    const { data } = await s("Page.captureScreenshot", { format: "png" });
    writeFileSync(path.join(pngDir, `${String(i).padStart(6, "0")}.png`), Buffer.from(data, "base64"));
    if (i % 100 === 0) process.stderr.write(`  shot ${i}/${n}\n`);
  }
  console.log(n);
} finally {
  ws.close();
  const exited = new Promise((r) => proc.once("exit", r));
  proc.kill();
  await exited;
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 5 }); } catch {}
}
