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
  function createShell(fetcher = (input, init) => fetch(input, init)) {
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
  var texts = (value) => Array.isArray(value) ? value.filter((item2) => typeof item2 === "string") : [];
  function parseSessions(data) {
    if (!Array.isArray(data)) return [];
    const sessions = data.filter(isRecord).map(
      (entry) => ({
        id: text(entry.id),
        order: typeof entry.order === "number" ? entry.order : 0,
        title: text(entry.title),
        date: typeof entry.date === "string" && entry.date !== "" ? entry.date : null,
        status: text(entry.status),
        label: typeof entry.label === "string" && entry.label !== "" ? entry.label : null,
        present: texts(entry.present),
        summary: typeof entry.summary === "string" && entry.summary !== "" ? entry.summary : null
      })
    );
    return sessions.sort((a, b) => a.order - b.order);
  }
  function parseCharacters(data) {
    if (!Array.isArray(data)) return [];
    return data.filter(isRecord).map((entry) => ({ id: text(entry.id), name: text(entry.name), kind: text(entry.kind) }));
  }
  var pcs = (characters) => characters.filter((character) => character.kind === "pj");
  var isPlayed = (session) => session.status === "closed" || session.status === "played";
  function statusLabel(session) {
    if (session.label !== null) return session.label;
    if (isPlayed(session)) return "Jou\xE9e";
    return session.status === "live" ? "En direct" : "En pr\xE9paration";
  }
  function presentName(characters, id) {
    const found = characters.find((character) => character.id === id);
    return (found?.name.split(" ")[0] ?? "") || id;
  }
  var defaultTitle = (sessions) => `S\xE9ance ${sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1}`;
  function problem(form) {
    if (!form.title.trim()) return "Donnez un titre \xE0 la s\xE9ance.";
    if (!/^\d{4}-\d{2}-\d{2}$/.test(form.plannedOn)) return "Choisissez la date de la s\xE9ance.";
    return "";
  }
  function payload(campaign, form, characters) {
    const allowed = new Set(pcs(characters).map((character) => character.id));
    return { campaign, title: form.title.trim(), plannedOn: form.plannedOn, present: form.present.filter((id) => allowed.has(id)) };
  }
  function commandBody(dataCapability, body) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: body, caller: { by: "dm", from: "SessionList", roles: [] } };
  }
  var VIOLATIONS = {
    "present-only-pj": "seuls les PJ de la campagne peuvent \xEAtre pr\xE9sents"
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
  function day(date) {
    const when = /* @__PURE__ */ new Date(`${date}T12:00:00`);
    return Number.isNaN(when.getTime()) ? date : when.toLocaleDateString("fr-FR", { weekday: "short", day: "numeric", month: "long" });
  }
  function skeleton() {
    return `<section class="panel mf-sessions" id="session-list" aria-labelledby="sessions-title"><div class="panel__head"><h2 id="sessions-title">S\xE9ances</h2><div class="kit-button kit-button--bare kit-button--small"><button type="button" class="primary" id="sessions-new" data-writes="${CREATE_SESSION}">Nouvelle s\xE9ance</button></div></div><ol class="mf-sessions__list" id="sessions-items" data-reads="${LIST_SESSIONS}"></ol><p class="mf-sessions__status" id="sessions-status" aria-live="polite"></p></section>`;
  }
  function badges(session) {
    const tone = isPlayed(session) ? "success" : "accent";
    return `<div class="kit-badge"><span class="${tone}">${esc(statusLabel(session))}</span>` + (session.date === null ? "" : `<span>${esc(day(session.date))}</span>`) + "</div>";
  }
  function links(session) {
    const link = (label, href, variant) => `<button type="button" class="${variant}" data-href="${href}">${esc(label)}</button>`;
    return '<div class="kit-button kit-button--bare kit-button--small">' + (isPlayed(session) ? link("R\xE9sum\xE9 et journal", "preparation-seance.html", "ghost") : link("Pr\xE9parer", "preparation-seance.html", "secondary") + link("Lancer en direct", "dm-view.html", "primary")) + "</div>";
  }
  function item(session, characters) {
    const there = session.present.map((id) => presentName(characters, id)).join(", ") || "\u2014";
    return `<li class="mf-sessions__item${isPlayed(session) ? "" : " is-next"}" id="session-${esc(session.id)}"><span class="mf-sessions__num">${session.order}</span><div class="mf-sessions__main"><div class="panel__head"><h3>${esc(session.title)}</h3>${badges(session)}</div>` + (session.summary === null ? "" : `<p class="mf-sessions__summary">${esc(session.summary)}</p>`) + `<p class="small muted" data-reads="${LIST_CHARACTERS}">PJ pr\xE9sents : ${esc(there)}</p>${links(session)}</div></li>`;
  }
  var items = (sessions, characters) => sessions.map((session) => item(session, characters)).join("");
  function dialog(p) {
    const ticked = new Set(p.present);
    const input = (id, label, value, type) => `<div class="kit-input is-wide"><label>${label}<input type="${type}" id="${id}"${attr("value", value)}></label></div>`;
    const options = p.pcs.map((pc) => `<label><input type="checkbox" name="present" value="${esc(pc.id)}"${ticked.has(pc.id) ? " checked" : ""}><span>${esc(pc.name)}</span></label>`).join("");
    return '<div class="mf-sessions__modal" id="session-create-dialog"><div class="kit-dialog"><div class="kit-dialog__veil"><div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="Nouvelle s\xE9ance"><header class="kit-dialog__header"><h2 class="kit-dialog__title">Nouvelle s\xE9ance</h2><button class="kit-dialog__close" type="button" aria-label="Fermer" id="session-create-close">\xD7</button></header><div class="kit-dialog__body">' + input("session-create-title", "Titre", p.title, "text") + input("session-create-date", "Date", p.plannedOn, "date") + `<div class="kit-checkbox"><fieldset id="session-create-present" data-reads="${LIST_CHARACTERS}"><legend>PJ pr\xE9sents</legend>${options}</fieldset></div><p class="small muted">Vous pourrez retirer ou ajouter des PJ dans la pr\xE9paration.</p><p class="mf-sessions__error" id="session-create-error" aria-live="polite"></p></div><footer class="kit-dialog__footer"><button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" id="session-create-cancel">Annuler</button><button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="session-create-submit" data-writes="${CREATE_SESSION}">Cr\xE9er la s\xE9ance</button></footer></div></div></div></div>`;
  }

  // apps/session/session-list/src/style.ts
  var STYLE = `
.mf-sessions, .mf-sessions__modal { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-sessions *, .mf-sessions *::before, .mf-sessions *::after, .mf-sessions__modal *, .mf-sessions__modal *::before, .mf-sessions__modal *::after { box-sizing: inherit; }
.mf-sessions h2, .mf-sessions h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-sessions h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sessions h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-sessions p, .mf-sessions__modal p { margin: 0; }
.mf-sessions.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-sessions .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-sessions .muted, .mf-sessions__modal .muted { color: var(--color-text-muted); }
.mf-sessions .small, .mf-sessions__modal .small { font-size: var(--font-size-small); }
.mf-sessions .kit-button.kit-button--bare, .mf-sessions__modal .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-sessions .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-sessions__modal .kit-input.is-wide { max-width: none; }
.mf-sessions__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-3); }
.mf-sessions__item { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-sessions__item.is-next { border-color: var(--color-primary); box-shadow: var(--shadow-high); }
.mf-sessions__num { display: grid; place-items: center; width: calc(var(--space-5) + var(--space-1)); height: calc(var(--space-5) + var(--space-1)); border-radius: var(--radius-pill); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); font-family: var(--font-family-heading); font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sessions__main { display: grid; gap: var(--space-2); min-width: 0; }
.mf-sessions__summary { color: var(--color-text-muted); }
.mf-sessions__status, .mf-sessions__error { min-height: var(--space-3); }
.mf-sessions__status:empty { display: none; }
.mf-sessions__status, .mf-sessions__error { color: var(--color-danger); }
.mf-sessions__modal { position: fixed; inset: 0; z-index: 100; }
.mf-sessions__modal .kit-dialog, .mf-sessions__modal .kit-dialog__veil { height: 100%; }
.mf-sessions__modal .kit-dialog__window { max-height: 88vh; }
.mf-sessions__modal .kit-dialog__body > * + * { margin-top: var(--space-3); }
`;

  // apps/session/session-list/src/index.ts
  var ENV = {
    today: () => {
      const now = /* @__PURE__ */ new Date();
      return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
    },
    sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    everyMs: 400,
    polls: 120
  };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="SessionList"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "SessionList";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, props2, shell2, env = ENV) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const campaign = typeof props2.campaign === "string" ? props2.campaign : "";
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`SessionList: #${id} is not in the list`);
      return found;
    };
    let sessions = [];
    let characters = [];
    const paint = () => {
      part("sessions-items").innerHTML = items(sessions, characters);
    };
    const say = (text2) => {
      part("sessions-status").textContent = text2;
    };
    async function refresh() {
      const [listed, known] = await Promise.allSettled([shell2.capability(LIST_SESSIONS, void 0, { campaign }), shell2.capability(LIST_CHARACTERS, void 0, { campaign })]);
      if (listed.status === "fulfilled") sessions = parseSessions(listed.value.data);
      if (known.status === "fulfilled") characters = parseCharacters(known.value.data);
      paint();
      const ok = listed.status === "fulfilled" && known.status === "fulfilled";
      say(ok ? "" : listed.status === "rejected" ? "Impossible de lire les s\xE9ances." : "Impossible de lire les personnages.");
      return ok;
    }
    async function send(body) {
      const answer = await shell2.command(body);
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < env.polls; asked += 1) {
        await env.sleep(env.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    let open = null;
    let busy = false;
    const close = () => {
      open?.remove();
      open = null;
    };
    const field = (id) => {
      const found = open?.querySelector(`#${id}`);
      if (!found) throw new Error(`SessionList: #${id} is not in the dialog`);
      return found;
    };
    const ask = () => ({
      title: field("session-create-title").value,
      plannedOn: field("session-create-date").value,
      present: Array.from(open?.querySelectorAll('input[name="present"]:checked') ?? []).map((box) => box.value)
    });
    const complain = (text2) => {
      const area = open?.querySelector("#session-create-error");
      if (area) area.textContent = text2;
    };
    function openDialog() {
      if (open) return;
      const players = pcs(characters);
      const wrapper = host2.ownerDocument.createElement("div");
      wrapper.innerHTML = dialog({ title: defaultTitle(sessions), plannedOn: env.today(), pcs: players, present: players.map((pc) => pc.id) });
      open = wrapper.firstElementChild;
      host2.append(open);
      open.addEventListener("keydown", (event) => {
        if (event.key === "Escape") close();
      });
      field("session-create-title").focus();
    }
    async function create() {
      if (busy || !open) return;
      const form = ask();
      const why = problem(form);
      if (why) return complain(why);
      busy = true;
      const submit = part("session-create-submit");
      submit.disabled = true;
      complain("");
      try {
        const state = await send(commandBody(submit.dataset.writes ?? CREATE_SESSION, payload(campaign, form, characters)));
        if (state.status === "rejected") return complain(`Cr\xE9ation refus\xE9e : ${reasons(state.violations)}.`);
        if (state.status !== "applied") return complain(`La cr\xE9ation n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`);
        close();
        if (!await refresh()) say("La s\xE9ance est cr\xE9\xE9e, mais la liste n\u2019a pas pu \xEAtre relue.");
      } catch {
        complain("La cr\xE9ation a \xE9chou\xE9.");
      } finally {
        busy = false;
        submit.disabled = false;
      }
    }
    host2.addEventListener("click", (event) => {
      const target = event.target;
      const button = target?.closest("button");
      if (button?.dataset.writes === CREATE_SESSION && button.id === "sessions-new") openDialog();
      else if (button?.dataset.writes === CREATE_SESSION) void create();
      else if (button?.id === "session-create-cancel" || button?.id === "session-create-close") close();
      else if (target?.classList.contains("kit-dialog__veil")) close();
    });
    paint();
    void refresh();
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
    // session.createSession (#157, planned): the next session of the campaign,
    // at the end of the list, in preparation, with the PCs ticked present.
    // `present-only-pj` refuses a character that is not a PC.
    "session.createSession": async (server, { payload: payload2 }) => {
      const pcs2 = new Set((await server.load("campaign.listCharacters")).filter((c) => c.kind === "pj").map((c) => c.id));
      const present = Array.isArray(payload2.present) ? payload2.present : [];
      if (present.some((id) => !pcs2.has(id))) return ["present-only-pj"];
      const sessions = await server.load("session.listSessions");
      const order = sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1;
      sessions.push({ id: `s${order}`, order, title: payload2.title, date: payload2.plannedOn, status: "prep", present, summary: null });
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
    s.fetch = async (input, init) => {
      await delay(LATENCY_MS);
      const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url);
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
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  compose({ campaign: "cendres-valombre", ...props }, (resolved) => mount(host, resolved, shell));
})();
