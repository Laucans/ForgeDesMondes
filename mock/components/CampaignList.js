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
  var asStrings = (value) => Array.isArray(value) && value.every((item) => typeof item === "string") ? value : void 0;
  function asCaller(value) {
    const caller2 = value && typeof value === "object" ? value : {};
    const roles = asStrings(caller2.roles);
    const permissions = caller2.permissions === void 0 ? [] : asStrings(caller2.permissions);
    return typeof caller2.by === "string" && caller2.by && roles && permissions ? { by: caller2.by, roles, permissions } : void 0;
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
  function withCaller(body, caller2) {
    if (!body || typeof body !== "object" || Array.isArray(body)) return body;
    const sent = body.caller;
    return { ...body, caller: { ...sent && typeof sent === "object" ? sent : {}, by: caller2.by, roles: caller2.roles, permissions: caller2.permissions } };
  }
  function createShell(fetcher = (input2, init) => fetch(input2, init), page = browserPage()) {
    let resolving;
    const callerOf = async () => {
      const asked = resolving ??= pageCaller(page);
      const { caller: caller2, settled: settled2 } = await asked;
      if (!settled2 && resolving === asked) resolving = void 0;
      return caller2;
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

  // apps/campaign/campaign-list/src/ids.ts
  var LIST_CAMPAIGNS = "campaign.listCampaigns";
  var CREATE_CAMPAIGN = "campaign.createCampaign";
  var EXPORT_CAMPAIGN = "campaign.exportCampaign";
  var IMPORT_CAMPAIGN = "campaign.importCampaign";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/campaign/campaign-list/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var day = (value) => typeof value === "string" && value !== "" ? value : null;
  function parseCampaigns(data) {
    const items = Array.isArray(data) ? data : isRecord(data) && Array.isArray(data.campaigns) ? data.campaigns : [];
    const out = [];
    for (const item of items) {
      if (!isRecord(item) || typeof item.id !== "string" || item.id === "" || typeof item.name !== "string") continue;
      out.push({
        id: item.id,
        name: item.name,
        type: item.type === "oneshot" ? "oneshot" : "campagne",
        art: text(item.art),
        glyph: text(item.glyph),
        pitch: text(item.pitch),
        sessions: typeof item.sessions === "number" ? item.sessions : 0,
        lastPlayed: day(item.lastPlayed),
        next: day(item.next)
      });
    }
    return out;
  }
  function dayText(value) {
    const date = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T12:00:00` : value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString("fr-FR", { day: "numeric", month: "long", year: "numeric" });
  }
  var sessionsText = (sessions) => `${sessions} s\xE9ance${sessions > 1 ? "s" : ""}`;
  var lastPlayedText = (lastPlayed) => lastPlayed === null ? "jamais jou\xE9e" : `jou\xE9e le ${dayText(lastPlayed)}`;
  var glyphOf = (name) => [...name.trim()][0]?.toUpperCase() ?? "";
  var caller = { by: "dm", from: "CampaignList", roles: [] };
  var command = (dataCapability, payload, idempotencyKey) => ({
    dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
    payload,
    ...idempotencyKey === void 0 ? {} : { idempotencyKey },
    caller
  });
  function createCommand(dataCapability, form, idempotencyKey) {
    const name = form.name.trim();
    const pitch = form.pitch.trim();
    return command(dataCapability, { name, type: form.type, pitch: pitch === "" ? null : pitch, glyph: glyphOf(name) }, idempotencyKey);
  }
  var exportCommand = (dataCapability, id, idempotencyKey) => command(dataCapability, { id }, idempotencyKey);
  var importCommand = (dataCapability, archive, idempotencyKey) => command(dataCapability, { archive }, idempotencyKey);
  var mayBeQueued = (state) => !isFinal(state.status) && !(state.status === "failed" && state.violations.length > 0);
  function parseArchive(source) {
    try {
      const document2 = JSON.parse(source);
      return isRecord(document2) ? document2 : void 0;
    } catch {
      return void 0;
    }
  }
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function archiveOf(value) {
    if (!isRecord(value) || typeof value.file !== "string" || value.file === "" || value.content === void 0 || value.content === null) return null;
    return { file: value.file, content: value.content };
  }
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    const violations = Array.isArray(result.violations) ? result.violations.filter((v) => typeof v === "string") : [];
    return { status: text(result.status), violations, archive: archiveOf(result.attachment) };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }
  var failed = (why) => ({ status: "failed", violations: why === "" ? [] : [why], archive: null });
  function unapplied(state, words) {
    const reasons = state.violations.length > 0 ? state.violations.join(", ") : "raison inconnue";
    if (state.status === "failed" && state.violations.length === 0) return words.failed;
    if (state.status === "rejected" || state.status === "failed") return `${words.refused} : ${reasons}.`;
    return `${words.pending} : \xE9tat ${state.status || "inconnu"}.`;
  }
  var safeFileName = (file) => file.replace(/[\\/:*?"<>|]+/g, "-");
  function refusedBy(error) {
    const body = isRecord(error) ? error.body : void 0;
    return isRecord(body) ? text(body.error) : "";
  }

  // apps/campaign/campaign-list/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var reads = `data-reads="${LIST_CAMPAIGNS}"`;
  function skeleton() {
    return `<section class="mf-campaigns" id="campaign-list" aria-labelledby="campaign-list-title"><div class="panel__head"><h2 id="campaign-list-title">Campagnes et oneshots</h2><div id="campaign-list-actions"><div class="kit-button kit-button--bare"><button type="button" class="primary" id="campaign-new" data-writes="${CREATE_CAMPAIGN}">Nouvelle campagne</button><button type="button" class="secondary" id="campaign-import" data-writes="${IMPORT_CAMPAIGN}">Importer une archive</button></div></div></div><p class="small muted" id="campaign-status" aria-live="polite"></p><div class="mf-campaigns__grid" id="campaign-cards" ${reads}></div><div id="campaign-dialogs"></div></section>`;
  }
  function cover(campaign) {
    const art = /^[a-z]+$/.test(campaign.art) ? campaign.art : "ember";
    return `<div class="art art--${art}" role="img" aria-label="${esc(`Couverture : ${campaign.name}`)}">` + (campaign.glyph === "" ? "" : `<span class="art__glyph">${esc(campaign.glyph)}</span>`) + "</div>";
  }
  function card(campaign) {
    const meta = `<div class="mf-campaigns__meta"><div class="kit-badge"><span class="${campaign.type === "oneshot" ? "accent" : "primary"}">${campaign.type === "oneshot" ? "Oneshot" : "Campagne"}</span></div><span>${sessionsText(campaign.sessions)}</span><span>\xB7 ${esc(lastPlayedText(campaign.lastPlayed))}</span></div>` + (campaign.next === null ? "" : `<p class="small">Prochaine s\xE9ance : <b>${esc(dayText(campaign.next))}</b></p>`);
    return `<div id="campaign-${esc(campaign.id)}" ${reads}><article class="kit-card"><div class="media">${cover(campaign)}</div><div class="body"><h3>${esc(campaign.name)}</h3>${campaign.pitch === "" ? "" : `<p>${esc(campaign.pitch)}</p>`}${meta}</div><div class="actions"><button type="button" data-href="campagne.html?id=${encodeURIComponent(campaign.id)}">Ouvrir</button><button type="button" class="secondary" data-action="export" data-campaign="${esc(campaign.id)}" data-writes="${EXPORT_CAMPAIGN}">Exporter</button></div></article></div>`;
  }
  var cards = (campaigns) => campaigns.map(card).join("");
  var empty = () => '<p class="muted" id="campaign-empty">Aucune campagne pour l\u2019instant : cr\xE9ez-en une ou importez une archive.</p>';
  function dialog(id, title, body, submit) {
    return `<div class="mf-modal" id="${id}"><div class="kit-dialog"><div class="kit-dialog__veil"><div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="${esc(title)}"><header class="kit-dialog__header"><h2 class="kit-dialog__title">${esc(title)}</h2><button class="kit-dialog__close" type="button" aria-label="Fermer" data-close>\xD7</button></header><div class="kit-dialog__body">${body}</div><footer class="kit-dialog__footer"><button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" data-close>Annuler</button><button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="${submit.id}" data-writes="${submit.writes}"${submit.disabled ? " disabled" : ""}>${esc(submit.label)}</button></footer></div></div></div></div>`;
  }
  var input = (id, label, placeholder) => `<div class="kit-input is-wide"><label>${esc(label)}<input type="text" id="${id}" placeholder="${esc(placeholder)}"></label></div>`;
  var UPLOAD_ICON = '<svg class="kit-filedrop__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>';
  function fileDrop() {
    return '<div class="kit-filedrop is-wide" id="campaign-import-field"><label class="kit-filedrop__zone" tabindex="0" id="campaign-import-drop">' + UPLOAD_ICON + '<span class="kit-filedrop__title">D\xE9posez l\u2019archive ou <b>parcourez</b></span><span class="kit-filedrop__hint">Fichier .json \xE9crit par un export</span><span class="kit-filedrop__types"><span class="kit-filedrop__tag">JSON</span></span><input class="kit-filedrop__input" type="file" id="campaign-import-file" accept=".json,application/json"></label><div class="kit-filedrop__error">Ce fichier n\u2019est pas une archive.</div><div class="kit-filedrop__file"><div class="kit-filedrop__thumb"></div><div class="kit-filedrop__info"><div class="kit-filedrop__name"></div><div class="kit-filedrop__row"><span class="kit-filedrop__size"></span><span class="kit-filedrop__pct">Pr\xEAt</span></div><div class="kit-filedrop__bar"><div class="kit-filedrop__fill"></div></div></div></div></div>';
  }
  function createDialog() {
    return dialog(
      "campaign-create-dialog",
      "Nouvelle campagne",
      input("campaign-create-name", "Nom", "Les Cendres de Valombre") + '<div class="kit-radio"><fieldset id="campaign-create-type"><legend>Type</legend><label><input type="radio" name="campaign-type" value="campagne" checked><span>Campagne</span><small>plusieurs s\xE9ances</small></label><label><input type="radio" name="campaign-type" value="oneshot"><span>Oneshot</span><small>une seule s\xE9ance</small></label></fieldset></div>' + input("campaign-create-pitch", "Pitch (facultatif)", "Un bourg couvert de cendres\u2026") + '<p class="small is-error" id="campaign-create-error" aria-live="polite"></p>',
      { id: "campaign-create-submit", label: "Cr\xE9er", writes: CREATE_CAMPAIGN }
    );
  }
  function importDialog() {
    return dialog(
      "campaign-import-dialog",
      "Importer une archive",
      '<p class="kit-dialog__text">Une archive export\xE9e depuis ForgeDesMondes, sur cette machine ou une autre. L\u2019import cr\xE9e toujours une nouvelle campagne : l\u2019originale n\u2019est jamais modifi\xE9e.</p>' + fileDrop() + '<p class="small is-error" id="campaign-import-error" aria-live="polite"></p>',
      { id: "campaign-import-submit", label: "Importer", writes: IMPORT_CAMPAIGN, disabled: true }
    );
  }

  // apps/campaign/campaign-list/src/style.ts
  var STYLE = `
.mf-campaigns, .mf-modal { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-campaigns *, .mf-campaigns *::before, .mf-campaigns *::after, .mf-modal *, .mf-modal *::before, .mf-modal *::after { box-sizing: inherit; }
.mf-campaigns [hidden], .mf-modal [hidden] { display: none !important; }
.mf-campaigns h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: var(--font-size-large); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; color: var(--color-accent); }
.mf-campaigns p, .mf-modal p { margin: 0; }
.mf-campaigns .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-campaigns .muted { color: var(--color-text-muted); }
.mf-campaigns .small, .mf-modal .small { font-size: var(--font-size-small); }
.mf-campaigns .is-error, .mf-modal .is-error { color: var(--color-danger); }
.mf-campaigns { display: grid; gap: var(--space-3); }
.mf-campaigns__grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 7), 1fr)); gap: var(--space-4); }
.mf-campaigns .kit-card { max-width: none; height: 100%; display: flex; flex-direction: column; }
.mf-campaigns .kit-card .body { flex: 1; }
.mf-campaigns .kit-card .media { height: calc(var(--space-5) * 4); }
.mf-campaigns .kit-card .media .art { width: 100%; height: 100%; }
.mf-campaigns__meta { display: flex; flex-wrap: wrap; gap: var(--space-2); align-items: center; font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-campaigns .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-campaigns .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-campaigns .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-campaigns .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-campaigns .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-campaigns .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-campaigns .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-campaigns .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-campaigns .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-campaigns .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-campaigns .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-modal { position: fixed; inset: 0; z-index: 100; }
.mf-modal .kit-dialog, .mf-modal .kit-dialog__veil { height: 100%; }
.mf-modal .kit-dialog__window { max-height: 88vh; }
.mf-modal .kit-dialog__body > * + * { margin-top: var(--space-3); }
`;

  // apps/campaign/campaign-list/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  var REVOKE_AFTER_MS = 1e4;
  var downloadIn = (doc) => (file, content) => {
    const win = doc.defaultView;
    if (!win) throw new Error("no window to download from");
    const url = win.URL.createObjectURL(new win.Blob([JSON.stringify(content, null, 2)], { type: "application/json" }));
    const link = doc.createElement("a");
    link.href = url;
    link.download = safeFileName(file);
    doc.body.append(link);
    link.click();
    link.remove();
    win.setTimeout(() => win.URL.revokeObjectURL(url), REVOKE_AFTER_MS);
  };
  var EXPORT_WORDS = { refused: "Export refus\xE9", pending: "Export non appliqu\xE9", failed: "L\u2019export a \xE9chou\xE9." };
  var CREATE_WORDS = { refused: "Cr\xE9ation refus\xE9e", pending: "Cr\xE9ation non appliqu\xE9e", failed: "La cr\xE9ation a \xE9chou\xE9." };
  var IMPORT_WORDS = { refused: "Import refus\xE9", pending: "Import non appliqu\xE9", failed: "L\u2019import a \xE9chou\xE9." };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="CampaignList"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "CampaignList";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var readText = (doc, file) => new Promise((resolve, reject) => {
    const Reader = doc.defaultView?.FileReader;
    if (!Reader) return reject(new Error("no reader"));
    const reader = new Reader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : "");
    reader.onerror = () => reject(new Error("the file could not be read"));
    reader.readAsText(file);
  });
  var size = (bytes) => bytes < 1024 ? `${bytes} o` : bytes < 1048576 ? `${(bytes / 1024).toFixed(1).replace(".", ",")} Ko` : `${(bytes / 1048576).toFixed(1).replace(".", ",")} Mo`;
  var keys = 0;
  var newKey = () => `CampaignList:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}-${keys += 1}`;
  var listening = /* @__PURE__ */ new WeakMap();
  function mount(host2, _props, shell2, timing = WAITING, save = downloadIn(host2.ownerDocument)) {
    ensureStyle(host2.ownerDocument);
    for (const drop of listening.get(host2) ?? []) drop();
    const dropped = [];
    listening.set(host2, dropped);
    const current = () => listening.get(host2) === dropped;
    const on = (type, handler) => {
      host2.addEventListener(type, handler);
      dropped.push(() => host2.removeEventListener(type, handler));
    };
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`CampaignList: #${id} is not in the list`);
      return found;
    };
    let campaigns = [];
    let busy = false;
    let archive;
    let key = newKey();
    let keyed = null;
    const renewKey = () => {
      key = newKey();
      keyed = null;
    };
    const exportKeys = /* @__PURE__ */ new Map();
    const keyFor = (payload) => {
      const sent = JSON.stringify(payload);
      if (keyed !== null && keyed !== sent) key = newKey();
      keyed = sent;
      return key;
    };
    const say = (words) => {
      if (current()) part("campaign-status").textContent = words;
    };
    const paintBusy = () => {
      if (!current()) return;
      for (const button of host2.querySelectorAll("[data-writes]")) {
        button.disabled = busy || button.id === "campaign-import-submit" && archive === void 0;
      }
    };
    const paint = () => {
      if (!current()) return;
      part("campaign-cards").innerHTML = campaigns.length === 0 ? empty() : cards(campaigns);
      paintBusy();
    };
    const read = async () => parseCampaigns((await shell2.capability(LIST_CAMPAIGNS, void 0, {})).data);
    async function send(body) {
      const answer = await shell2.command(body);
      const followed = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && followed !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        try {
          state = settled(await shell2.commandResult(followed));
        } catch {
        }
      }
      return state;
    }
    async function run(body) {
      busy = true;
      paintBusy();
      try {
        return await send(body);
      } catch (error) {
        return failed(refusedBy(error));
      } finally {
        busy = false;
        paintBusy();
      }
    }
    async function reload(applied) {
      try {
        campaigns = await read();
        paint();
        say(applied);
      } catch {
        say(`${applied} La liste n\u2019a pas pu \xEAtre relue.`);
      }
    }
    let dialogs = 0;
    const closeDialog = () => {
      dialogs += 1;
      part("campaign-dialogs").innerHTML = "";
      archive = void 0;
    };
    const leave = () => {
      if (!busy && part("campaign-dialogs").firstElementChild) closeDialog();
    };
    const dialogError = (id, words) => {
      if (current()) part(id).textContent = words;
    };
    async function exportCampaign(id) {
      if (busy) return;
      const campaign = campaigns.find((c) => c.id === id);
      if (!campaign) return;
      say(`Export de \xAB ${campaign.name} \xBB en cours\u2026`);
      const exportKey = exportKeys.get(id) ?? newKey();
      exportKeys.set(id, exportKey);
      const state = await run(exportCommand(EXPORT_CAMPAIGN, id, exportKey));
      if (!mayBeQueued(state)) exportKeys.delete(id);
      if (!current()) return;
      if (state.status !== "applied") return say(unapplied(state, EXPORT_WORDS));
      if (state.archive === null) return say("Export effectu\xE9, mais l\u2019archive n\u2019a pas \xE9t\xE9 re\xE7ue.");
      try {
        save(state.archive.file, state.archive.content);
        say(`Archive \xAB ${state.archive.file} \xBB enregistr\xE9e.`);
      } catch {
        say("Export effectu\xE9, mais l\u2019archive n\u2019a pas pu \xEAtre enregistr\xE9e.");
      }
    }
    async function createCampaign() {
      if (busy) return;
      const name = part("campaign-create-name").value;
      if (name.trim() === "") return dialogError("campaign-create-error", "Le nom est obligatoire.");
      const type = part("campaign-create-dialog").querySelector('input[name="campaign-type"]:checked')?.value === "oneshot" ? "oneshot" : "campagne";
      const pitch = part("campaign-create-pitch").value;
      dialogError("campaign-create-error", "");
      const form = { name, type, pitch };
      const state = await run(createCommand(CREATE_CAMPAIGN, form, keyFor({ name: name.trim(), type, pitch: pitch.trim() })));
      if (!current()) return;
      if (state.status !== "applied") {
        if (!mayBeQueued(state)) renewKey();
        return dialogError("campaign-create-error", unapplied(state, CREATE_WORDS));
      }
      renewKey();
      closeDialog();
      await reload(`Campagne \xAB ${name.trim()} \xBB cr\xE9\xE9e.`);
    }
    async function importArchive() {
      if (busy || archive === void 0) return;
      dialogError("campaign-import-error", "");
      const state = await run(importCommand(IMPORT_CAMPAIGN, archive, keyFor(archive)));
      if (!current()) return;
      if (state.status !== "applied") {
        if (!mayBeQueued(state)) renewKey();
        return dialogError("campaign-import-error", unapplied(state, IMPORT_WORDS));
      }
      renewKey();
      closeDialog();
      await reload("Archive import\xE9e : une nouvelle campagne a \xE9t\xE9 ajout\xE9e.");
    }
    async function takeFile(file) {
      const drop = part("campaign-import-field");
      const asked = dialogs;
      dialogError("campaign-import-error", "");
      let parsed;
      try {
        parsed = parseArchive(await readText(host2.ownerDocument, file));
      } catch {
        parsed = void 0;
      }
      if (!current() || asked !== dialogs) return;
      archive = parsed;
      if (archive === void 0) {
        drop.classList.remove("has-file", "is-done");
        drop.classList.add("has-error");
        return paintBusy();
      }
      drop.classList.remove("has-error");
      drop.classList.add("has-file", "is-done");
      const named = (cls) => {
        const found = drop.querySelector(`.${cls}`);
        if (!found) throw new Error(`CampaignList: .${cls} is not in the drop zone`);
        return found;
      };
      named("kit-filedrop__name").textContent = file.name;
      named("kit-filedrop__size").textContent = size(file.size);
      named("kit-filedrop__thumb").textContent = (file.name.split(".").pop() ?? "").toUpperCase();
      named("kit-filedrop__fill").style.width = "100%";
      paintBusy();
    }
    const open = (html) => {
      dialogs += 1;
      archive = void 0;
      part("campaign-dialogs").innerHTML = html;
      paintBusy();
    };
    on("click", (event) => {
      const button = event.target?.closest("button");
      if (!button) return;
      const writes = button.dataset.writes;
      if (button.hasAttribute("data-close")) leave();
      else if (button.id === "campaign-new") open(createDialog());
      else if (button.id === "campaign-import") open(importDialog());
      else if (writes === EXPORT_CAMPAIGN) void exportCampaign(button.dataset.campaign ?? "");
      else if (writes === CREATE_CAMPAIGN && button.id === "campaign-create-submit") void createCampaign();
      else if (writes === IMPORT_CAMPAIGN && button.id === "campaign-import-submit") void importArchive();
      else if (button.dataset.href) host2.ownerDocument.defaultView?.location.assign(button.dataset.href);
    });
    on("change", (event) => {
      const target = event.target;
      if (target?.id !== "campaign-import-file") return;
      const file = target.files?.[0];
      if (file) void takeFile(file);
      target.value = "";
    });
    on("dragover", (event) => {
      if (event.target?.closest("#campaign-import-drop")) {
        event.preventDefault();
        part("campaign-import-drop").classList.add("is-over");
      }
    });
    on("dragleave", (event) => {
      if (event.target?.closest("#campaign-import-drop")) part("campaign-import-drop").classList.remove("is-over");
    });
    on("drop", (event) => {
      if (!event.target?.closest("#campaign-import-drop")) return;
      event.preventDefault();
      part("campaign-import-drop").classList.remove("is-over");
      const file = event.dataTransfer?.files[0];
      if (file) void takeFile(file);
    });
    on("keydown", (event) => {
      const key2 = event.key;
      if (event.target?.id === "campaign-import-drop" && (key2 === "Enter" || key2 === " ")) {
        event.preventDefault();
        part("campaign-import-file").click();
      } else if (key2 === "Escape") leave();
    });
    read().then(
      (loaded) => {
        campaigns = loaded;
        paint();
      },
      () => say("Impossible de lire les campagnes.")
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
  function undeclaredKeys(manifest, payload) {
    const declared = Object.keys(manifest && manifest.payload || {});
    if (!declared.length) return [];
    const id = manifest.target && typeof manifest.target.id === "string" ? manifest.target.id.replace(/^\$+/, "") : null;
    const declares = (key) => declared.some((d) => d === key || d.startsWith(key) && d[key.length] === ".");
    return Object.keys(payload || {}).filter((key) => key !== id && !declares(key)).sort();
  }
  function fieldsOf(manifest, payload) {
    const touches = Array.isArray(manifest && manifest.touches) ? manifest.touches.filter((t) => typeof t === "string") : [];
    return touches.length ? touches.map((t) => t.split(".").pop()) : Object.keys(payload || {});
  }
  function reviewNeeded(aggregate, fields, caller2) {
    const declared = aggregate && aggregate.fields || {};
    const roles = caller2 && Array.isArray(caller2.roles) ? caller2.roles : [];
    return fields.filter((f) => Object.hasOwn(declared, f) && declared[f] && declared[f].criticality === "critical").map((f) => [f, declared[f].review || "role:approver"]).filter(([, role]) => !roles.includes(role)).map(([f, role]) => `${f} (${role})`);
  }
  var undeclaredWarning = (keys2, dataCapability) => `payload key(s) ${keys2.join(", ")} not declared by ${dataCapability}`;
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
    const invariants = async (aggregate, payload) => {
      const out = { reject: [], review: [] };
      if (!aggregate) return out;
      const manifest = await s.aggregate(aggregate);
      for (const inv of manifest && manifest.invariants || []) {
        if (!ruleHolds(inv.rule, payload)) (inv.onViolation === "review" ? out.review : out.reject).push(inv.id);
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
    const command2 = async (body) => {
      const [dataCapability, version] = String(body.dataCapability || "").split("@");
      const payload = body.payload || {};
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
      const derived = Object.keys(payload).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json2({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      const keys2 = known ? undeclaredKeys(known.manifest, payload) : [];
      const warnUndeclared = () => {
        if (!keys2.length) return;
        const warning = undeclaredWarning(keys2, dataCapability);
        entry.warnings.push(warning);
        s.undeclared.push({ commandId: commandId2, dataCapability, keys: keys2, manifest: known.path, warning });
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
      const verdict = await invariants(aggregate, payload);
      if (verdict.reject.length || verdict.review.length) {
        entry.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId2, { violations: verdict.reject.concat(verdict.review) });
      }
      warnUndeclared();
      const needed = reviewNeeded(document2, fieldsOf(known && known.manifest, payload), body.caller);
      const needsReview = verdict.review.length > 0 || needed.length > 0;
      const merging = new Set(Object.keys(declared).filter((f) => declared[f].merge));
      const ahead = declaredAhead(entry, merging);
      if (needsReview) {
        entry.state = "awaiting_review";
        entry.reviewId = `r_${s.next()}`;
        entry.warnings.push(`review ${entry.reviewId} required`);
        const first = fieldsOf(known && known.manifest, payload)[0];
        const approver = first && declared[first] && declared[first].review || "role:approver";
        s.tell("ReviewRequired", approver, commandId2, { actions: ["approve", "reject"], reason: needed.length ? `critical: ${needed.join(", ")}` : "an invariant wants a review" });
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
      if (method === "POST" && url.pathname === "/commands") return command2(body);
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

  // mock/src/CampaignList.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input2, init) => server.fetch(new URL(input2, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
