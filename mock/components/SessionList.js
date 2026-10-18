(() => {
  // apps/shell/src/client/context.ts
  var REFERENCE = "$ctx.";
  var Context = class {
    values = /* @__PURE__ */ new Map();
    listeners = /* @__PURE__ */ new Set();
    /** Sets one value and tells everyone who follows the context; nothing happens when it did not change. */
    set(name, value) {
      if (this.values.get(name) === value) return;
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
  function createShell(fetcher = (input2, init) => fetch(input2, init)) {
    return {
      capability: async (id, sha256, variables = {}) => await call(fetcher, "POST", `/capabilities/${encodeURIComponent(id)}`, { sha256, variables }),
      command: (body) => call(fetcher, "POST", "/commands", body),
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
      context: new Context()
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
      part("session-create-notice").innerHTML = notice(next);
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

  // mock/src/commands.js
  function apiType(settings, url) {
    let host2 = "";
    try {
      const parsed = new URL(url);
      if (parsed.protocol === "http:" || parsed.protocol === "https:") host2 = parsed.hostname.toLowerCase();
    } catch {
    }
    const home = settings.providers.opencode.url;
    return host2 === "api.anthropic.com" ? "cloud" : url !== "" && url === home ? "opencode" : null;
  }
  var COMMANDS = {
    // session.createSession (planned, #157): the next session of the campaign, at
    // the position after the last, in preparation, with the PCs ticked present.
    // `present-only-pj` refuses a character that is not a PC of the fixtures.
    "session.createSession": async (server, { payload: payload2 }) => {
      const sessions = await server.load("session.listSessions");
      const characters = await server.load("campaign.listCharacters");
      const present = Array.isArray(payload2.present) ? payload2.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
      sessions.push({
        id: server.uuid(),
        order: sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1,
        title: String(payload2.title || ""),
        date: payload2.plannedOn || null,
        status: "prep",
        label: "En pr\xE9paration",
        present,
        summary: null
      });
      return void 0;
    },
    // ai.saveSettings (crates/ai/data-capabilities/save-settings): the DM's
    // provider saved and made active. The API type is derived from the URL,
    // never taken from the payload: the cloud host by itself, the home server
    // at the URL whose model list the fixtures know; any other URL is refused
    // by `api-type-recognized` (« type d'API non reconnu »). `binary: claude`
    // is stored « claude-code ». The key is kept masked, never echoed.
    "ai.saveSettings": async (server, { payload: payload2 }) => {
      const settings = await server.load("ai.getSettings");
      const models = { generationModel: payload2.generationModel, summaryModel: payload2.summaryModel };
      if (!String(models.generationModel || "") || !String(models.summaryModel || "")) return ["single-settings-row"];
      if (payload2.mode === "api") {
        const url = String(payload2.url || "");
        const type = apiType(settings, url);
        if (!type) return ["api-type-recognized"];
        const key = String(payload2.key || "");
        settings.providers[type] = { ...settings.providers[type], url, ...models, key: key ? `\u2022\u2022\u2022\u2022${key.slice(-4)}` : "" };
        Object.assign(settings, { mode: "api", provider: type, binary: null });
        return void 0;
      }
      if (payload2.mode === "binary") {
        const binary = { claude: "claude-code", "claude-code": "claude-code", opencode: "opencode" }[payload2.binary];
        if (!binary) return ["mode-fields"];
        Object.assign(settings, { mode: "binary", provider: binary, binary: { binary, ...models } });
        return void 0;
      }
      return ["mode-fields"];
    },
    // ai.testConnection (crates/ai/data-capabilities/test-connection): the
    // configuration on screen tested and nothing of it saved — only `lastTest`
    // changes (`test-does-not-save`). Its type is detected as a save's; a URL
    // of no known type is the active provider's failed test, « type d'API non
    // reconnu ». The key is never kept.
    "ai.testConnection": async (server, { payload: payload2 }) => {
      const settings = await server.load("ai.getSettings");
      const at = (/* @__PURE__ */ new Date()).toISOString();
      const provider = payload2.mode === "api" ? apiType(settings, String(payload2.url || "")) : payload2.mode === "binary" ? { claude: "claude-code", "claude-code": "claude-code", opencode: "opencode" }[payload2.binary] || null : null;
      if (!provider) {
        settings.lastTest = { ok: false, at, provider: settings.provider, latency: null, message: "type d'API non reconnu" };
        return void 0;
      }
      const message = payload2.mode === "api" && settings.testResults?.[provider]?.message || `Connexion r\xE9ussie (binaire ${provider})`;
      const latency = message.match(/répond en ([\d,]+ s)/)?.[1] || "0,01 s";
      settings.lastTest = { ok: true, at, provider, latency, message };
      return void 0;
    }
  };

  // mock/src/reads.js
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e", played: "Jou\xE9e" };
  var READS = {
    // One session's header and its present PCs: a session of `session.listSessions`.
    "session.getSession": async (server, { id }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === id);
      if (!session) return void 0;
      const { summary: _summary, ...header } = session;
      return { ...header, statusLabel: STATUS_LABELS[session.status] ?? null };
    },
    // A session's scenes by rank: the fixture is the scenes of session 4, the one
    // in preparation; another session has none yet.
    "session.listScenes": async (server, { session }) => session === "s4" ? await server.load("session.listScenes") : []
  };

  // mock/src/server.js
  var LATENCY_MS = 120;
  var delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  var uuid = () => crypto.randomUUID ? crypto.randomUUID() : "xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx".replace(/x/g, () => Math.floor(Math.random() * 16).toString(16));
  var json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  var notFound = () => json({ error: "not-found" }, 404);
  var refused = (why) => json({ error: why }, 409);
  var same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  var FINAL2 = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
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
  function mockServer() {
    if (window.__mockServer) return window.__mockServer;
    const s = { dataVersion: 1, fixtures: /* @__PURE__ */ new Map(), listeners: /* @__PURE__ */ new Set(), commands: /* @__PURE__ */ new Map(), queue: /* @__PURE__ */ new Map(), messages: [], tick: 1e3, parkAfterMs: 3e4 };
    s.load = async (name) => {
      if (!s.fixtures.has(name)) {
        s.fixtures.set(name, fetch(new URL(`../fixtures/${name}.json`, location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0));
      }
      return s.fixtures.get(name);
    };
    s.bump = () => {
      s.dataVersion += 1;
      for (const listener of s.listeners) listener({ data: JSON.stringify({ dataVersion: s.dataVersion }) });
    };
    s.uuid = uuid;
    s.next = () => s.tick++;
    s.planned = async (id) => {
      if (!s.graph) s.graph = fetch(new URL("../../plan/graph.json", location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0);
      const graph = await s.graph;
      const entry = graph && graph.ids && graph.ids[id];
      return Boolean(entry && entry.kind === "data-capability" && entry.producer != null);
    };
    s.tell = (message, to, commandId2, extra = {}) => {
      const m = { message, to: Array.isArray(to) ? to : [to], commandId: commandId2, at: (/* @__PURE__ */ new Date()).toISOString(), ...extra };
      s.messages.push(m);
      return m;
    };
    const read = async (identifier, variables) => {
      const handler = READS[identifier];
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
      const manifest = await s.load(`aggregates/${aggregate}`);
      for (const inv of manifest && manifest.invariants || []) {
        if (!ruleHolds(inv.rule, payload2)) (inv.onViolation === "review" ? out.review : out.reject).push(inv.id);
      }
      return out;
    };
    const pendingIn = (partition) => [...s.queue.values()].filter((e) => e.partition === partition && !FINAL2.has(e.state)).sort((a, b) => a.position - b.position);
    const declaredAhead = (entry) => {
      const fields = Object.keys(entry.payload);
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
    const answer = (entry, replayed = false) => json({ commandId: entry.commandId, partition: entry.partition, replayed, warnings: entry.warnings || [], result: resultOf(entry) }, 202);
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
      const handler = COMMANDS[dataCapability];
      if (body.idempotencyKey) {
        const seen = [...s.queue.values()].find((e) => e.idempotencyKey === body.idempotencyKey && e.dataCapability === dataCapability);
        if (seen) return answer(seen, true);
      }
      const commandId2 = uuid();
      const aggregate = target.aggregate || null;
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload: payload2, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      if (!handler && await s.planned(dataCapability)) {
        entry.state = "applied";
        entry.warnings.push(`mocked-data-capability: ${dataCapability} is planned, not built`);
        s.tell("CommandApplied", by, commandId2, { mocked: true });
        s.bump();
        return answer(entry);
      }
      if (!handler) {
        entry.state = "rejected";
        entry.violations = ["unhandled-data-capability"];
        s.tell("CommandRejected", by, commandId2, { violations: entry.violations, reason: `${dataCapability} is not in mock/src/commands.js` });
        return answer(entry);
      }
      const verdict = await invariants(aggregate, payload2);
      if (verdict.reject.length || verdict.review.length) {
        entry.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId2, { violations: verdict.reject.concat(verdict.review) });
      }
      const critical = (await s.load(`aggregates/${aggregate}`) || { fields: {} }).fields || {};
      const needsReview = verdict.review.length > 0 || Object.keys(payload2).some((f) => critical[f] && critical[f].criticality === "critical");
      const ahead = declaredAhead(entry);
      if (needsReview) {
        entry.state = "awaiting_review";
        entry.reviewId = `r_${s.next()}`;
        entry.warnings.push(`review ${entry.reviewId} required`);
        s.tell("ReviewRequired", "role:approver", commandId2, { actions: ["approve", "reject"] });
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
      return json({ result: resultOf(entry) });
    };
    s.fetch = async (input2, init) => {
      await delay(LATENCY_MS);
      const url = new URL(typeof input2 === "string" ? input2 : input2 instanceof URL ? input2.href : input2.url);
      const method = init && init.method || "GET";
      const body = init && typeof init.body === "string" ? JSON.parse(init.body) : {};
      if (method === "POST" && url.pathname.startsWith("/capabilities/")) {
        const data = await read(decodeURIComponent(url.pathname.slice("/capabilities/".length)), body.variables || {});
        return data === void 0 ? notFound() : json({ asOf: s.dataVersion, data });
      }
      if (method === "POST" && url.pathname === "/commands") return command(body);
      const lookup = /^\/commands\/([^/]+)$/.exec(url.pathname);
      if (lookup && method === "GET") {
        const entry = s.commands.get(decodeURIComponent(lookup[1]));
        return entry ? json({ result: resultOf(entry) }) : notFound();
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
