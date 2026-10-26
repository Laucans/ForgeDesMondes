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
    fetch: (input, init) => fetch(input, init)
  });
  var asStrings = (value) => Array.isArray(value) && value.every((item) => typeof item === "string") ? value : void 0;
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
  function createShell(fetcher = (input, init) => fetch(input, init), page = browserPage()) {
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

  // apps/live/table-sheet/src/ids.ts
  var GET_ENTITY = "campaign.getEntity";
  var LIST_MEDIA = "media.listMedia";
  var SHOW_ON_PLAYER_VIEW = "live.showOnPlayerView";
  var SHOW_WINDOW_ON_PLAYER_VIEW = "live.showWindowOnPlayerView";
  var AGGREGATE = "Diffusion";
  var DATA_CAPABILITY_VERSION = 1;
  var OPEN_EVENT = "fdm:open";
  var OPEN_MAP_EVENT = "fdm:open-map";
  var EDITOR = "fiche-entite.html";

  // apps/live/table-sheet/src/types.ts
  var ABILITIES = ["FOR", "DEX", "CON", "INT", "SAG", "CHA"];

  // apps/live/table-sheet/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var num = (value) => typeof value === "number" && Number.isFinite(value) ? value : null;
  var TYPES = ["character", "place", "event", "illustration"];
  function parseTarget(raw) {
    return { type: TYPES.find((t) => t === raw.type) ?? "", id: text(raw.id) };
  }
  function parseProps(raw) {
    return { ...parseTarget(raw), campaign: text(raw.campaign) };
  }
  var KINDS = /* @__PURE__ */ new Map([
    ["pj", "character"],
    ["pnj", "character"],
    ["lieu", "place"],
    ["evenement", "event"]
  ]);
  function scores(value) {
    const given = isRecord(value) ? value : {};
    return Object.fromEntries(ABILITIES.map((k) => [k, num(given[k])]));
  }
  function features(value) {
    if (!Array.isArray(value)) return [];
    return value.filter(isRecord).map((f) => ({ name: text(f.name), text: text(f.text) })).filter((f) => f.name !== "" || f.text !== "");
  }
  function parseRefs(value) {
    if (!Array.isArray(value)) return [];
    const ids = value.map((c) => isRecord(c) ? text(c.id) : text(c)).filter((id) => id !== "");
    return [...new Set(ids)];
  }
  function parseEntity(data, asked) {
    if (!isRecord(data) || text(data.id) === "") return null;
    const kind = text(data.kind);
    const type = KINDS.get(kind) ?? (asked.type === "" || asked.type === "illustration" ? isRecord(data.stats) ? "character" : "place" : asked.type);
    const name = text(data.name);
    return {
      id: text(data.id),
      name,
      aliases: Array.isArray(data.aliases) ? data.aliases.filter((a) => typeof a === "string" && a !== "") : [],
      type,
      pc: kind === "pj" || kind === "" && text(data.player) !== "",
      description: text(data.description),
      notes: text(data.notes),
      behavior: text(data.behavior),
      background: text(data.background),
      creature: text(data.type),
      stats: scores(data.stats),
      modifiers: scores(data.modifiers),
      ca: num(data.ca),
      pvMax: num(data.pvMax),
      speed: typeof data.speed === "number" ? String(data.speed) : text(data.speed),
      features: features(data.features),
      art: text(data.art),
      initials: text(data.initials) || name.charAt(0),
      illustration: text(data.illustration),
      characters: parseRefs(data.characters),
      campaign: text(data.campaignId) || text(data.campaign)
    };
  }
  function refOf(data) {
    const entity = parseEntity(data, { type: "character", id: "" });
    return entity === null || entity.name === "" ? null : { id: entity.id, name: entity.name, pc: entity.pc };
  }
  function parseMedia(data) {
    const answer = isRecord(data) ? data : {};
    const illustrations = [];
    if (Array.isArray(answer.illustrations)) {
      for (const entry of answer.illustrations) {
        if (isRecord(entry) && text(entry.id) !== "") {
          illustrations.push({ id: text(entry.id), title: text(entry.title), art: text(entry.art), glyph: text(entry.glyph), place: text(entry.place), file: text(entry.file), size: text(entry.size) });
        }
      }
    }
    return { illustrations };
  }
  var illustrationOf = (media, id) => media.illustrations.find((i) => i.id === id);
  function illustrationFor(entity, media) {
    const linked = illustrationOf(media, entity.illustration);
    if (linked !== void 0 || entity.type !== "place") return linked;
    return media.illustrations.find((i) => i.place === entity.id);
  }
  function modifierText(score, given) {
    if (score === null && given === null) return "";
    const value = given ?? Math.floor(((score ?? 10) - 10) / 2);
    return value < 0 ? `\u2212${Math.abs(value)}` : `+${value}`;
  }
  function commandBody(dataCapability, payload) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, target: { aggregate: AGGREGATE }, payload, caller: { by: "dm", from: "TableSheet", roles: [] } };
  }
  var VIOLATIONS = {
    "base-coherent": "l\u2019illustration n\u2019est pas dans la m\xE9diath\xE8que de la campagne",
    "window-proportional": "la fen\xEAtre doit garder les proportions de l\u2019image",
    "window-fields-together": "la fen\xEAtre ne peut pas s\u2019ouvrir",
    "single-diffusion-source": "la diffusion a chang\xE9 entre-temps",
    "unhandled-data-capability": "commande inconnue du serveur"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  var isFinal = (status) => FINAL.has(status);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    const violations = Array.isArray(result.violations) ? result.violations.filter((v) => typeof v === "string") : [];
    return { status: text(result.status), violations };
  }
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/live/table-sheet/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  function art(image) {
    const name = /^[a-z]+$/.test(image.art) ? image.art : "night";
    return `<div class="art art--${name}" role="img" aria-label="${esc(image.title)}">${image.glyph === "" ? "" : `<span class="art__glyph">${esc(image.glyph)}</span>`}</div>`;
  }
  function skeleton() {
    return `<article class="mf-sheet" id="table-sheet" aria-labelledby="sheet-title" data-reads="${GET_ENTITY}"><header class="mf-sheet__head" id="sheet-head" data-reads="${LIST_MEDIA}"></header><div class="mf-sheet__body" id="sheet-body"></div><p class="mf-sheet__status" id="sheet-status" role="status" hidden></p></article>`;
  }
  function notice(id, message) {
    return `<p class="muted" id="${id}" role="status">${esc(message)}</p>`;
  }
  function badges(items) {
    return `<div class="kit-badge">${items.map((b) => `<span class="${b.tone}">${esc(b.label)}</span>`).join("")}</div>`;
  }
  function buttons(view) {
    const items = [];
    if (view.editable) items.push(`<a class="ghost" id="sheet-edit" href="${EDITOR}?id=${encodeURIComponent(view.id)}">Modifier</a>`);
    if (view.toMap) items.push('<button type="button" class="secondary" id="sheet-to-map" data-action="to-map">Placer sur la carte</button>');
    if (view.sends !== void 0) {
      const media = esc(view.sends.id);
      items.push(`<button type="button" class="secondary" id="sheet-send-scene" data-action="send-scene" data-media="${media}" data-writes="${SHOW_ON_PLAYER_VIEW}">Envoyer l\u2019illustration \u2014 sc\xE8ne</button>`);
      items.push(`<button type="button" class="ghost" id="sheet-send-window" data-action="send-window" data-media="${media}" data-writes="${SHOW_WINDOW_ON_PLAYER_VIEW}">Envoyer \u2014 fen\xEAtre</button>`);
    }
    return `<div class="kit-button kit-button--bare kit-button--small">${items.join("")}</div>`;
  }
  function sheet(view) {
    const head = `<div class="mf-sheet__art">${art(view.picture)}</div><div class="mf-sheet__title">${badges(view.badges)}<h2 id="sheet-title">${esc(view.title)}</h2>` + (view.aliases.length > 0 ? `<p class="muted">Alias : ${esc(view.aliases.join(", "))}</p>` : "") + `${buttons(view)}</div>`;
    const body = `<div class="mf-sheet__text" id="sheet-text">${view.text}</div><div id="sheet-side">${view.side}</div>`;
    return { head, body };
  }
  var paragraph = (value) => value === "" ? "" : `<p>${esc(value)}</p>`;
  var vital = (label, value) => `<div class="kit-statblock__vital"><dt>${label}</dt><dd>${esc(value === "" ? "\u2014" : value)}</dd></div>`;
  function statBlock(entity) {
    const abilities = ABILITIES.map((k) => {
      const score = entity.stats[k];
      return `<li class="kit-statblock__ability"><span class="kit-statblock__abbr">${k}</span><span class="kit-statblock__score">${score === null ? "\u2014" : score}</span><span class="kit-statblock__mod">${modifierText(score, entity.modifiers[k])}</span></li>`;
    }).join("");
    const features2 = entity.features.map((f) => `<li class="kit-statblock__feature"><strong>${esc(f.name)}.</strong> ${esc(f.text)}</li>`).join("");
    return `<div class="kit-statblock" id="sheet-statblock"><div class="kit-statblock__rule"></div><div class="kit-statblock__body"><p class="kit-statblock__name">${esc(entity.name)}</p><p class="kit-statblock__type">${esc(entity.creature)}</p><dl class="kit-statblock__vitals">${vital("CA", entity.ca === null ? "" : String(entity.ca))}${vital("PV", entity.pvMax === null ? "" : String(entity.pvMax))}${vital("Vitesse", entity.speed)}</dl><ul class="kit-statblock__abilities">${abilities}</ul><ul class="kit-statblock__features">${features2}</ul></div></div>`;
  }
  function meetings(refs) {
    if (refs.length === 0) return "";
    const chips = refs.map((c) => `<button type="button" class="chip chip--${c.pc ? "pj" : "pnj"}" data-open="character:${esc(c.id)}">${esc(c.name)}</button>`).join("");
    return `<div class="mf-sheet__block" id="sheet-place-characters"><h3>On y croise</h3><div class="row">${chips}</div></div>`;
  }
  function viewOf(entity, illustration, met) {
    const base = { id: entity.id, title: entity.name, aliases: entity.aliases, editable: true, sends: illustration };
    if (entity.type === "character") {
      const text2 = paragraph(entity.description) + (entity.behavior === "" ? "" : `<div class="mf-sheet__block mf-sheet__play" id="sheet-behavior"><h3>Pour l\u2019interpr\xE9ter</h3><p>${esc(entity.behavior)}</p></div>`) + (entity.background === "" ? "" : `<details id="sheet-background"><summary>Background</summary><p>${esc(entity.background)}</p></details>`) + (entity.notes === "" ? "" : `<div class="mf-sheet__block mf-sheet__notes" id="sheet-notes"><h3>Notes du MJ</h3><p>${esc(entity.notes)}</p></div>`);
      return { ...base, badges: [{ label: entity.pc ? "PJ" : "PNJ", tone: "primary" }], picture: { art: entity.art, glyph: entity.initials, title: entity.name }, text: text2, side: statBlock(entity), toMap: true };
    }
    if (entity.type === "place") {
      const picture = illustration === void 0 ? { art: entity.art, glyph: entity.name.charAt(0), title: entity.name } : { art: illustration.art, glyph: illustration.glyph, title: entity.name };
      return { ...base, badges: [{ label: "Lieu", tone: "accent" }], picture, text: paragraph(entity.description), side: meetings(met), toMap: false };
    }
    return { ...base, badges: [{ label: "\xC9v\xE9nement", tone: "danger" }], picture: { art: "blood", glyph: "!", title: entity.name }, text: paragraph(entity.description), side: "", toMap: false };
  }
  function viewOfIllustration(image) {
    const details = [image.file, image.size].filter((part) => part !== "").join(" \xB7 ");
    return {
      id: image.id,
      title: image.title,
      aliases: [],
      badges: [{ label: "Illustration", tone: "accent" }],
      picture: { art: image.art, glyph: image.glyph, title: image.title },
      text: details === "" ? "" : `<p class="muted">${esc(details)}</p>`,
      side: "",
      editable: false,
      sends: image,
      toMap: false
    };
  }

  // apps/live/table-sheet/src/style.ts
  var STYLE = `
.mf-sheet { box-sizing: border-box; display: grid; gap: var(--space-4); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-sheet *, .mf-sheet *::before, .mf-sheet *::after { box-sizing: inherit; }
.mf-sheet h2, .mf-sheet h3, .mf-sheet p { margin: 0; }
.mf-sheet .muted { color: var(--color-text-muted); }
.mf-sheet .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-sheet .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-sheet .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-sheet .chip--pj { border-color: var(--color-success); }
.mf-sheet .chip--pnj { border-color: var(--color-secondary); }
.mf-sheet .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-sheet .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-sheet .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-sheet .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-sheet .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-sheet .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-sheet .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-sheet .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-sheet .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-sheet .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-sheet .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-sheet .kit-button--small button, .mf-sheet .kit-button--small a { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-sheet .kit-button a { font: inherit; font-weight: 600; letter-spacing: .04em; padding: var(--space-2) var(--space-4); border-radius: var(--radius-pill); border: var(--border-width) solid transparent; text-decoration: none; cursor: pointer; }
.mf-sheet .kit-button a.ghost { background: transparent; color: var(--color-text); }
.mf-sheet .kit-button a.ghost:hover { background: var(--color-surface-2); color: var(--color-accent); }
.mf-sheet .kit-button button:disabled { cursor: progress; }
.mf-sheet .kit-statblock { max-width: none; }
.mf-sheet__head { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-4); align-items: center; }
.mf-sheet__art { width: calc(var(--space-5) * 3); height: calc(var(--space-5) * 3); border-radius: var(--radius-large); overflow: hidden; box-shadow: var(--shadow-high); }
.mf-sheet__title { display: grid; gap: var(--space-2); }
.mf-sheet__title h2 { font-size: var(--font-size-heading); color: var(--color-primary); }
.mf-sheet__body { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: var(--space-4); align-items: start; }
.mf-sheet__text { display: grid; gap: var(--space-3); font-size: var(--font-size-large); line-height: 1.55; }
.mf-sheet__block { display: grid; gap: var(--space-1); }
.mf-sheet__block h3 { color: var(--color-accent); font-size: var(--font-size-small); }
.mf-sheet__play { padding: var(--space-3); background: var(--color-surface); border-left: calc(var(--border-width) * 4) solid var(--color-primary); border-radius: var(--radius-medium); }
.mf-sheet__notes { padding: var(--space-3); background: var(--color-surface); border: var(--border-width) dashed var(--color-accent); border-radius: var(--radius-medium); font-size: var(--font-size-base); }
.mf-sheet details { font-size: var(--font-size-base); color: var(--color-text-muted); }
.mf-sheet summary { cursor: pointer; color: var(--color-accent); }
.mf-sheet__status { padding: var(--space-1) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-accent); border-radius: var(--radius-medium); color: var(--color-text); font-size: var(--font-size-small); }
.mf-sheet__status[hidden] { display: none; }
@media (max-width: 1100px) { .mf-sheet__body { grid-template-columns: minmax(0, 1fr); } }
`;

  // apps/live/table-sheet/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60 };
  var mounts = /* @__PURE__ */ new WeakMap();
  var UNREAD = "unread";
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="TableSheet"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "TableSheet";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, raw, shell2, timing = WAITING) {
    const doc = host2.ownerDocument;
    ensureStyle(doc);
    let props2 = parseProps(raw);
    let shown2 = { type: props2.type, id: props2.id };
    let campaign = props2.campaign;
    let image;
    let generation = 0;
    let sending = false;
    const part = (id) => host2.querySelector(`#${id}`);
    function say(message) {
      const box = part("sheet-status");
      if (!box) return;
      box.textContent = message;
      box.hidden = message === "";
    }
    function busy(on) {
      for (const button of host2.querySelectorAll("button[data-writes]")) button.disabled = on;
    }
    function put(head, body) {
      const [headBox, bodyBox] = [part("sheet-head"), part("sheet-body")];
      if (headBox) headBox.innerHTML = head;
      if (bodyBox) bodyBox.innerHTML = body;
    }
    async function met(ids) {
      const read = await Promise.all(ids.map((id) => shell2.capability(GET_ENTITY, void 0, { id }).then((answer) => refOf(answer.data), () => null)));
      return read.filter((ref) => ref !== null);
    }
    async function load(target) {
      generation += 1;
      const mine = generation;
      sending = false;
      image = void 0;
      campaign = props2.campaign;
      host2.innerHTML = skeleton();
      if (target.id === "") {
        put(notice("sheet-empty", "Aucun \xE9l\xE9ment ouvert."), "");
        return;
      }
      try {
        const entity = target.type === "illustration" ? null : parseEntity((await shell2.capability(GET_ENTITY, void 0, { id: target.id })).data, target);
        if (mine !== generation) return;
        if (target.type !== "illustration" && entity === null) {
          put(notice("sheet-missing", "\xC9l\xE9ment introuvable."), "");
          return;
        }
        campaign = props2.campaign || entity?.campaign || "";
        const named = campaign;
        const [media, characters] = await Promise.all([
          shell2.capability(LIST_MEDIA, void 0, named === "" ? {} : { campaign: named }).then(
            (answer) => parseMedia(answer.data),
            (error) => {
              if (entity === null) throw error;
              return { illustrations: [] };
            }
          ),
          entity?.type === "place" ? met(entity.characters) : Promise.resolve([])
        ]);
        if (mine !== generation) return;
        let view;
        if (entity === null) {
          const picked = illustrationOf(media, target.id);
          if (picked === void 0) {
            put(notice("sheet-missing", "\xC9l\xE9ment introuvable."), "");
            return;
          }
          view = viewOfIllustration(picked);
        } else {
          view = viewOf(entity, illustrationFor(entity, media), characters);
        }
        image = view.sends;
        const { head, body } = sheet(view);
        put(head, body);
      } catch {
        if (mine === generation) put(notice("sheet-error", "Impossible de lire la fiche."), "");
      }
    }
    async function submit(body) {
      const answer = await shell2.command(body);
      const id = commandId(answer);
      let state = settled(answer);
      try {
        for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
          await timing.sleep(timing.everyMs);
          state = settled(await shell2.commandResult(id));
        }
      } catch {
        return { status: UNREAD, violations: [] };
      }
      return state;
    }
    async function send(mode, media) {
      const picked = image;
      if (sending || picked === void 0 || picked.id !== media) return;
      if (mode === "window" && campaign === "") {
        say("La campagne n\u2019est pas connue : l\u2019illustration ne peut pas \xEAtre envoy\xE9e en fen\xEAtre.");
        return;
      }
      const named = campaign === "" ? {} : { campaign };
      const body = mode === "scene" ? commandBody(SHOW_ON_PLAYER_VIEW, { ...named, kind: "illustration", media }) : commandBody(SHOW_WINDOW_ON_PLAYER_VIEW, { ...named, media });
      const mine = generation;
      sending = true;
      busy(true);
      say("");
      let message;
      try {
        const state = await submit(body);
        if (state.status === "applied") message = `\xAB ${picked.title} \xBB est envoy\xE9e aux joueurs, en mode ${mode === "scene" ? "sc\xE8ne" : "fen\xEAtre"}.`;
        else if (state.status === UNREAD) message = "L\u2019envoi est parti, mais son \xE9tat n\u2019a pas pu \xEAtre lu.";
        else if (state.status === "rejected") message = `Envoi refus\xE9 : ${reasons(state.violations)}.`;
        else message = `La commande n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`;
      } catch {
        message = "La commande n\u2019a pas pu \xEAtre envoy\xE9e.";
      }
      if (mine !== generation) return;
      sending = false;
      busy(false);
      say(message);
    }
    const raise = (name, detail) => {
      const win = doc.defaultView;
      if (win) doc.dispatchEvent(new win.CustomEvent(name, { detail }));
    };
    const onClick = (event) => {
      const button = event.target?.closest("button");
      if (!button || !host2.contains(button)) return;
      const { open, action, media } = button.dataset;
      if (open !== void 0) {
        const at = open.indexOf(":");
        if (at > 0) raise(OPEN_EVENT, { type: open.slice(0, at), id: open.slice(at + 1) });
      } else if (action === "to-map") raise(OPEN_MAP_EVENT, { place: shown2.id });
      else if (action === "send-scene" || action === "send-window") void send(action === "send-scene" ? "scene" : "window", media ?? "");
    };
    const onOpen = (event) => {
      if (!host2.isConnected) {
        doc.removeEventListener(OPEN_EVENT, onOpen);
        return;
      }
      const detail = event.detail;
      const target = parseTarget(typeof detail === "object" && detail !== null ? detail : {});
      if (target.id === "") return;
      shown2 = target;
      void load(shown2);
    };
    const destroy = () => {
      generation += 1;
      host2.removeEventListener("click", onClick);
      doc.removeEventListener(OPEN_EVENT, onOpen);
    };
    mounts.get(host2)?.();
    mounts.set(host2, destroy);
    host2.addEventListener("click", onClick);
    doc.addEventListener(OPEN_EVENT, onOpen);
    void load(shown2);
    return {
      destroy,
      update(next) {
        const before = props2;
        props2 = parseProps(next);
        if (props2.type === before.type && props2.id === before.id && props2.campaign === before.campaign) return;
        shown2 = { type: props2.type, id: props2.id };
        void load(shown2);
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
    const command = async (body) => {
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
      const keys = known ? undeclaredKeys(known.manifest, payload) : [];
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
        const first2 = fieldsOf(known && known.manifest, payload)[0];
        const approver = first2 && declared[first2] && declared[first2].review || "role:approver";
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
    s.fetch = async (input, init) => {
      try {
        await s.ready;
        if (s.problems.length) return json2({ error: `the mock server did not start: ${s.problems.join("; ")}` }, 500);
        return await respond(input, init);
      } catch (e) {
        if (!(e instanceof HandlerError)) throw e;
        console.error(e.message);
        return json2({ error: e.message }, 500);
      }
    };
    const respond = async (input, init) => {
      await delay(LATENCY_MS);
      const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url);
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

  // mock/src/TableSheet.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var first = { type: "place", id: "taverne-sanglier" };
  compose(props, (resolved) => mount(host, resolved.id ? resolved : { ...first, ...resolved }, shell));
})();
