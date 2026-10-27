// Prints each tab's toolbar buttons, the toolbar's width against its room
// and the topbar's height, at one window width. Measures; asserts nothing.
//
//   node scripts/web-toolbar.mjs <token-url> <width>
//
// $CHROME overrides the browser, as in web-check.mjs.
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const [url, width] = process.argv.slice(2);
const chrome = process.env.CHROME
  || `${homedir()}/Library/Caches/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-mac-arm64/chrome-headless-shell`;
const origin = new URL(url).origin;
const profile = mkdtempSync(join(process.env.TMPDIR, "cdp-"));
const proc = spawn(chrome, ["--headless", "--remote-debugging-port=9335", `--user-data-dir=${profile}`, `--window-size=${width},900`, "about:blank"], { stdio: "ignore" });
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
let target;
for (let i = 0; i < 50 && !target; i++) {
  try { target = (await (await fetch("http://127.0.0.1:9335/json/list")).json()).find((t) => t.type === "page"); } catch { await wait(200); }
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let seq = 0; const pending = new Map();
ws.addEventListener("message", (m) => { const d = JSON.parse(m.data); if (pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } });
const send = (method, params = {}) => new Promise((r) => { const id = ++seq; pending.set(id, r); ws.send(JSON.stringify({ id, method, params })); });
const js = async (e) => (await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true })).result.result.value;

await send("Page.navigate", { url });
await wait(2000);
for (const tab of ["inbox", "projects", "activity", "matrix", "library", "report", "notes", "settings"]) {
  await send("Page.navigate", { url: `${origin}/?tab=${tab}` });
  await wait(1200);
  const r = await js(`(() => { const t = document.getElementById("toolbar"); const top = document.querySelector(".topbar");
    return { buttons: [...t.querySelectorAll("button")].map(b => b.textContent.replace(/\\s+/g, " ").trim()),
      toolbar: t.scrollWidth, room: t.clientWidth, page: document.documentElement.scrollWidth, view: innerWidth,
      rows: top.getBoundingClientRect().height }; })()`);
  console.log(`${tab}: ${r.buttons.length} buttons, toolbar ${r.toolbar}/${r.room}px, page ${r.page}/${r.view}px, topbar ${Math.round(r.rows)}px tall`);
  console.log(`  ${r.buttons.join(" | ")}`);
}
ws.close(); proc.kill();
