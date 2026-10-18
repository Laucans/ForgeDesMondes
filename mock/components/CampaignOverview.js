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

  // apps/campaign/campaign-overview/src/ids.ts
  var GET_CAMPAIGN = "campaign.getCampaign";
  var EXPORT_CAMPAIGN = "campaign.exportCampaign";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/campaign/campaign-overview/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  function nextSession(value) {
    if (!isRecord(value) || typeof value.order !== "number" || typeof value.date !== "string") return null;
    return { order: value.order, date: value.date };
  }
  function parseCampaign(data) {
    if (!isRecord(data) || typeof data.id !== "string" || typeof data.name !== "string") return null;
    const counts2 = {};
    if (isRecord(data.counts)) {
      for (const [name, value] of Object.entries(data.counts)) if (typeof value === "number") counts2[name] = value;
    }
    return {
      id: data.id,
      name: data.name,
      type: data.type === "oneshot" ? "oneshot" : "campagne",
      art: text(data.art),
      glyph: text(data.glyph),
      pitch: text(data.pitch),
      lastExport: typeof data.lastExport === "string" && data.lastExport !== "" ? data.lastExport : null,
      counts: counts2,
      nextSession: nextSession(data.nextSession)
    };
  }
  var COUNT_LABELS = [
    ["sessions", "s\xE9ances"],
    ["players", "joueurs"],
    ["characters", "personnages"],
    ["places", "lieux"],
    ["events", "\xE9v\xE9nements"],
    ["media", "m\xE9dias"],
    ["sounds", "sons"]
  ];
  function countsOf(campaign) {
    const out = [];
    for (const [name, label] of COUNT_LABELS) {
      const value = campaign.counts[name];
      if (value !== void 0) out.push({ value, label });
    }
    return out;
  }
  function parse(value) {
    const date = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T12:00:00` : value);
    return Number.isNaN(date.getTime()) ? null : date;
  }
  function nextSessionDay(value) {
    const date = parse(value);
    return date === null ? value : date.toLocaleDateString("fr-FR", { weekday: "long", day: "numeric", month: "long" });
  }
  function lastExportText(lastExport) {
    if (lastExport === null) return "Jamais export\xE9";
    const date = parse(lastExport);
    return `Dernier export : ${date === null ? lastExport : date.toLocaleDateString("fr-FR", { day: "numeric", month: "long", year: "numeric" })}`;
  }
  function campaignId(props2, search) {
    return text(props2.campaign).trim() || (new URLSearchParams(search).get("id") ?? "").trim();
  }
  function exportCommand(dataCapability, id) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: { id }, caller: { by: "dm", from: "CampaignOverview", roles: [] } };
  }
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    const violations = Array.isArray(result.violations) ? result.violations.filter((v) => typeof v === "string") : [];
    return { status: text(result.status), violations };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/campaign/campaign-overview/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var reads = `data-reads="${GET_CAMPAIGN}"`;
  function skeleton() {
    return `<section class="mf-overview" id="campaign-overview" aria-labelledby="overview-title" ${reads}><div class="mf-overview__cover" id="overview-cover" ${reads}></div><div class="mf-overview__body"><div class="row" id="overview-type" ${reads}></div><h1 id="overview-title" ${reads}></h1><p class="muted" id="overview-pitch" ${reads}></p><ul class="mf-overview__counts" id="overview-counts" ${reads}></ul><div id="overview-actions"></div><p class="small muted" id="overview-export-status" ${reads} aria-live="polite"></p></div></section>`;
  }
  function cover(campaign) {
    const art = /^[a-z]+$/.test(campaign.art) ? campaign.art : "ember";
    return `<div class="art art--${art}" role="img" aria-label="${esc(`Couverture : ${campaign.name}`)}">` + (campaign.glyph === "" ? "" : `<span class="art__glyph">${esc(campaign.glyph)}</span>`) + '<span class="art__caption">Couverture \xB7 affich\xE9e sur la player view</span></div>';
  }
  function badges(campaign) {
    const next = campaign.nextSession;
    return `<div class="kit-badge"><span class="primary">${campaign.type === "oneshot" ? "Oneshot" : "Campagne"}</span>` + (next === null ? "" : `<span class="accent">${esc(`Prochaine : ${nextSessionDay(next.date)}`)}</span>`) + "</div>";
  }
  function counts(campaign) {
    return countsOf(campaign).map(({ value, label }) => `<li><b>${value}</b>${esc(label)}</li>`).join("");
  }
  function actions(campaign) {
    const next = campaign.nextSession;
    return '<div class="kit-button kit-button--bare">' + (next === null ? "" : `<button type="button" class="primary" id="overview-launch" data-href="dm-view.html">${esc(`Lancer la s\xE9ance ${next.order} en direct`)}</button><button type="button" class="secondary" id="overview-prepare" data-href="preparation-seance.html">${esc(`Pr\xE9parer la s\xE9ance ${next.order}`)}</button>`) + `<button type="button" class="ghost" id="overview-export" data-writes="${EXPORT_CAMPAIGN}">Exporter l\u2019archive</button></div>`;
  }

  // apps/campaign/campaign-overview/src/style.ts
  var STYLE = `
.mf-overview { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-overview *, .mf-overview *::before, .mf-overview *::after { box-sizing: inherit; }
.mf-overview { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); gap: var(--space-4); align-items: stretch; background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); overflow: hidden; box-shadow: var(--shadow-high); }
.mf-overview h1 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: var(--font-size-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; color: var(--color-primary); }
.mf-overview p { margin: 0; }
.mf-overview .muted { color: var(--color-text-muted); }
.mf-overview .small { font-size: var(--font-size-small); }
.mf-overview .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-overview__cover { min-height: calc(var(--space-5) * 5); }
.mf-overview__body { display: grid; gap: var(--space-2); align-content: center; padding: var(--space-4); }
.mf-overview__counts { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--space-3); font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-overview__counts b { color: var(--color-text); font-size: var(--font-size-large); margin-right: var(--space-1); }
.mf-overview .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-overview .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-overview .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-overview .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-overview .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-overview .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-overview .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-overview .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-overview .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-overview .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-overview .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-overview .art__caption { position: absolute; z-index: 1; left: var(--space-3); right: var(--space-3); bottom: var(--space-2); font-family: var(--font-family); font-size: var(--font-size-small); text-transform: none; color: var(--color-text); text-shadow: var(--shadow-low); }
@media (max-width: 760px) { .mf-overview { grid-template-columns: minmax(0, 1fr); } }
`;

  // apps/campaign/campaign-overview/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="CampaignOverview"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "CampaignOverview";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var listening = /* @__PURE__ */ new WeakMap();
  function mount(host2, props2, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    const previous = listening.get(host2);
    if (previous) host2.removeEventListener("click", previous);
    host2.innerHTML = skeleton();
    const part = (id2) => {
      const found = host2.querySelector(`#${id2}`);
      if (!found) throw new Error(`CampaignOverview: #${id2} is not in the panel`);
      return found;
    };
    const id = campaignId(props2, host2.ownerDocument.defaultView?.location.search ?? "");
    let campaign = null;
    let busy = false;
    const say = (words) => {
      part("overview-export-status").textContent = words;
    };
    const paintExportButton = () => {
      for (const button of host2.querySelectorAll("#overview-export")) button.disabled = busy;
    };
    const paint = (shown) => {
      part("overview-cover").innerHTML = cover(shown);
      part("overview-type").innerHTML = badges(shown);
      part("overview-title").textContent = shown.name;
      part("overview-pitch").textContent = shown.pitch;
      part("overview-counts").innerHTML = counts(shown);
      part("overview-actions").innerHTML = actions(shown);
      say(lastExportText(shown.lastExport));
      paintExportButton();
    };
    const read = async () => parseCampaign((await shell2.capability(GET_CAMPAIGN, void 0, { id })).data);
    async function send() {
      const answer = await shell2.command(exportCommand(EXPORT_CAMPAIGN, id));
      const followed = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && followed !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(followed));
      }
      return state;
    }
    async function exportCampaign() {
      if (busy || campaign === null) return;
      const before = lastExportText(campaign.lastExport);
      busy = true;
      paintExportButton();
      say("Export en cours\u2026");
      try {
        const state = await send();
        if (state.status === "rejected") return say(`Export refus\xE9 : ${state.violations.join(", ") || "raison inconnue"}. ${before}`);
        if (state.status !== "applied") return say(`L\u2019export n\u2019est pas encore appliqu\xE9 (\xE9tat : ${state.status || "inconnu"}). ${before}`);
        try {
          const fresh = await read();
          if (fresh !== null) {
            campaign = fresh;
            return paint(fresh);
          }
        } catch {
        }
        say(`Export effectu\xE9, mais la date n\u2019a pas pu \xEAtre relue. ${before}`);
      } catch {
        say(`L\u2019export a \xE9chou\xE9. ${before}`);
      } finally {
        busy = false;
        paintExportButton();
      }
    }
    const onClick = (event) => {
      const button = event.target?.closest("button");
      if (!button) return;
      if (button.dataset.writes === EXPORT_CAMPAIGN) void exportCampaign();
      else if (button.dataset.href) host2.ownerDocument.defaultView?.location.assign(button.dataset.href);
    };
    listening.set(host2, onClick);
    host2.addEventListener("click", onClick);
    if (id === "") return say("Aucune campagne ouverte.");
    read().then(
      (loaded) => {
        if (loaded === null) return say("Campagne introuvable.");
        campaign = loaded;
        paint(loaded);
      },
      () => say("Impossible de lire la campagne.")
    );
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
    // campaign.exportCampaign (crates/dataguard/data-capabilities/export-campaign):
    // stamps the campaign's last export with today's date and changes nothing
    // else (`export-stamps-date`). An unknown campaign is `not-found`.
    "campaign.exportCampaign": async (server, { payload }) => {
      const campaign = await server.load("campaign.getCampaign");
      if (!campaign || campaign.id !== payload.id) return ["not-found"];
      const now = /* @__PURE__ */ new Date();
      const pad = (n) => String(n).padStart(2, "0");
      campaign.lastExport = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
      return void 0;
    },
    // ai.saveSettings (crates/ai/data-capabilities/save-settings): the DM's
    // provider saved and made active. The API type is derived from the URL,
    // never taken from the payload: the cloud host by itself, the home server
    // at the URL whose model list the fixtures know; any other URL is refused
    // by `api-type-recognized` (« type d'API non reconnu »). `binary: claude`
    // is stored « claude-code ». The key is kept masked, never echoed.
    "ai.saveSettings": async (server, { payload }) => {
      const settings = await server.load("ai.getSettings");
      const models = { generationModel: payload.generationModel, summaryModel: payload.summaryModel };
      if (!String(models.generationModel || "") || !String(models.summaryModel || "")) return ["single-settings-row"];
      if (payload.mode === "api") {
        const url = String(payload.url || "");
        const type = apiType(settings, url);
        if (!type) return ["api-type-recognized"];
        const key = String(payload.key || "");
        settings.providers[type] = { ...settings.providers[type], url, ...models, key: key ? `\u2022\u2022\u2022\u2022${key.slice(-4)}` : "" };
        Object.assign(settings, { mode: "api", provider: type, binary: null });
        return void 0;
      }
      if (payload.mode === "binary") {
        const binary = { claude: "claude-code", "claude-code": "claude-code", opencode: "opencode" }[payload.binary];
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
    "ai.testConnection": async (server, { payload }) => {
      const settings = await server.load("ai.getSettings");
      const at = (/* @__PURE__ */ new Date()).toISOString();
      const provider = payload.mode === "api" ? apiType(settings, String(payload.url || "")) : payload.mode === "binary" ? { claude: "claude-code", "claude-code": "claude-code", opencode: "opencode" }[payload.binary] || null : null;
      if (!provider) {
        settings.lastTest = { ok: false, at, provider: settings.provider, latency: null, message: "type d'API non reconnu" };
        return void 0;
      }
      const message = payload.mode === "api" && settings.testResults?.[provider]?.message || `Connexion r\xE9ussie (binaire ${provider})`;
      const latency = message.match(/répond en ([\d,]+ s)/)?.[1] || "0,01 s";
      settings.lastTest = { ok: true, at, provider, latency, message };
      return void 0;
    }
  };

  // mock/src/reads.js
  var READS = {
    // One element's sheet: a character, a place or an event of `entities`.
    "campaign.getEntity": async (server, { id }) => {
      const entities = await server.load("entities");
      return [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === id);
    }
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
  function ruleHolds(rule, payload) {
    const m = /^\s*([A-Za-z_][\w.]*)\s*(<=|>=|==|!=|<|>|=)\s*(.+?)\s*$/.exec(String(rule || ""));
    if (!m) return true;
    const [, field, op, raw] = m;
    if (!(field in payload)) return true;
    const left = payload[field];
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
    const invariants = async (aggregate, payload) => {
      const out = { reject: [], review: [] };
      if (!aggregate) return out;
      const manifest = await s.load(`aggregates/${aggregate}`);
      for (const inv of manifest && manifest.invariants || []) {
        if (!ruleHolds(inv.rule, payload)) (inv.onViolation === "review" ? out.review : out.reject).push(inv.id);
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
      const payload = body.payload || {};
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
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
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
      const verdict = await invariants(aggregate, payload);
      if (verdict.reject.length || verdict.review.length) {
        entry.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId2, { violations: verdict.reject.concat(verdict.review) });
      }
      const critical = (await s.load(`aggregates/${aggregate}`) || { fields: {} }).fields || {};
      const needsReview = verdict.review.length > 0 || Object.keys(payload).some((f) => critical[f] && critical[f].criticality === "critical");
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
        s.tell("ValueDeclaredAhead", by, commandId2, { field: `${partition}.${ahead.field}`, yourValue: payload[ahead.field], declaredAhead: { value: ahead.value, by: ahead.by, command: ahead.command }, actions: ["confirm_overwrite", "cancel", "edit"] });
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

  // mock/src/CampaignOverview.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var opened = new URLSearchParams(location.search).get("id") ? {} : { campaign: "cendres-valombre" };
  compose(props, (resolved) => mount(host, { ...opened, ...resolved }, shell));
})();
