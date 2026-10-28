/* Shared mock engine for the Hopper redesign mockups. Nothing here talks to a network. */
(function () {
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const $ = (s, r = document) => r.querySelector(s);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

  // ------------------------------------------------------------------ pixel sprites (original art)
  const SPRITES = {
    hopper: {
      pal: { k: "var(--hop-k,#17161d)", L: "var(--hop-L,#e4e7eb)", G: "var(--hop-G,#a3aab5)", D: "var(--hop-D,#4b505b)" },
      rows: [
        "kkkkkkkkkkkkkkkk", "kLLLLLLLLLLLLLGk", "kLDDDDDDDDDDDDGk", "kLDDDDDDDDDDDDGk", "kkGGGGGGGGGGGGkk",
        "kLGGGGGGGGGGGk", "kLGGGGGGGGGk", "kLGGGGGGGk", "kLGGGGGk", "kGGGGk", "kGGGGk", "kDDk", "kDDk", "kkkk",
      ],
    },
    creeper: {
      pal: { g: "#4fa83a", m: "#69c24c", k: "#101410" },
      rows: ["gmggmgmg", "mgmgggmg", "gkkggkkg", "mkkgmkkg", "gggkkggg", "ggkkkkgm", "mgkkkkgg", "ggkggkgg"],
    },
    diamond: {
      pal: { c: "#4aedd9", w: "#d5fff8", d: "#1fa39a" },
      rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
    },
    emerald: {
      pal: { c: "#2fdc6b", w: "#c4ffd8", d: "#0c8f3f" },
      rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
    },
    gold: {
      pal: { c: "#f7c93a", w: "#fff3b0", d: "#b57d0b" },
      rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
    },
    redstone: {
      pal: { c: "#e5392b", w: "#ff8a74", d: "#7d1209" },
      rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
    },
    villager: {
      pal: { s: "#c4906c", n: "#9c6b4b", b: "#3b2a1e", W: "#ffffff", G: "#4d8a3e" },
      rows: ["ssssssssss", "ssssssssss", "bbbbbbbbbb", "sWGssssGWs", "ssssssssss", "ssssnnssss", "sssnnnnsss", "sssnnnnsss", "ssssnnssss", "ssssssssss"],
    },
    cube: { // a plain material block, tinted through --c
      pal: { t: "color-mix(in srgb, var(--c) 100%, #fff 22%)", f: "var(--c)", s: "color-mix(in srgb, var(--c) 100%, #000 38%)", k: "rgba(0,0,0,.45)" },
      rows: ["kkkkkkkkkk", "ktttttttsk", "ktttttttsk", "kfffffffsk", "kfffffffsk", "kfffffffsk", "kfffffffsk", "kffffffssk", "kssssssssk", "kkkkkkkkkk"],
    },
  };

  function sprite(name, px = 2, cls = "") {
    const s = SPRITES[name];
    const w = Math.max(...s.rows.map((r) => r.length));
    const h = s.rows.length;
    let rects = "";
    s.rows.forEach((row, y) => {
      const off = Math.floor((w - row.length) / 2);
      let x = 0;
      while (x < row.length) {
        const ch = row[x];
        let run = 1;
        while (x + run < row.length && row[x + run] === ch) run++;
        if (ch !== ".") rects += `<rect x="${x + off}" y="${y}" width="${run}" height="1" fill="${s.pal[ch]}"/>`;
        x += run;
      }
    });
    return `<svg class="sprite ${cls}" width="${w * px}" height="${h * px}" viewBox="0 0 ${w} ${h}" aria-hidden="true">${rects}</svg>`;
  }

  // ------------------------------------------------------------------ mock data
  const MATERIAL = { local: "#5dbb3f", dev: "#4f78f0", staging: "#f2b928", production: "#ec4a3c", other: "#a98bff" };
  const METHOD_COLOR = { GET: "#5ec8ff", POST: "#4ade80", PUT: "#f5c542", PATCH: "#d29bff", DELETE: "#ff6b5b" };

  const envs = [
    { name: "local", kind: "local", base: "http://localhost:5080", vars: { baseUrl: "http://localhost:5080", userId: "42" }, secrets: [{ name: "apiKey", set: true }] },
    { name: "dev", kind: "dev", base: "https://dev.api.acme.test", vars: { baseUrl: "https://dev.api.acme.test", userId: "42" }, secrets: [{ name: "apiKey", set: true }] },
    { name: "staging", kind: "staging", base: "https://staging.api.acme.test", vars: { baseUrl: "https://staging.api.acme.test", userId: "1017" }, secrets: [{ name: "apiKey", set: false }] },
    { name: "production", kind: "production", base: "https://api.acme.com", vars: { baseUrl: "https://api.acme.com", userId: "9001" }, secrets: [{ name: "apiKey", set: true }, { name: "adminToken", set: true }] },
  ];

  const requests = [
    { id: "u-get", col: "Account API", folder: "Users", m: "GET", name: "Get user", path: "/users/{{userId}}", status: 200, ago: "2m", pin: 1 },
    { id: "u-post", col: "Account API", folder: "Users", m: "POST", name: "Create user", path: "/users", status: 201, ago: "9m", pin: 2 },
    { id: "u-del", col: "Account API", folder: "Users", m: "DELETE", name: "Delete user", path: "/users/{{userId}}", status: 204, ago: "1h", pin: 3, agentAsks: true },
    { id: "inv", col: "Billing API", folder: "Invoices", m: "GET", name: "List open invoices", path: "/invoices?status=open", status: 200, ago: "3h", pin: 4 },
    { id: "chg", col: "Billing API", folder: "Charges", m: "POST", name: "Create charge", path: "/charges", status: 402, ago: "3h", pin: 5 },
    { id: "rep", col: "Billing API", folder: "Reports", m: "GET", name: "Quarterly report", path: "/reports/quarterly", status: 500, ago: "yesterday", pin: 6 },
    { id: "miss", col: "Account API", folder: "Users", m: "GET", name: "Get user (missing)", path: "/users/0", status: 404, ago: "yesterday" },
    { id: "health", col: "Notifications", folder: "", m: "GET", name: "Health check", path: "/health", status: 200, ago: "2d" },
    { id: "tea", col: "Notifications", folder: "", m: "GET", name: "Brew coffee", path: "/brew", status: 418, ago: "2d" },
  ];

  const specRoutes = [
    { m: "GET", path: "/users/{id}/sessions", col: "Account API spec" },
    { m: "POST", path: "/users/{id}/sessions/revoke", col: "Account API spec" },
    { m: "GET", path: "/invoices/{id}/pdf", col: "Billing API spec" },
  ];

  const BODY = {
    "u-get": { id: 42, name: "Alex Moreno", email: "alex@acme.test", plan: "team", roles: ["admin", "billing"], lastLogin: null, createdAt: "2026-03-14T09:21:07Z" },
    "u-post": { id: 43, name: "Sam Rivera", email: "sam@acme.test", plan: "free", roles: [], createdAt: "2026-10-27T14:02:55Z" },
    "u-del": null,
    inv: { total: 3, items: [{ id: "inv_8841", amount: 129.0, currency: "USD", dueAt: "2026-11-02" }, { id: "inv_8842", amount: 49.5, currency: "USD", dueAt: "2026-11-09" }, { id: "inv_8850", amount: 980.0, currency: "EUR", dueAt: "2026-11-15" }] },
    chg: { error: "payment_required", message: "Card on file was declined", retryable: false },
    rep: { error: "internal_error", message: "Unhandled exception in ReportBuilder.Quarterly", traceId: "7f3a-91c2-0b6d" },
    miss: { error: "user_not_found", message: "No user with id 0", hint: "Ids start at 1" },
    health: { status: "ok", version: "4.12.0", uptimeTicks: 5184000 },
    tea: { error: "i_am_a_teapot", message: "The server refuses to brew coffee", brewing: ["tea"] },
  };
  const STATUS_TEXT = { 200: "OK", 201: "Created", 204: "No Content", 402: "Payment Required", 404: "Not Found", 418: "I'm a teapot", 500: "Internal Server Error" };
  const HEADERS = (type) => ({ "content-type": type, "x-request-id": "b1f4-77c0-3e19", "cache-control": "no-store", server: "kestrel", date: "Tue, 27 Oct 2026 14:02:55 GMT" });

  async function fakeSend(req, env) {
    const t0 = performance.now();
    await sleep((env.kind === "local" ? 200 : 460) + Math.random() * 380);
    const body = BODY[req.id];
    const text = body == null ? "" : JSON.stringify(body, null, 2);
    return {
      req, env, status: req.status, text: STATUS_TEXT[req.status] || "", body,
      ms: Math.max(9, Math.round(performance.now() - t0 - 140 - Math.random() * 60)),
      bytes: text.length, headers: HEADERS(body == null ? "text/plain" : "application/json; charset=utf-8"),
    };
  }

  const statusClass = (s) => (s >= 500 ? "5xx" : s >= 400 ? "4xx" : s >= 300 ? "3xx" : "2xx");
  const ticks = (ms) => { const t = Math.max(1, Math.round(ms / 50)); return `${t} tick${t === 1 ? "" : "s"}`; };
  const stacks = (b) => { const s = Math.max(1, Math.ceil(b / 64)); return `${s} stack${s === 1 ? "" : "s"}`; };
  const size = (b) => (b < 1024 ? `${b} B` : `${(b / 1024).toFixed(1)} KB`);
  const resolve = (str, env) => str.replace(/\{\{(\w+)\}\}/g, (_, k) => (env.vars[k] ?? `{{${k}}}`));

  function jsonHtml(v, ind = 0) {
    const pad = "  ".repeat(ind);
    const leaf = (cls, raw, text) => `<span class="${cls} jv" data-copy="${esc(raw)}" title="Click to copy">${esc(text)}</span>`;
    if (v === null) return leaf("jb", "null", "null");
    if (typeof v === "string") return leaf("js", v, JSON.stringify(v));
    if (typeof v === "number") return leaf("jn", v, String(v));
    if (typeof v === "boolean") return leaf("jb", v, String(v));
    if (Array.isArray(v)) {
      if (!v.length) return "[]";
      return "[\n" + v.map((x) => pad + "  " + jsonHtml(x, ind + 1)).join(",\n") + "\n" + pad + "]";
    }
    const keys = Object.keys(v);
    return "{\n" + keys.map((k) => `${pad}  <span class="jk">"${esc(k)}"</span>: ${jsonHtml(v[k], ind + 1)}`).join(",\n") + "\n" + pad + "}";
  }

  const GLYPHS = "ᔑᓵᒷᓭᔭᑑ∷ℸ⍑╎⊣⍊ꖌ";
  const enchant = (n = 10) => Array.from({ length: n }, () => GLYPHS[Math.floor(Math.random() * GLYPHS.length)]).join("");

  // ------------------------------------------------------------------ toasts
  function ensureToasts() {
    let t = $("#toasts");
    if (!t) { t = document.createElement("div"); t.id = "toasts"; t.setAttribute("role", "status"); t.setAttribute("aria-live", "polite"); document.body.append(t); }
    return t;
  }
  function pushToast(el, ms) {
    const box = ensureToasts();
    box.append(el);
    setTimeout(() => { el.classList.add("out"); setTimeout(() => el.remove(), 280); }, ms);
    while (box.children.length > 3) box.firstChild.remove();
  }
  const seen = new Set();
  const toast = {
    advancement(id, { sprite: sp, title, desc }) {
      if (seen.has(id)) return;
      seen.add(id);
      const el = document.createElement("div");
      el.className = "toast";
      el.innerHTML = `<div class="t-icon">${sprite(sp, 3)}</div><div><div class="t-title">Advancement made!</div><div class="t-desc"><b>${esc(title)}</b> · ${esc(desc)}</div></div>`;
      pushToast(el, 5200);
    },
    plain(text, sub = "") {
      const el = document.createElement("div");
      el.className = "toast plain";
      el.innerHTML = `<div><div class="t-title">${esc(text)}</div>${sub ? `<div class="t-desc">${esc(sub)}</div>` : ""}</div>`;
      pushToast(el, 2600);
    },
  };

  // ------------------------------------------------------------------ xp orbs: fly from one element to another
  function orbs(from, to, n = 5) {
    if (reduced || !from || !to) return Promise.resolve();
    const a = from.getBoundingClientRect(), b = to.getBoundingClientRect();
    const jobs = Array.from({ length: n }, (_, i) => {
      const o = document.createElement("div");
      o.className = "xp-orb";
      document.body.append(o);
      const sx = a.left + a.width * (0.2 + Math.random() * 0.6), sy = a.top + a.height / 2;
      const ex = b.left + b.width * (0.15 + Math.random() * 0.7), ey = b.top + b.height / 2;
      const mx = sx + (Math.random() - 0.5) * 120, my = Math.min(sy, ey) - 30 - Math.random() * 50;
      const anim = o.animate(
        [
          { transform: `translate(${sx}px,${sy}px) scale(.4)`, opacity: 0 },
          { transform: `translate(${mx}px,${my}px) scale(1)`, opacity: 1, offset: 0.35 },
          { transform: `translate(${ex}px,${ey}px) scale(.55)`, opacity: 0.9 },
        ],
        { duration: 720 + Math.random() * 260, delay: i * 55, easing: "cubic-bezier(.45,.05,.3,1)", fill: "both" },
      );
      return anim.finished.then(() => o.remove());
    });
    return Promise.all(jobs);
  }

  // ------------------------------------------------------------------ hold-to-confirm: break the block
  function mine(btn, { ms = 750, onDone, onProgress }) {
    const N = 48;
    const order = Array.from({ length: N }, (_, i) => i).sort(() => Math.random() - 0.5);
    const cols = 12, rows = 4;
    let layer = btn.querySelector(".crack");
    if (!layer) {
      layer = document.createElement("span");
      layer.className = "crack";
      layer.innerHTML = `<svg viewBox="0 0 ${cols} ${rows}" preserveAspectRatio="none">${Array.from({ length: N }, (_, i) => `<rect x="${i % cols}" y="${Math.floor(i / cols)}" width="1" height="1" opacity="0"/>`).join("")}</svg>`;
      btn.append(layer);
    }
    const cells = [...layer.querySelectorAll("rect")];
    let start = 0, raf = 0, holding = false;
    const paint = (p) => { const k = Math.floor(p * N); cells.forEach((c, i) => c.setAttribute("opacity", order.indexOf(i) < k ? "1" : "0")); onProgress && onProgress(p); };
    const tick = (t) => {
      if (!holding) return;
      const p = Math.min(1, (t - start) / ms);
      paint(p);
      if (p >= 1) { holding = false; setTimeout(() => paint(0), 160); onDone(); return; }
      raf = requestAnimationFrame(tick);
    };
    const down = () => { if (holding) return; holding = true; start = performance.now(); raf = requestAnimationFrame(tick); };
    const up = () => { if (!holding) return; holding = false; cancelAnimationFrame(raf); const from = performance.now(); const fade = () => { const p = Math.max(0, 1 - (performance.now() - from) / 200); paint(p * (parseFloat(btn.dataset.mine || "0.5"))); if (p > 0) requestAnimationFrame(fade); else paint(0); }; fade(); };
    btn.addEventListener("pointerdown", down);
    btn.addEventListener("pointerup", up);
    btn.addEventListener("pointerleave", up);
    btn.addEventListener("pointercancel", up);
    btn.addEventListener("keydown", (e) => { if ((e.key === "Enter" || e.key === " ") && !e.repeat) { e.preventDefault(); down(); } });
    btn.addEventListener("keyup", (e) => { if (e.key === "Enter" || e.key === " ") up(); });
    btn.addEventListener("blur", up);
    return () => { cancelAnimationFrame(raf); layer.remove(); };
  }

  // ------------------------------------------------------------------ easter eggs that live at page level
  function konami(cb) {
    const seq = ["ArrowUp", "ArrowUp", "ArrowDown", "ArrowDown", "ArrowLeft", "ArrowRight", "ArrowLeft", "ArrowRight", "b", "a"];
    let i = 0;
    addEventListener("keydown", (e) => {
      const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      i = k === seq[i] ? i + 1 : k === seq[0] ? 1 : 0;
      if (i === seq.length) { i = 0; cb(); }
    });
  }
  function gemRain() {
    if (reduced) return;
    const kinds = ["diamond", "emerald", "gold", "redstone"];
    for (let i = 0; i < 36; i++) {
      const g = document.createElement("div");
      g.className = "gem";
      g.style.left = Math.random() * 100 + "vw";
      g.style.setProperty("--rot", (Math.random() * 180 - 90) + "deg");
      g.style.animationDuration = 1.6 + Math.random() * 1.6 + "s";
      g.style.animationDelay = Math.random() * 1.2 + "s";
      g.innerHTML = sprite(kinds[i % 4], 3 + (i % 3));
      document.body.append(g);
      g.addEventListener("animationend", () => g.remove());
    }
  }
  function weather(on) {
    let w = $("#weather");
    if (!w) {
      w = document.createElement("div"); w.id = "weather";
      for (let i = 0; i < 90; i++) { const d = document.createElement("i"); d.style.left = Math.random() * 110 + "vw"; d.style.animationDuration = 0.55 + Math.random() * 0.5 + "s"; d.style.animationDelay = -Math.random() * 1.2 + "s"; w.append(d); }
      document.body.append(w);
    }
    w.classList.toggle("on", !!on);
  }
  function f3(getLines) {
    let el = $("#f3");
    if (!el) { el = document.createElement("div"); el.id = "f3"; document.body.append(el); }
    let frames = 0, fps = 60, last = performance.now();
    const loop = (t) => { frames++; if (t - last >= 500) { fps = Math.round((frames * 1000) / (t - last)); frames = 0; last = t; if (el.classList.contains("on")) el.innerHTML = getLines(fps).map((l) => `<div>${l}</div>`).join(""); } requestAnimationFrame(loop); };
    requestAnimationFrame(loop);
    addEventListener("keydown", (e) => { if (e.key === "F3") { e.preventDefault(); el.classList.toggle("on"); el.innerHTML = getLines(fps).map((l) => `<div>${l}</div>`).join(""); } });
  }

  const SPLASHES = ["Also try curl!", "Now with 100% more HTTP!", "Funnel your requests!", "Don't dig straight down!", "Hopper, not Hoppr!", "Works on my machine!", "Sssssend!", "Ticks not milliseconds!"];
  const TIPS = [
    "Hold Send on production to mine through the guard.",
    "Alt+1 to Alt+9 sends the request in that hotbar slot.",
    "Click any value in a response to copy it.",
    "Ctrl+Alt+H summons Hopper from anywhere.",
    "Secrets stay in Rust. The panel only sees whether they are set.",
    "Press Esc to close the panel and give focus back to your editor.",
  ];
  const pick = (a) => a[Math.floor(Math.random() * a.length)];

  window.HM = { $, sleep, esc, reduced, sprite, MATERIAL, METHOD_COLOR, envs, requests, specRoutes, fakeSend, statusClass, ticks, stacks, size, resolve, jsonHtml, enchant, toast, orbs, mine, konami, gemRain, weather, f3, SPLASHES, TIPS, pick, STATUS_TEXT };
})();
