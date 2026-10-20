// pma web. Draws the screens the server sends: tables with typed columns,
// a drawer for the selected row and its actions, toasts for outcomes. What a
// row is, what a key does to it and what a state means come from the server
// (pma_core::screen and ::keys), as they do for the TUI. Text from the store
// is only ever set as textContent, never parsed as HTML.
"use strict";

const TABS = {
  next: { label: "Next up", group: "Work", about: "What agents can take now, and what waits on you." },
  runs: { label: "Runs", group: "Work", about: "Agent runs waiting on a decision." },
  jobs: { label: "Jobs", group: "Work", about: "Background commands, and plans waiting for approval." },
  agents: { label: "Agents", group: "Work", about: "What running agents are doing right now." },
  matrix: { label: "Matrix", group: "Plan", about: "Every open task, by urgency and importance." },
  workflows: { label: "Workflows", group: "Plan", about: "Workflow instances and the revisions behind them." },
  campaigns: { label: "Campaigns", group: "Plan", about: "One task across many projects." },
  status: { label: "Health", group: "Insight", about: "Every project, worst first, by what needs attention." },
  report: { label: "Report", group: "Insight", about: "What dispatching produced." },
  notes: { label: "Notes", group: "Insight", about: "Notes that belong to no one project." },
  project: { label: "Project", group: null, about: "" },
};
const GROUPS = ["Work", "Plan", "Insight"];

const S = {
  keys: null,
  tab: "next",
  by: 1,
  screens: {},
  pane: {},
  sel: {},
  drawer: null, // null, { mode: "row" }, or { mode: "detail", screen }
  project: "", // the project the project view shows
  previous: "next", // the view it was opened from
  projects: [], // every project, worst first: { name, tier, state, tone }
  filter: "",
};

const $ = (id) => document.getElementById(id);

function el(tag, attrs, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined) continue;
    e.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return e;
}

