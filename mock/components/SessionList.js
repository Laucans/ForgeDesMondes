(() => {
  // apps/shell/src/client/context.ts
  var REFERENCE = "$ctx.";
  var Context = class {
    values = /* @__PURE__ */ new Map();
    listeners = /* @__PURE__ */ new Set();
    held = /* @__PURE__ */ new Set();
    /** Sets one value the address named and keeps it: a later `set` of that name changes nothing. */
    hold(name, value) {
      this.set(name, value);
      this.held.add(name);
    }
    /** Sets one value and tells everyone who follows the context; nothing happens when it did not change, or when the address holds the name. */
    set(name, value) {
      if (this.held.has(name) || this.values.get(name) === value) return;
      this.values.set(name, value);
      for (const listener of [...this.listeners]) {
        try {
          listener(name, value);
        } catch (error) {
          console.error(`shell: a follower of the context failed on "${name}"`, error);
        }
      }
    }
    /** One value; `undefined` while nothing set it. */
    get(name) {
      return this.values.get(name);
    }
    /** Listens to the context; answers how to stop. */
    on(listener) {
      this.listeners.add(listener);
      return () => this.listeners.delete(listener);
    }
    /** The raw props with every `$ctx.<name>` resolved; `undefined` while the context has no value. */
    resolve(raw) {
      const out = {};
      for (const [key, value] of Object.entries(raw)) {
        out[key] = typeof value === "string" && value.startsWith(REFERENCE) ? this.get(value.slice(REFERENCE.length)) : value;
      }
      return out;
    }
    /** The context names the raw props follow. */
    follows(raw) {
      return Object.values(raw).filter((value) => typeof value === "string" && value.startsWith(REFERENCE)).map((value) => value.slice(REFERENCE.length));
    }
  };

  // apps/shell/src/client/shell.ts
  var ProtocolError = class extends Error {
    constructor(status, body) {
      super(`the server answered ${status}`);
      this.status = status;
      this.body = body;
    }
  };
  async function call(fetcher, method, path, body) {
    const init = { method };
    if (body !== void 0) {
      init.headers = { "content-type": "application/json" };
      init.body = JSON.stringify(body);
    }
    const answer = await fetcher(path, init);
    const text2 = await answer.text();
    let parsed = text2;
    try {
      parsed = text2 ? JSON.parse(text2) : null;
    } catch {
    }
    if (!answer.ok) throw new ProtocolError(answer.status, parsed);
    return parsed;
  }
  var POLL_MS = 2e3;
  var FAIL_CLOSED = { by: "player", roles: [], permissions: [] };
  var browserPage = () => ({
    href: typeof location === "undefined" ? "" : location.href,
    meta: (name) => typeof document === "undefined" ? void 0 : document.querySelector(`meta[name="${name}"]`)?.getAttribute("content") ?? void 0,
    fetch: (input2, init) => fetch(input2, init)
  });
  var asStrings = (value) => Array.isArray(value) && value.every((item2) => typeof item2 === "string") ? value : void 0;
  function asCaller(value) {
    const caller = value && typeof value === "object" ? value : {};
    const roles = asStrings(caller.roles);
    const permissions = caller.permissions === void 0 ? [] : asStrings(caller.permissions);
    return typeof caller.by === "string" && caller.by && roles && permissions ? { by: caller.by, roles, permissions } : void 0;
  }
  async function json(page, url) {
    const answer = await page.fetch(url);
    if (!answer.ok) throw new Error(`${url}: ${answer.status}`);
    return answer.json();
  }
  async function product(page, path) {
    try {
      return await json(page, new URL(`../__product/${path}`, page.href).href);
    } catch {
      return json(page, new URL(`../../${path}`, page.href).href);
    }
  }
  async function mockupCaller(page) {
    const here = new URL(page.href);
    const pagesUrl = new URL("../pages.json", here);
    const listed = await json(page, pagesUrl.href);
    const mocked = listed.pages?.find((p) => typeof p.path === "string" && new URL(p.path, pagesUrl).pathname === here.pathname);
    if (!mocked) return FAIL_CLOSED;
    let surface = mocked.surface;
    if (mocked.screen !== void 0) {
      const screen = await product(page, `apps/screens/${mocked.screen}.json`);
      const pilot = screen.pilot;
      surface = pilot && typeof pilot.query === "string" && here.searchParams.get(pilot.query) === "1" ? pilot.surface : screen.surface;
    }
    const declared = (await product(page, "surfaces.json")).surfaces;
    return (surface !== void 0 && declared && Object.hasOwn(declared, surface) ? asCaller(declared[surface]) : void 0) ?? FAIL_CLOSED;
  }
  async function pageCaller(page) {
    try {
      const meta = page.meta("shell-caller");
      if (meta !== void 0) return { caller: asCaller(JSON.parse(meta)) ?? FAIL_CLOSED, settled: true };
      return { caller: await mockupCaller(page), settled: true };
    } catch {
      return { caller: FAIL_CLOSED, settled: false };
    }
  }
  function withCaller(body, caller) {
    if (!body || typeof body !== "object" || Array.isArray(body)) return body;
    const sent = body.caller;
    return { ...body, caller: { ...sent && typeof sent === "object" ? sent : {}, by: caller.by, roles: caller.roles, permissions: caller.permissions } };
  }
  function createShell(fetcher = (input2, init) => fetch(input2, init), page = browserPage()) {
    let resolving;
    const callerOf = async () => {
      const asked = resolving ??= pageCaller(page);
      const { caller, settled: settled2 } = await asked;
      if (!settled2 && resolving === asked) resolving = void 0;
      return caller;
    };
    const context2 = new Context();
    const named = page.href ? new URL(page.href, "http://shell").searchParams.get("campaign") : null;
    if (named) context2.hold("campaign", named);
    else {
      const fallback = page.meta("shell-default-campaign");
      if (fallback) context2.set("campaign", fallback);
    }
    return {
      capability: async (id, sha256, variables = {}) => await call(fetcher, "POST", `/capabilities/${encodeURIComponent(id)}`, { sha256, variables }),
      command: async (body) => call(fetcher, "POST", "/commands", withCaller(body, await callerOf())),
      commandResult: (commandId2) => call(fetcher, "GET", `/commands/${encodeURIComponent(commandId2)}`),
      onDataVersion(listener, everyMs = POLL_MS) {
        let last;
        let inFlight = false;
        const poll = async () => {
          if (inFlight) return;
          inFlight = true;
          try {
            const { dataVersion } = await call(fetcher, "GET", "/dataVersion");
            if (dataVersion !== last) {
              last = dataVersion;
              listener(dataVersion);
            }
          } catch {
          } finally {
            inFlight = false;
          }
        };
        void poll();
        const timer = setInterval(() => void poll(), everyMs);
        return () => clearInterval(timer);
      },
      context: context2
    };
  }

  // apps/session/session-list/src/ids.ts
  var LIST_SESSIONS = "session.listSessions";
  var LIST_CHARACTERS = "campaign.listCharacters";
  var CREATE_SESSION = "session.createSession";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/session/session-list/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var optionalText = (value) => typeof value === "string" ? value : null;
  var texts = (value) => Array.isArray(value) ? value.filter((item2) => typeof item2 === "string") : [];
  function parseSessions(data) {
    const sessions = [];
    for (const entry of Array.isArray(data) ? data : []) {
      if (!isRecord(entry) || text(entry.id) === "") continue;
      sessions.push({
        id: text(entry.id),
        order: typeof entry.order === "number" ? entry.order : 0,
        title: text(entry.title),
        date: optionalText(entry.date),
        status: text(entry.status),
        label: optionalText(entry.label),
        present: texts(entry.present),
        summary: optionalText(entry.summary)
      });
    }
    return sessions.sort((a, b) => a.order - b.order);
  }
  function parseCharacters(data) {
    const characters = [];
    for (const entry of Array.isArray(data) ? data : []) {
      if (isRecord(entry) && text(entry.id) !== "") characters.push({ id: text(entry.id), name: text(entry.name), kind: text(entry.kind) });
    }
    return characters;
  }
  var playerCharacters = (characters) => characters.filter((c) => c.kind === "pj");
  var firstName = (characters, id) => (characters.find((c) => c.id === id)?.name ?? id).split(" ")[0] ?? id;
  var isNext = (session) => session.status === "prep" || session.status === "live";
  function statusLabel(session) {
    if (session.label) return session.label;
    return session.status === "prep" ? "En pr\xE9paration" : session.status === "live" ? "En direct" : "Jou\xE9e";
  }
  var statusTone = (session) => session.status === "prep" ? "accent" : session.status === "live" ? "primary" : "success";
  function dateLabel(date) {
    if (date === null || date === "") return "";
    const parsed = /* @__PURE__ */ new Date(`${date.slice(0, 10)}T12:00:00`);
    return Number.isNaN(parsed.getTime()) ? date : parsed.toLocaleDateString("fr-FR", { weekday: "short", day: "numeric", month: "long" });
  }
  var nextOrder = (sessions) => sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1;
  function initialForm(sessions, characters) {
    return { title: `S\xE9ance ${nextOrder(sessions)}`, date: "", present: playerCharacters(characters).map((c) => c.id) };
  }
  function payload(campaign, form, sessions) {
    const title = form.title.trim() || `S\xE9ance ${nextOrder(sessions)}`;
    return { campaign, title, ...form.date === "" ? {} : { plannedOn: form.date }, present: form.present };
  }
  function newKey() {
    const crypto2 = globalThis.crypto;
    return typeof crypto2?.randomUUID === "function" ? crypto2.randomUUID() : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  }
  function commandBody(dataCapability, body, idempotencyKey) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: body, caller: { by: "dm", from: "SessionList", roles: [] }, idempotencyKey };
  }
  var VIOLATIONS = {
    "present-only-pj": "seuls les PJ de la campagne peuvent \xEAtre pr\xE9sents",
    "oneshot-single-session": "un one-shot n\u2019a qu\u2019une seule s\xE9ance"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    return { status: text(result.status), violations: texts(result.violations) };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/session/session-list/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var attr = (name, value) => value === "" ? "" : ` ${name}="${esc(value)}"`;
  function skeleton() {
    return `<section class="panel mf-sessions" id="session-list" aria-labelledby="sessions-title"><div class="panel__head"><h2 id="sessions-title">S\xE9ances</h2><div class="kit-button kit-button--bare kit-button--small"><button type="button" class="primary" id="sessions-new" data-writes="${CREATE_SESSION}" disabled>Nouvelle s\xE9ance</button></div></div><ol class="mf-sessions__list" id="sessions-items" data-reads="${LIST_SESSIONS}" aria-live="polite"></ol><div id="sessions-dialog"></div></section>`;
  }
  function kitButtons(buttons) {
    return '<div class="kit-button kit-button--bare kit-button--small">' + buttons.map((b) => `<button type="button" class="${b.variant}"${attr("data-href", b.href)}>${esc(b.label)}</button>`).join("") + "</div>";
  }
  function item(session, characters) {
    const next = isNext(session);
    const date = dateLabel(session.date);
    const names = session.present.map((id) => firstName(characters, id)).join(", ") || "\u2014";
    return `<li class="mf-sessions__item${next ? " is-next" : ""}" id="session-${esc(session.id)}"><span class="mf-sessions__num">${session.order}</span><div class="mf-sessions__main"><div class="panel__head"><h3>${esc(session.title)}</h3><div class="kit-badge"><span class="${statusTone(session)}">${esc(statusLabel(session))}</span>${date === "" ? "" : `<span>${esc(date)}</span>`}</div></div>` + (session.summary ? `<p class="mf-sessions__summary">${esc(session.summary)}</p>` : "") + `<p class="small muted" data-reads="${LIST_CHARACTERS}">PJ pr\xE9sents : ${esc(names)}</p>` + kitButtons(
      session.status === "live" ? [{ label: "Reprendre en direct", href: "dm-view.html", variant: "primary" }] : next ? [
        { label: "Pr\xE9parer", href: "preparation-seance.html", variant: "secondary" },
        { label: "Lancer en direct", href: "dm-view.html", variant: "primary" }
      ] : [{ label: "R\xE9sum\xE9 et journal", href: "preparation-seance.html", variant: "ghost" }]
    ) + "</div></li>";
  }
  function items(sessions, characters, reading) {
    if (reading === "loading") return '<li class="mf-sessions__empty muted">Chargement des s\xE9ances\u2026</li>';
    if (reading === "failed") return '<li class="mf-sessions__empty is-error">Impossible de lire les s\xE9ances.</li>';
    if (sessions.length === 0) return '<li class="mf-sessions__empty muted">Aucune s\xE9ance pour l\u2019instant.</li>';
    return sessions.map((session) => item(session, characters)).join("");
  }
  function input(p) {
    return `<div class="kit-input${p.wide ? " is-wide" : ""}"><label>${esc(p.label)}<input type="${p.type}" id="${p.id}"${attr("value", p.value)}></label></div>`;
  }
  function presentChoice(characters, form) {
    const options = playerCharacters(characters).map((c) => `<label><input type="checkbox" name="present" value="${esc(c.id)}"${form.present.includes(c.id) ? " checked" : ""}><span>${esc(c.name)}</span></label>`).join("");
    return `<div class="kit-checkbox"><fieldset id="session-create-present" data-reads="${LIST_CHARACTERS}"><legend>PJ pr\xE9sents</legend>${options}</fieldset></div>`;
  }
  function notice(state) {
    switch (state.kind) {
      case "none":
        return "";
      case "busy":
        return '<p class="muted">Cr\xE9ation en cours\u2026</p>';
      case "error":
        return `<p class="is-error" id="session-create-message">${esc(state.text)}</p>`;
    }
  }
  function dialog(form, characters) {
    return '<div class="mf-modal" id="session-create-dialog"><div class="kit-dialog"><div class="kit-dialog__veil"><div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="Nouvelle s\xE9ance"><header class="kit-dialog__header"><h2 class="kit-dialog__title">Nouvelle s\xE9ance</h2><button class="kit-dialog__close" type="button" aria-label="Fermer">\xD7</button></header><div class="kit-dialog__body">' + input({ id: "session-create-title", label: "Titre", type: "text", value: form.title, wide: true }) + input({ id: "session-create-date", label: "Date", type: "date", value: form.date, wide: false }) + presentChoice(characters, form) + `<p class="small muted">Vous pourrez retirer ou ajouter des PJ dans la pr\xE9paration.</p><div id="session-create-notice" aria-live="polite"></div></div><footer class="kit-dialog__footer"><button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" id="session-create-cancel">Annuler</button><button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="session-create-submit" data-writes="${CREATE_SESSION}">Cr\xE9er la s\xE9ance</button></footer></div></div></div></div>`;
  }

  // apps/session/session-list/src/style.ts
  var STYLE = `
.mf-sessions { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-sessions *, .mf-sessions *::before, .mf-sessions *::after { box-sizing: inherit; }
.mf-sessions h2, .mf-sessions h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-sessions h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sessions h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-sessions p { margin: 0; }
.mf-sessions.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-sessions .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-sessions .muted { color: var(--color-text-muted); }
.mf-sessions .small { font-size: var(--font-size-small); }
.mf-sessions .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-sessions .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-sessions .kit-input.is-wide { max-width: none; }
.mf-sessions__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-3); }
.mf-sessions__item { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-sessions__item.is-next { border-color: var(--color-primary); box-shadow: var(--shadow-high); }
.mf-sessions__num { display: grid; place-items: center; width: calc(var(--space-5) + var(--space-1)); height: calc(var(--space-5) + var(--space-1)); border-radius: var(--radius-pill); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); font-family: var(--font-family-heading); font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sessions__main { display: grid; gap: var(--space-2); min-width: 0; }
.mf-sessions__summary { color: var(--color-text-muted); }
.mf-sessions .is-error { color: var(--color-danger); }
.mf-sessions .mf-modal { position: fixed; inset: 0; z-index: 100; }
.mf-sessions .mf-modal .kit-dialog, .mf-sessions .mf-modal .kit-dialog__veil { height: 100%; }
.mf-sessions .mf-modal .kit-dialog__window { max-height: 88vh; }
.mf-sessions .kit-dialog__body { display: grid; gap: var(--space-3); align-content: start; }
`;

  // apps/session/session-list/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="SessionList"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "SessionList";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, props2, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`SessionList: #${id} is not in the list`);
      return found;
    };
    const campaignOf = (raw) => typeof raw.campaign === "string" ? raw.campaign : "";
    let campaign = campaignOf(props2);
    let sessions = [];
    let characters = [];
    let reading = "loading";
    let charactersRead = false;
    let form = initialForm(sessions, characters);
    let opened = false;
    let busy = false;
    let loading = 0;
    let key = "";
    const paintList = () => {
      part("sessions-items").innerHTML = items(sessions, characters, reading);
      const opener = part("sessions-new");
      opener.disabled = reading !== "ready" || !charactersRead;
      opener.title = reading === "ready" && !charactersRead ? "Impossible de lire les PJ de la campagne." : "";
    };
    async function load() {
      const mine = loading += 1;
      const variables = { campaign };
      const [read, people] = await Promise.allSettled([shell2.capability(LIST_SESSIONS, void 0, variables), shell2.capability(LIST_CHARACTERS, void 0, variables)]);
      if (mine !== loading) return;
      if (read.status === "fulfilled") {
        sessions = parseSessions(read.value.data);
        reading = "ready";
      } else if (reading !== "ready") reading = "failed";
      if (people.status === "fulfilled") {
        characters = parseCharacters(people.value.data);
        charactersRead = true;
      }
      paintList();
    }
    const say = (next) => {
      const target = host2.querySelector("#session-create-notice");
      if (target) target.innerHTML = notice(next);
    };
    const lock = (locked) => {
      for (const control of host2.querySelectorAll("#session-create-dialog input, #session-create-dialog button")) control.disabled = locked;
    };
    function open() {
      if (opened) return;
      opened = true;
      form = initialForm(sessions, characters);
      key = newKey();
      part("sessions-dialog").innerHTML = dialog(form, characters);
      host2.querySelector("#session-create-title")?.focus();
    }
    function close() {
      if (!opened || busy) return;
      opened = false;
      part("sessions-dialog").innerHTML = "";
    }
    async function send() {
      const answer = await shell2.command(commandBody(CREATE_SESSION, payload(campaign, form, sessions), key));
      const id = commandId(answer);
      let result = settled(answer);
      for (let asked = 0; !isFinal(result.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        result = settled(await shell2.commandResult(id));
      }
      return result;
    }
    async function create() {
      if (busy || !opened) return;
      busy = true;
      lock(true);
      say({ kind: "busy" });
      let created = false;
      try {
        const result = await send();
        if (result.status === "applied") created = true;
        else if (result.status === "rejected") {
          key = newKey();
          say({ kind: "error", text: `Cr\xE9ation refus\xE9e : ${reasons(result.violations)}.` });
        } else say({ kind: "error", text: `La cr\xE9ation n\u2019a pas abouti (\xE9tat : ${result.status || "inconnu"}).` });
      } catch {
        say({ kind: "error", text: "La cr\xE9ation a \xE9chou\xE9." });
      } finally {
        busy = false;
        lock(false);
      }
      if (!created) return;
      close();
      await load();
    }
    host2.addEventListener("submit", (event) => event.preventDefault());
    host2.addEventListener("keydown", (event) => {
      if (event.key === "Escape") close();
    });
    host2.addEventListener("input", (event) => {
      const target = event.target;
      if (target?.id === "session-create-title") form = { ...form, title: target.value ?? "" };
      else if (target?.id === "session-create-date") form = { ...form, date: target.value ?? "" };
      else return;
      key = newKey();
    });
    host2.addEventListener("change", (event) => {
      const target = event.target;
      if (target?.name !== "present") return;
      const id = target.value ?? "";
      form = { ...form, present: target.checked ? [...form.present.filter((p) => p !== id), id] : form.present.filter((p) => p !== id) };
      key = newKey();
    });
    host2.addEventListener("click", (event) => {
      const target = event.target;
      if (target?.classList.contains("kit-dialog__veil")) return close();
      const button = target?.closest("button");
      if (!button) return;
      if (button.id === "sessions-new") open();
      else if (button.dataset.writes === CREATE_SESSION) void create();
      else if (button.id === "session-create-cancel" || button.classList.contains("kit-dialog__close")) close();
      else if (button.dataset.href) host2.ownerDocument.defaultView?.location.assign(button.dataset.href);
    });
    paintList();
    void load();
    return {
      update(next) {
        const following = campaignOf(next);
        if (following === campaign) return;
        campaign = following;
        busy = false;
        close();
        sessions = [];
        characters = [];
        reading = "loading";
        charactersRead = false;
        paintList();
        void load();
      }
    };
  }

  // mock/src/screen.js
  var context = () => window.__mockContext = window.__mockContext || {};
  function getContext(name) {
    return context()[name];
  }
  function onContext(listener) {
    const handler = (e) => listener(e.detail.name, e.detail.value);
    document.addEventListener("mock:context", handler);
    return () => document.removeEventListener("mock:context", handler);
  }
  function mountPoint() {
    const script = document.currentScript;
    const component = script && script.closest("[data-component]");
    const host2 = component ? component.querySelector(".mf-root") || component : document.body;
    let props2;
    try {
      props2 = JSON.parse(component && component.dataset.props || "{}");
    } catch {
      props2 = {};
    }
    return { component, host: host2, props: props2 };
  }
  function resolveProps(raw) {
    const out = {};
    for (const [key, value] of Object.entries(raw || {})) {
      out[key] = typeof value === "string" && value.startsWith("$ctx.") ? getContext(value.slice(5)) : value;
    }
    return out;
  }
  function compose(raw, mount2) {
    const follows = Object.values(raw || {}).filter((v) => typeof v === "string" && v.startsWith("$ctx.")).map((v) => v.slice(5));
    const mounted = mount2(resolveProps(raw));
    if (follows.length) onContext((name) => {
      if (follows.includes(name) && mounted && mounted.update) mounted.update(resolveProps(raw));
    });
    return mounted;
  }

  // mock/src/server.js
  var LATENCY_MS = 120;
  var delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  var uuid = () => crypto.randomUUID ? crypto.randomUUID() : "xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx".replace(/x/g, () => Math.floor(Math.random() * 16).toString(16));
  var json2 = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  var notFound = () => json2({ error: "not-found" }, 404);
  var refused = (why) => json2({ error: why }, 409);
  var same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  var FINAL2 = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  var LEGACY = { reads: ["../src/reads.js", "READS"], commands: ["../src/commands.js", "COMMANDS"] };
  var HandlerError = class extends Error {
  };
  var shown = (path) => `mock/${path.replace(/^\.\.\//, "")}`;
  async function moduleAt(path) {
    const url = new URL(path, location.href).href;
    const served = await fetch(url).then((r) => r.ok, () => false);
    if (!served) return null;
    try {
      return await import(url);
    } catch (e) {
      throw new HandlerError(`${shown(path)} does not load: ${e && e.message ? e.message : e}`);
    }
  }
  function ownFile(id, mod) {
    if (!mod) return null;
    const file = `mock/handlers/${id}.js`;
    const kinds = Object.keys(LEGACY).filter((kind) => mod[kind] !== void 0);
    if (!kinds.length) throw new HandlerError(`${file} exports neither reads nor commands`);
    for (const kind of kinds) {
      if (!mod[kind] || typeof mod[kind] !== "object") throw new HandlerError(`${file}: ${kind} is not an object`);
      for (const key of Object.keys(mod[kind])) {
        if (key !== id) throw new HandlerError(`${file} defines ${key}: a handler file defines its own id only \u2014 ${key} belongs in mock/handlers/${key}.js`);
      }
    }
    return mod;
  }
  function ruleHolds(rule, payload2) {
    const m = /^\s*([A-Za-z_][\w.]*)\s*(<=|>=|==|!=|<|>|=)\s*(.+?)\s*$/.exec(String(rule || ""));
    if (!m) return true;
    const [, field, op, raw] = m;
    if (!(field in payload2)) return true;
    const left = payload2[field];
    let right = raw;
    if (/^-?\d+(\.\d+)?$/.test(raw)) right = Number(raw);
    else if (raw === "true" || raw === "false") right = raw === "true";
    else right = raw.replace(/^['"]|['"]$/g, "");
    switch (op) {
      case "<=":
        return left <= right;
      case ">=":
        return left >= right;
      case "<":
        return left < right;
      case ">":
        return left > right;
      case "!=":
        return left !== right;
      default:
        return left === right;
    }
  }
  function undeclaredKeys(manifest, payload2) {
    const declared = Object.keys(manifest && manifest.payload || {});
    if (!declared.length) return [];
    const id = manifest.target && typeof manifest.target.id === "string" ? manifest.target.id.replace(/^\$+/, "") : null;
    const declares = (key) => declared.some((d) => d === key || d.startsWith(key) && d[key.length] === ".");
    return Object.keys(payload2 || {}).filter((key) => key !== id && !declares(key)).sort();
  }
  function fieldsOf(manifest, payload2) {
    const touches = Array.isArray(manifest && manifest.touches) ? manifest.touches.filter((t) => typeof t === "string") : [];
    return touches.length ? touches.map((t) => t.split(".").pop()) : Object.keys(payload2 || {});
  }
  function reviewNeeded(aggregate, fields, caller) {
    const declared = aggregate && aggregate.fields || {};
    const roles = caller && Array.isArray(caller.roles) ? caller.roles : [];
    return fields.filter((f) => Object.hasOwn(declared, f) && declared[f] && declared[f].criticality === "critical").map((f) => [f, declared[f].review || "role:approver"]).filter(([, role]) => !roles.includes(role)).map(([f, role]) => `${f} (${role})`);
  }
  var undeclaredWarning = (keys, dataCapability) => `payload key(s) ${keys.join(", ")} not declared by ${dataCapability}`;
  function mockServer() {
    if (window.__mockServer) return window.__mockServer;
    const s = { dataVersion: 1, fixtures: /* @__PURE__ */ new Map(), listeners: /* @__PURE__ */ new Set(), commands: /* @__PURE__ */ new Map(), queue: /* @__PURE__ */ new Map(), messages: [], tick: 1e3, parkAfterMs: 3e4 };
    s.load = async (name) => {
      if (!s.fixtures.has(name)) {
        s.fixtures.set(name, fetch(new URL(`../fixtures/${name}.json`, location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0));
      }
      return s.fixtures.get(name);
    };
    s.legacy = () => {
      if (!s.legacyHandlers) {
        s.legacyHandlers = Promise.all(Object.entries(LEGACY).map(async ([kind, [path, name]]) => [kind, (await moduleAt(path) || {})[name] || {}])).then(Object.fromEntries);
      }
      return s.legacyHandlers;
    };
    s.files = /* @__PURE__ */ new Map();
    s.handlerOf = async (kind, id) => {
      if (!s.files.has(id)) s.files.set(id, moduleAt(`../handlers/${encodeURIComponent(id)}.js`).then((mod2) => ownFile(id, mod2)));
      const [mod, legacy] = await Promise.all([s.files.get(id), s.legacy()]);
      const own = mod && mod[kind] && Object.hasOwn(mod[kind], id) ? mod[kind][id] : void 0;
      const kept = Object.hasOwn(legacy[kind], id) ? legacy[kind][id] : void 0;
      if (own && kept) throw new HandlerError(`${id} is defined twice: mock/handlers/${id}.js and ${shown(LEGACY[kind][0])} \u2014 keep one`);
      return own || kept;
    };
    s.problems = [];
    s.check = async () => {
      const said = /* @__PURE__ */ new Set();
      const fail = (e) => {
        const message = e && e.message ? e.message : String(e);
        if (said.has(message)) return;
        said.add(message);
        s.problems.push(message);
        console.error(message);
      };
      const ids = /* @__PURE__ */ new Set();
      try {
        const legacy = await s.legacy();
        for (const kind of Object.keys(LEGACY)) for (const id of Object.keys(legacy[kind])) ids.add(id);
      } catch (e) {
        fail(e);
        return;
      }
      const pages = await fetch(new URL("../pages.json", location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0);
      for (const component of pages && pages.components || []) {
        for (const id of [].concat(component.needs || [], component.actions || [])) if (typeof id === "string") ids.add(id);
      }
      for (const id of [...ids].sort()) {
        for (const kind of Object.keys(LEGACY)) await s.handlerOf(kind, id).catch(fail);
      }
    };
    s.ready = s.check();
    s.bump = () => {
      s.dataVersion += 1;
      for (const listener of s.listeners) listener({ data: JSON.stringify({ dataVersion: s.dataVersion }) });
    };
    s.uuid = uuid;
    s.next = () => s.tick++;
    s.productFiles = /* @__PURE__ */ new Map();
    s.product = (path) => {
      if (!s.productFiles.has(path)) {
        const at = (base) => fetch(new URL(base + path, location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0);
        s.productFiles.set(path, at("../__product/").then((found) => found === void 0 ? at("../../") : found));
      }
      return s.productFiles.get(path);
    };
    s.manifests = async () => {
      const index = await s.product("data-capabilities.json");
      if (index && !s.manifestsSaid) {
        s.manifestsSaid = true;
        for (const problem of index.problems || []) console.error(`mock server: ${problem}`);
      }
      return index && index.manifests || {};
    };
    s.aggregates = async () => {
      const index = await s.product("aggregates.json");
      if (index && !s.aggregatesSaid) {
        s.aggregatesSaid = true;
        for (const problem of index.problems || []) console.error(`mock server: ${problem}`);
      }
      return index && index.aggregates || {};
    };
    s.aggregate = async (name) => {
      if (!name) return void 0;
      const fixture = await s.load(`aggregates/${name}`);
      if (fixture !== void 0) return fixture;
      const known = (await s.aggregates())[name];
      return known ? known.manifest : void 0;
    };
    s.undeclared = [];
    s.planned = async (id) => {
      const graph = await s.product("plan/graph.json");
      const entry = graph && graph.ids && graph.ids[id];
      return Boolean(entry && entry.kind === "data-capability" && entry.producer != null);
    };
    s.tell = (message, to, commandId2, extra = {}) => {
      const m = { message, to: Array.isArray(to) ? to : [to], commandId: commandId2, at: (/* @__PURE__ */ new Date()).toISOString(), ...extra };
      s.messages.push(m);
      return m;
    };
    const read = async (identifier, variables) => {
      const handler = await s.handlerOf("reads", identifier);
      if (handler) return handler(s, variables);
      const fixture = await s.load(identifier);
      if (fixture === void 0) return void 0;
      if (fixture && typeof fixture === "object" && Array.isArray(fixture.$match)) {
        const hit = fixture.$match.find((m) => Object.entries(m.variables || {}).every(([k, v]) => same(variables[k], v)));
        return hit ? hit.data : fixture.data;
      }
      return fixture;
    };
    const invariants = async (aggregate, payload2) => {
      const out = { reject: [], review: [] };
      if (!aggregate) return out;
      const manifest = await s.aggregate(aggregate);
      for (const inv of manifest && manifest.invariants || []) {
        if (!ruleHolds(inv.rule, payload2)) (inv.onViolation === "review" ? out.review : out.reject).push(inv.id);
      }
      return out;
    };
    const pendingIn = (partition) => [...s.queue.values()].filter((e) => e.partition === partition && !FINAL2.has(e.state)).sort((a, b) => a.position - b.position);
    const declaredAhead = (entry, merging = /* @__PURE__ */ new Set()) => {
      const fields = Object.keys(entry.payload).filter((f) => !merging.has(f));
      const ahead = pendingIn(entry.partition).find((e) => e.position < entry.position && e.state !== "parked" && fields.some((f) => f in e.payload));
      if (ahead) {
        const f = fields.find((x) => x in ahead.payload);
        return { by: ahead.by, command: ahead.commandId, value: ahead.payload[f], field: f };
      }
      const read2 = entry.basedOn && entry.basedOn.values || {};
      for (const f of fields) {
        if (f in read2 && s.confirmed.has(`${entry.partition}.${f}`) && !same(s.confirmed.get(`${entry.partition}.${f}`), read2[f])) {
          return { by: "(applied)", command: "(applied)", value: s.confirmed.get(`${entry.partition}.${f}`), field: f };
        }
      }
      return null;
    };
    s.confirmed = /* @__PURE__ */ new Map();
    const resultOf = (entry) => ({ commandId: entry.commandId, status: entry.state, dataVersion: entry.dataVersion || null, violations: entry.violations || [], reviewId: entry.reviewId || null });
    const answer = (entry, replayed = false) => json2({ commandId: entry.commandId, partition: entry.partition, replayed, warnings: replayed ? [] : entry.warnings || [], result: resultOf(entry) }, 202);
    const drive = async () => {
      for (const partition of new Set([...s.queue.values()].filter((e) => !FINAL2.has(e.state)).map((e) => e.partition))) {
        for (; ; ) {
          const head = pendingIn(partition).find((e) => e.state !== "parked");
          if (!head) break;
          if (head.state === "awaiting_review") break;
          if (head.state === "awaiting_confirmation") {
            if (head.atHeadSince === void 0) head.atHeadSince = Date.now();
            if (Date.now() - head.atHeadSince < s.parkAfterMs) break;
            head.state = "parked";
            s.tell("Parked", head.by, head.commandId, { reason: "reached the head unconfirmed", actions: ["requeue", "cancel"] });
            continue;
          }
          const verdict = await invariants(head.aggregate, head.payload);
          if (verdict.reject.length) {
            head.state = "rejected";
            head.violations = verdict.reject;
            s.tell("CommandRejected", head.by, head.commandId, { violations: verdict.reject });
            continue;
          }
          if (head.handler) {
            const said = await head.handler(s, { payload: head.payload, targetId: head.targetId, version: head.version, basedOn: head.basedOn });
            const violations = Array.isArray(said) ? said : said && said.violations || [];
            if (violations.length) {
              head.state = "rejected";
              head.violations = violations;
              s.tell("CommandRejected", head.by, head.commandId, { violations });
              continue;
            }
          }
          head.state = "applied";
          s.bump();
          head.dataVersion = s.dataVersion;
          for (const [f, v] of Object.entries(head.payload)) s.confirmed.set(`${partition}.${f}`, v);
          s.tell("CommandApplied", [head.by, "infrastructure"], head.commandId, { dataVersion: s.dataVersion });
          for (const behind of pendingIn(partition)) {
            if (behind.aheadOf === head.commandId) s.tell("AheadResolved", behind.by, behind.commandId, { reason: `${head.commandId} was resolved` });
          }
        }
      }
    };
    const command = async (body) => {
      const [dataCapability, version] = String(body.dataCapability || "").split("@");
      const payload2 = body.payload || {};
      const target = body.target || {};
      const targetId = target.id || body.targetId;
      const by = body.caller && body.caller.by || body.by || "anonymous";
      const handler = await s.handlerOf("commands", dataCapability);
      if (body.idempotencyKey) {
        const seen = [...s.queue.values()].find((e) => e.idempotencyKey === body.idempotencyKey && e.dataCapability === dataCapability);
        if (seen) return answer(seen, true);
      }
      const known = (await s.manifests())[dataCapability];
      const aggregate = known && known.manifest && known.manifest.target && known.manifest.target.aggregate || target.aggregate || null;
      const document2 = await s.aggregate(aggregate) || { fields: {} };
      const declared = document2.fields || {};
      const derived = Object.keys(payload2).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json2({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload: payload2, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      const keys = known ? undeclaredKeys(known.manifest, payload2) : [];
      const warnUndeclared = () => {
        if (!keys.length) return;
        const warning = undeclaredWarning(keys, dataCapability);
        entry.warnings.push(warning);
        s.undeclared.push({ commandId: commandId2, dataCapability, keys, manifest: known.path, warning });
        console.error(`mock server: ${warning} (${known.path}) \u2014 the real server accepts it with this warning and the journeys fail on it: send only what the manifest's payload declares`);
      };
      if (!handler && await s.planned(dataCapability)) {
        entry.state = "applied";
        entry.warnings.push(`mocked-data-capability: ${dataCapability} is planned, not built`);
        warnUndeclared();
        s.tell("CommandApplied", by, commandId2, { mocked: true });
        s.bump();
        return answer(entry);
      }
      if (!handler) {
        warnUndeclared();
        entry.state = "rejected";
        entry.violations = ["unhandled-data-capability"];
        s.tell("CommandRejected", by, commandId2, { violations: entry.violations, reason: `${dataCapability} has no handler: write mock/handlers/${dataCapability}.js` });
        return answer(entry);
      }
      const verdict = await invariants(aggregate, payload2);
      if (verdict.reject.length || verdict.review.length) {
        entry.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId2, { violations: verdict.reject.concat(verdict.review) });
      }
      warnUndeclared();
      const needed = reviewNeeded(document2, fieldsOf(known && known.manifest, payload2), body.caller);
      const needsReview = verdict.review.length > 0 || needed.length > 0;
      const merging = new Set(Object.keys(declared).filter((f) => declared[f].merge));
      const ahead = declaredAhead(entry, merging);
      if (needsReview) {
        entry.state = "awaiting_review";
        entry.reviewId = `r_${s.next()}`;
        entry.warnings.push(`review ${entry.reviewId} required`);
        const first = fieldsOf(known && known.manifest, payload2)[0];
        const approver = first && declared[first] && declared[first].review || "role:approver";
        s.tell("ReviewRequired", approver, commandId2, { actions: ["approve", "reject"], reason: needed.length ? `critical: ${needed.join(", ")}` : "an invariant wants a review" });
      } else if (ahead && (body.mode || "confirm_on_stale") === "confirm_on_stale") {
        entry.state = "awaiting_confirmation";
        entry.aheadOf = ahead.command;
        entry.warnings.push("a value is declared ahead: confirm or cancel");
        s.tell("ValueDeclaredAhead", by, commandId2, { field: `${partition}.${ahead.field}`, yourValue: payload2[ahead.field], declaredAhead: { value: ahead.value, by: ahead.by, command: ahead.command }, actions: ["confirm_overwrite", "cancel", "edit"] });
      }
      s.tell("CommandQueued", by, commandId2);
      await drive();
      return answer(entry);
    };
    const gesture = async (id, verb, body) => {
      const entry = s.queue.get(id);
      if (!entry) return notFound();
      const by = body && body.by || entry.by;
      if (by !== entry.by) return refused(`${id} is ${entry.by}'s to ${verb}`);
      if (verb === "confirm") {
        if (entry.state !== "awaiting_confirmation") return refused(`${id} is ${entry.state}, not awaiting a confirmation`);
        entry.state = "confirmed";
      } else if (verb === "cancel") {
        if (FINAL2.has(entry.state)) return refused(`${id} is ${entry.state}`);
        entry.state = "cancelled";
        s.tell("CommandCancelled", entry.by, id);
      } else if (verb === "requeue") {
        if (entry.state !== "parked") return refused(`${id} is ${entry.state}, not parked`);
        entry.state = "cancelled";
        const fresh = { ...entry, commandId: `${id}-r`, position: s.next(), state: "queued", warnings: [], requeuedFrom: id, aheadOf: void 0, atHeadSince: void 0 };
        s.queue.set(fresh.commandId, fresh);
        s.commands.set(fresh.commandId, fresh);
        const ahead = declaredAhead(fresh);
        if (ahead) {
          fresh.state = "awaiting_confirmation";
          fresh.aheadOf = ahead.command;
          fresh.warnings.push("a value is declared ahead: confirm or cancel");
          s.tell("ValueDeclaredAhead", fresh.by, fresh.commandId, { field: `${fresh.partition}.${ahead.field}`, yourValue: fresh.payload[ahead.field], declaredAhead: { value: ahead.value, by: ahead.by, command: ahead.command }, actions: ["confirm_overwrite", "cancel", "edit"] });
        }
        s.tell("CommandQueued", fresh.by, fresh.commandId);
        await drive();
        return answer(fresh);
      } else if (verb === "approve" || verb === "reject") {
        if (entry.state !== "awaiting_review") return refused(`${id} is ${entry.state}, not awaiting a review`);
        entry.state = verb === "approve" ? "confirmed" : "rejected";
        if (verb === "reject") entry.violations = [`review ${entry.reviewId} rejected`];
      } else return notFound();
      await drive();
      return json2({ result: resultOf(entry) });
    };
    s.fetch = async (input2, init) => {
      try {
        await s.ready;
        if (s.problems.length) return json2({ error: `the mock server did not start: ${s.problems.join("; ")}` }, 500);
        return await respond(input2, init);
      } catch (e) {
        if (!(e instanceof HandlerError)) throw e;
        console.error(e.message);
        return json2({ error: e.message }, 500);
      }
    };
    const respond = async (input2, init) => {
      await delay(LATENCY_MS);
      const url = new URL(typeof input2 === "string" ? input2 : input2 instanceof URL ? input2.href : input2.url);
      const method = init && init.method || "GET";
      const body = init && typeof init.body === "string" ? JSON.parse(init.body) : {};
      if (method === "POST" && url.pathname.startsWith("/capabilities/")) {
        const data = await read(decodeURIComponent(url.pathname.slice("/capabilities/".length)), body.variables || {});
        return data === void 0 ? notFound() : json2({ asOf: s.dataVersion, data });
      }
      if (method === "POST" && url.pathname === "/commands") return command(body);
      const lookup = /^\/commands\/([^/]+)$/.exec(url.pathname);
      if (lookup && method === "GET") {
        const entry = s.commands.get(decodeURIComponent(lookup[1]));
        return entry ? json2({ result: resultOf(entry) }) : notFound();
      }
      const move = /^\/commands\/([^/]+)\/(confirm|cancel|requeue|approve|reject)$/.exec(url.pathname);
      if (move && method === "POST") return gesture(decodeURIComponent(move[1]), move[2], body);
      return notFound();
    };
    s.drive = drive;
    s.eventSource = () => {
      const source = { readyState: 1, addEventListener(type, listener) {
        if (type === "dataVersion") s.listeners.add(listener);
      }, close() {
        source.readyState = 2;
      } };
      return source;
    };
    window.__mockServer = s;
    return s;
  }
  function shellOnce(create) {
    if (!window.__mockShell) window.__mockShell = create(mockServer());
    return window.__mockShell;
  }

  // mock/src/SessionList.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input2, init) => server.fetch(new URL(input2, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