// Inline markdown an agent writes: **bold**, which may hold `code`, and
// `code`. Built as nodes.
function codeSpans(text) {
  const out = [];
  let last = 0;
  for (const m of text.matchAll(/`([^`]+)`/g)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(el("code", {}, m[1]));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

function inline(text) {
  const out = [];
  let last = 0;
  for (const m of text.matchAll(/\*\*(.+?)\*\*/g)) {
    if (m.index > last) out.push(...codeSpans(text.slice(last, m.index)));
    out.push(el("strong", {}, ...codeSpans(m[1])));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(...codeSpans(text.slice(last)));
  return out;
}

async function api(path, body) {
  const opt = body === undefined
    ? {}
    : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) };
  const r = await fetch(path, opt);
  let data = null;
  try { data = await r.json(); } catch (_) { /* a status with no JSON */ }
  if (!r.ok) throw new Error((data && data.error) || `${r.status} ${r.statusText}`);
  return data;
}

// The binding `key` has on the current tab; one for that tab alone wins over
// one for every tab. As pma_core::keys::lookup.
function lookup(key) {
  const fit = S.keys.bindings.filter((b) => b.key === key);
  return fit.find((b) => b.tabs.includes(S.tab)) || fit.find((b) => b.tabs.length === 0);
}

function onTab() {
  return S.keys.bindings.filter((b) => b.tabs.length === 0 || b.tabs.includes(S.tab));
}

function acts(row) {
  const seen = new Set();
  return onTab().filter((b) => {
    if (b.command.kind !== "act" || b.command.row !== row || seen.has(b.command.act)) return false;
    seen.add(b.command.act);
    return true;
  });
}

function panes() {
  const s = S.screens[S.tab];
  return s && s.body && s.body.panes ? s.body.panes : [];
}

function selected() {
  const p = S.pane[S.tab] || 0;
  const pane = panes()[p];
  const i = (S.sel[S.tab] || [])[p];
  return pane && pane.items[i] ? { pane, item: pane.items[i] } : null;
}

function currentPick() {
  const s = selected();
  return s ? s.item.pick : { kind: "nothing" };
}

// ---------------------------------------------------------------- loading

async function load() {
  const by = S.keys.by[S.by];
  const project = encodeURIComponent(S.project);
  try {
    S.screens[S.tab] = await api(`/api/screen?tab=${S.tab}&by=${by}&project=${project}`);
  } catch (e) {
    S.screens[S.tab] = { error: e.message };
  }
  const ps = panes();
  const sel = S.sel[S.tab] || [];
  S.sel[S.tab] = ps.map((p, i) => Math.min(sel[i] || 0, Math.max(p.items.length - 1, 0)));
  let p = S.pane[S.tab] || 0;
  if (p >= ps.length || !ps[p].items.length) {
    const first = ps.findIndex((x) => x.items.length);
    p = first >= 0 ? first : 0;
  }
  S.pane[S.tab] = p;
  if (S.drawer && S.drawer.mode === "row" && !selected()) S.drawer = null;
  draw();
}

// ---------------------------------------------------------------- drawing

function drawNav() {
  const nav = $("nav");
  nav.replaceChildren();
  const tabKeys = Object.fromEntries(
    S.keys.bindings.filter((b) => b.command.kind === "tab").map((b) => [b.command.tab, b]),
  );
  for (const g of GROUPS) {
    const group = el("div", { class: "nav-group" }, el("h2", {}, g));
    for (const [tab, meta] of Object.entries(TABS)) {
      if (meta.group !== g || !tabKeys[tab]) continue;
      const b = tabKeys[tab];
      group.append(el("button", {
        class: "nav-item",
        type: "button",
        "data-tab": tab,
        "aria-keyshortcuts": b.key,
        onclick: () => run(b),
      }, el("span", {}, meta.label), el("kbd", {}, b.key)));
    }
    nav.append(group);
  }
  const projects = el("div", { class: "nav-group projects" },
    el("h2", { id: "projects-heading" }, "Projects"),
    el("input", {
      id: "project-filter",
      class: "filter",
      type: "search",
      placeholder: "Filter projects",
      "aria-label": "Filter projects",
      oninput: (e) => { S.filter = e.target.value.toLowerCase(); drawProjects(); },
    }),
    el("div", { id: "project-list", role: "list", "aria-labelledby": "projects-heading" }));
  nav.append(projects);
}

function drawProjects() {
  const list = $("project-list");
  if (!list) return;
  list.replaceChildren();
  const shown = S.projects.filter((p) => p.name.toLowerCase().includes(S.filter));
  for (const p of shown) {
    const current = S.tab === "project" && S.project === p.name;
    list.append(el("button", {
      class: "nav-item project-item",
      type: "button",
      role: "listitem",
      title: `${p.name}: ${p.state}`,
      "aria-current": current ? "page" : "false",
      onclick: () => openProject(p.name),
    }, el("span", { class: "project-name" }, el("span", { class: `dot tone-${p.tone}`, "aria-hidden": "true" }), p.name),
       el("span", { class: "tier" }, p.tier === "-" ? "" : `T${p.tier}`)));
  }
  if (!shown.length) list.append(el("p", { class: "empty small" }, S.projects.length ? "No match." : "No projects scanned."));
}

// Every project for the sidebar, from the status view: worst first.
async function loadProjects() {
  try {
    const s = await api("/api/screen?tab=status");
    const items = (s.body.panes && s.body.panes[0] ? s.body.panes[0].items : []);
    S.projects = items.map((it) => ({
      name: it.cells[0].text,
      tier: it.cells[1].text,
      state: it.cells[2].text,
      tone: it.cells[2].tone,
    }));
  } catch (_) {
    S.projects = [];
  }
  drawProjects();
}

function openProject(name) {
  if (S.tab !== "project") S.previous = S.tab;
  S.tab = "project";
  S.project = name;
  S.drawer = null;
  load();
}

function button(b, extraClass) {
  const weight = b.command.weight && b.command.weight !== "default" ? b.command.weight : "";
  return el("button", {
    class: `button ${weight} ${extraClass || ""}`.trim(),
    type: "button",
    "aria-keyshortcuts": b.key,
    onclick: () => run(b),
  }, label(b.label), el("kbd", {}, b.key));
}

function label(text) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function draw() {
  for (const b of document.querySelectorAll(".nav-item")) {
    b.setAttribute("aria-current", b.dataset.tab === S.tab ? "page" : "false");
  }
  const meta = TABS[S.tab];
  const s = S.screens[S.tab];
  const title = S.tab === "project" ? S.project : meta.label;
  $("title").textContent = title;
  $("subtitle").textContent = s && s.header
    ? (S.tab === "project" ? s.header.replace(`${S.project}  `, "") : s.header)
    : meta.about;
  document.title = `${title} - pma`;
  drawProjects();

  const toolbar = $("toolbar");
  toolbar.replaceChildren();
  if (S.tab === "report") {
    S.keys.by.forEach((name, i) => {
      toolbar.append(el("button", {
        class: `button ${i === S.by ? "primary" : ""}`,
        type: "button",
        "aria-pressed": String(i === S.by),
        onclick: () => { S.by = i; load(); },
      }, `By ${name}`));
    });
  }
  for (const b of acts(false)) toolbar.append(button(b));

  const content = $("content");
  content.replaceChildren();
  content.className = "content";
  if (!s) {
    content.append(el("p", { class: "empty" }, "Loading..."));
  } else if (s.error) {
    content.append(el("div", { class: "error-box", role: "alert" }, s.error));
  } else {
    if (s.summary && s.summary.length) {
      content.append(el("dl", { class: "stats" }, s.summary.map(([k, c]) =>
        el("div", { class: "stat" }, el("dt", {}, label(k)), el("dd", { class: `tone-${c.tone}` }, ...inline(c.text))))));
    }
    const ps = panes();
    if (ps.length === 4) content.className = "content grid";
    ps.forEach((p, pi) => content.append(card(p, pi)));
  }
  drawDrawer();
}

function cellNode(c, col) {
  const node = shownNode(c, col);
  // A link opens GitHub in a new tab; the click does not also select the row.
  if (!c.link) return node;
  return el("a", { class: "out", href: c.link, target: "_blank", rel: "noopener noreferrer", onclick: (e) => e.stopPropagation() }, node);
}

function shownNode(c, col) {
  if (col.kind === "badge" && c.text) return el("span", { class: `pill tone-${c.tone}` }, c.text);
  const span = el("span", { class: `tone-${c.tone}` }, ...inline(c.text));
  if (col.kind === "wide") {
    span.classList.add("clamp");
    span.title = c.text;
  }
  return span;
}

function card(p, pi) {
  const focused = pi === (S.pane[S.tab] || 0);
  const id = `pane-${pi}`;
  const box = el("section", { class: `card ${focused ? "focused" : ""}`, "aria-labelledby": id },
    el("header", { class: "card-head" }, el("h2", { id }, p.title), el("span", { class: "count" }, p.items.length)));
  if (!p.items.length) {
    box.append(el("p", { class: "empty" }, p.empty));
    return box;
  }
  // A column no row fills says nothing; it is left out.
  const shown = p.columns.map((_, i) => i).filter((i) => p.items.some((it) => it.cells[i] && it.cells[i].text));
  const head = el("tr", {}, shown.map((i) => el("th", { scope: "col", class: p.columns[i].kind }, p.columns[i].title)));
  const body = el("tbody");
  const foot = el("tfoot");
  p.items.forEach((item, ii) => {
    const sel = focused && ii === S.sel[S.tab][pi];
    const total = item.cells.every((c) => c.tone === "strong");
    const tr = el("tr", {
      "aria-selected": String(sel),
      onclick: () => select(pi, ii),
      ondblclick: () => { select(pi, ii); run(lookup("Enter")); },
    }, shown.map((i) => el("td", { class: p.columns[i].kind }, cellNode(item.cells[i], p.columns[i]))));
    (total ? foot : body).append(tr);
    if (sel) requestAnimationFrame(() => tr.scrollIntoView({ block: "nearest" }));
  });
  box.append(el("div", { class: "table-wrap" }, el("table", {}, el("thead", {}, head), body, foot)));
  return box;
}

function select(pi, ii) {
  S.pane[S.tab] = pi;
  S.sel[S.tab][pi] = ii;
  S.drawer = { mode: "row" };
  draw();
}

function fieldList(pairs) {
  return el("dl", { class: "fields" }, pairs.flatMap(([k, v]) => [el("dt", {}, k), el("dd", {}, v)]));
}

function diffNode(text) {
  return el("div", { class: "diff", role: "region", "aria-label": "Diff" }, text.split("\n").map((l) => {
    const cls = l.startsWith("+++") || l.startsWith("---") || l.startsWith("diff ") ? "meta"
      : l.startsWith("+") ? "add"
      : l.startsWith("-") ? "del"
      : l.startsWith("@@") ? "hunk" : "";
    return el("div", { class: cls }, l || " ");
  }));
}

function drawDrawer() {
  const d = $("drawer");
  const sel = selected();
  if (!S.drawer || (S.drawer.mode === "row" && !sel)) {
    d.hidden = true;
    return;
  }
  d.hidden = false;
  const body = $("drawer-body");
  const actions = $("drawer-actions");
  body.replaceChildren();
  actions.replaceChildren();
  const kicker = $("drawer-kicker");
  kicker.replaceChildren();

  if (S.drawer.mode === "detail") {
    d.classList.add("wide");
    const s = S.drawer.screen;
    const dt = s.body.detail;
    $("drawer-heading").replaceChildren(...inline(dt.title));
    if (dt.badge) kicker.append(el("span", { class: `pill tone-${dt.badge.tone}` }, dt.badge.text));
    if (s.header) kicker.append(s.header);
    if (sel) body.append(el("button", { class: "link back", type: "button", onclick: () => { S.drawer = { mode: "row" }; draw(); } }, "Back to summary"));
    if (dt.notice) body.append(el("div", { class: "notice", role: "alert" }, dt.notice));
    if (dt.fields.length) body.append(fieldList(dt.fields));
    for (const sec of dt.sections) {
      const content = sec.kind === "diff" ? diffNode(sec.text)
        : sec.kind === "log" ? el("pre", { class: "log" }, sec.text)
        : el("div", { class: "prose" }, sec.text.split("\n").flatMap((l, i) => (i ? ["\n", ...inline(l)] : inline(l))));
      body.append(el("section", { class: "section" }, el("h3", {}, sec.title), content));
    }
  } else {
    d.classList.remove("wide");
    const { pane, item } = sel;
    const wide = pane.columns.findIndex((c) => c.kind === "wide");
    const title = wide >= 0 && item.cells[wide] ? item.cells[wide].text : item.cells[0].text;
    $("drawer-heading").replaceChildren(...inline(title));
    kicker.append(pane.title);
    const pairs = [];
    pane.columns.forEach((c, i) => {
      const cell = item.cells[i];
      if (!cell || !cell.text || i === wide) return;
      pairs.push([label(c.title), cellNode(cell, c)]);
    });
    body.append(fieldList(pairs));
    const proj = onTab().find((b) => b.command.kind === "OpenProject");
    const named = item.pick.project;
    if (proj && named) {
      actions.append(el("button", { class: "button", type: "button", onclick: () => run(proj) },
        `Open ${named}`, el("kbd", {}, proj.key)));
    }
    const open = onTab().find((b) => b.command.kind === "Open");
    if (open && (item.pick.kind === "run" || item.pick.kind === "job")) {
      actions.append(el("button", { class: "button primary", type: "button", onclick: () => run(open) },
        item.pick.kind === "run" ? "Review diff" : "View log", el("kbd", {}, "Enter")));
    }
  }
  // Only the acts the server says fit this row.
  const fit = S.drawer.mode === "row" ? sel.item.acts : (sel ? sel.item.acts : []);
  for (const b of acts(true)) if (fit.includes(b.command.act)) actions.append(button(b));
}

// ---------------------------------------------------------------- feedback

function toast(text, tone) {
  const t = el("div", { class: `toast ${tone}`, role: tone === "bad" ? "alert" : "status" },
    el("p", {}, text),
    el("button", { type: "button", "aria-label": "Dismiss", onclick: () => t.remove() }, "Dismiss"));
  $("toasts").append(t);
  setTimeout(() => t.remove(), tone === "bad" ? 12000 : 6000);
  return t;
}

// One dialog serves a question, a prompt and a list. Resolves to the input's
// value, true for OK, or null when cancelled.
function dialog({ title, text, field, input, ok, okClass, cancel }) {
  const d = $("dialog");
  $("dialog-title").textContent = title;
  const t = $("dialog-text");
  t.replaceChildren();
  if (text instanceof Node) t.append(text);
  else if (text) t.textContent = text;
  $("dialog-label").classList.toggle("hidden", field === undefined);
  $("dialog-label-text").textContent = field || "";
  $("dialog-input").value = input || "";
  $("dialog-cancel").classList.toggle("hidden", !cancel);
  const okb = $("dialog-ok");
  okb.textContent = ok || "OK";
  okb.className = `button ${okClass || "primary"}`;
  d.returnValue = "";
  d.showModal();
  if (field !== undefined) $("dialog-input").focus();
  else okb.focus();
  return new Promise((resolve) => {
    d.addEventListener("close", () => {
      if (d.returnValue !== "ok") return resolve(null);
      resolve(field === undefined ? true : $("dialog-input").value);
    }, { once: true });
  });
}

function shortcuts() {
  const grid = el("div", { class: "shortcuts" });
  const group = (title, filter) => {
    const seen = new Map();
    for (const b of onTab().filter(filter)) {
      const k = `${b.command.kind}:${b.command.act || b.command.tab || ""}:${b.label}`;
      if (seen.has(k)) seen.get(k).keys.push(b.key);
      else seen.set(k, { label: b.label, keys: [b.key] });
    }
    if (!seen.size) return;
    grid.append(el("h3", {}, title));
    for (const { label: raw, keys } of seen.values()) {
      const tab = Object.entries(TABS).find(([t]) => t === raw);
      const l = tab ? tab[1].label : raw;
      grid.append(el("div", {}, el("span", {}, label(l)), el("span", {}, keys.map((k) => el("kbd", {}, k)))));
    }
  };
  group("Views", (b) => b.command.kind === "tab");
  group("Move", (b) => !["tab", "act", "Quit"].includes(b.command.kind));
  group(`On ${S.tab === "project" ? S.project : TABS[S.tab].label}`, (b) => b.command.kind === "act" && b.tabs.length > 0);
  group("Anywhere", (b) => b.command.kind === "act" && b.tabs.length === 0);
  return grid;
}

// ---------------------------------------------------------------- acting

async function act(b) {
  const pick = currentPick();
  const name = b.command.act;
  let input = "";
  try {
    if (b.ask.kind !== "nothing") {
      const asked = await api("/api/ask", { tab: S.tab, act: name, pick, project: S.project });
      if (b.ask.kind === "confirm") {
        const yes = await dialog({
          title: label(asked.question),
          ok: label(b.label),
          okClass: b.command.weight === "danger" ? "danger" : "primary",
          cancel: true,
        });
        if (!yes) return;
      } else {
        const typed = await dialog({ title: label(b.label), field: label(b.ask.label), input: asked.input, ok: label(b.label), cancel: true });
        if (typed === null) return;
        input = typed;
      }
    }
    const pending = toast(`${label(b.label)}...`, "accent");
    const out = await api("/api/act", { tab: S.tab, act: name, pick, input, project: S.project })
      .finally(() => pending.remove());
    const lines = out.said.concat(out.warnings.map((w) => `Warning: ${w}`));
    toast(lines.length ? lines.join("\n") : `${label(b.label)}: done.`, out.warnings.length ? "warn" : "good");
  } catch (e) {
    toast(e.message, "bad");
  }
  if (S.drawer && S.drawer.mode === "detail") S.drawer = { mode: "row" };
  await Promise.all([load(), loadProjects()]);
}

async function run(b) {
  if (!b) return;
  const t = S.tab;
  const p = S.pane[t] || 0;
  const sel = S.sel[t] || [];
  const len = (panes()[p] || { items: [] }).items.length;
  const move = (i) => { if (len) { sel[p] = i; if (S.drawer && S.drawer.mode === "row") S.drawer = { mode: "row" }; } };
  switch (b.command.kind) {
    case "tab":
      S.tab = b.command.tab;
      S.drawer = null;
      return load();
    case "Down": move(Math.min((sel[p] || 0) + 1, len - 1)); break;
    case "Up": move(Math.max((sel[p] || 0) - 1, 0)); break;
    case "First": move(0); break;
    case "Last": move(len - 1); break;
    case "NextPane": S.pane[t] = (p + 1) % Math.max(panes().length, 1); break;
    case "PrevPane": S.pane[t] = (p + panes().length - 1) % Math.max(panes().length, 1); break;
    case "Back":
      if (S.drawer) {
        S.drawer = S.drawer.mode === "detail" && selected() ? { mode: "row" } : null;
      } else if (S.tab === "project") {
        S.tab = S.previous;
        return load();
      }
      break;
    case "OpenProject": {
      const named = currentPick().project;
      if (named) return openProject(named);
      toast("Select a row that names a project.", "warn");
      break;
    }
    case "ReportBy":
      S.by = (S.by + 1) % S.keys.by.length;
      return load();
    case "Help":
      await dialog({ title: "Keyboard shortcuts", text: shortcuts(), ok: "Close" });
      break;
    case "Quit": toast("Close the browser tab to leave. pma web runs until Ctrl-C in its terminal.", "accent"); break;
    case "Open": {
      const pick = currentPick();
      if (pick.kind !== "run" && pick.kind !== "job") break;
      try {
        S.drawer = { mode: "detail", screen: await api("/api/open", { pick }) };
      } catch (e) {
        toast(e.message, "bad");
      }
      break;
    }
    case "act": return act(b);
  }
  draw();
}

function onKey(e) {
  if ($("dialog").open) return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
  // Tab moves between panes only where the table binds it; elsewhere it
  // moves the browser's focus as usual.
  const b = lookup(e.key);
  if (!b) return;
  if (e.key === "Enter" && e.target instanceof HTMLButtonElement) return;
  e.preventDefault();
  run(b);
}

function setBusy(text, cls) {
  const b = $("busy");
  b.textContent = text;
  b.className = `busy ${cls}`;
}

async function start() {
  const token = location.hash.slice(1);
  if (token) {
    await fetch("/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ token }),
    });
    history.replaceState(null, "", "/");
  }
  try {
    S.keys = await api("/api/keys");
  } catch (_) {
    $("content").replaceChildren(el("div", { class: "error-box", role: "alert" },
      "Not signed in. Open the address `pma web` printed in its terminal; it carries a one-time token."));
    setBusy("Not signed in", "lost");
    return;
  }
  drawNav();
  $("drawer-close").addEventListener("click", () => { S.drawer = null; draw(); });
  $("help-button").addEventListener("click", () => run(lookup("?")));
  document.addEventListener("keydown", onKey);
  await Promise.all([load(), loadProjects()]);
  const events = new EventSource("/api/events");
  events.addEventListener("tick", (e) => {
    let busy = false;
    try { busy = JSON.parse(e.data).busy; } catch (_) { /* keep false */ }
    setBusy(busy ? "Agents working" : "Idle", busy ? "working" : "");
    if (!$("dialog").open && !(S.drawer && S.drawer.mode === "detail")) {
      load();
      loadProjects();
    }
  });
  events.addEventListener("error", () => setBusy("Disconnected", "lost"));
}

start();
