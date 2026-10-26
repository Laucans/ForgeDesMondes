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
    const named2 = page.href ? new URL(page.href, "http://shell").searchParams.get("campaign") : null;
    if (named2) context2.hold("campaign", named2);
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

  // apps/live/elements-panel/src/ids.ts
  var LIST_SCENES = "session.listScenes";
  var LIST_CHARACTERS = "campaign.listCharacters";
  var LIST_PLACES = "campaign.listPlaces";
  var LIST_EVENTS = "campaign.listEvents";
  var LIST_MEDIA = "media.listMedia";
  var SHOW_ON_PLAYER_VIEW = "live.showOnPlayerView";
  var SHOW_WINDOW_ON_PLAYER_VIEW = "live.showWindowOnPlayerView";
  var AGGREGATE = "Diffusion";
  var DATA_CAPABILITY_VERSION = 1;
  var SESSION_CONTEXT = "session";
  var CAMPAIGN_CONTEXT = "campaign";
  var OPEN_EVENT = "fdm:open";
  var OPEN_MAP_EVENT = "fdm:open-map";
  var SCENE_EVENT = "fdm:scene";
  var CHARACTER_DRAG_TYPE = "text/fdm-character";

  // apps/live/elements-panel/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((v) => typeof v === "string") : [];
  var records = (value) => Array.isArray(value) ? value.filter(isRecord) : [];
  var ACCENTS = /[\u0300-\u036f]/g;
  function normalize(term) {
    return term.toLowerCase().normalize("NFD").replace(ACCENTS, "").normalize("NFC").trim();
  }
  function distance(a, b) {
    const row2 = Array.from({ length: b.length + 1 }, (_, i) => i);
    for (let i = 1; i <= a.length; i += 1) {
      let previous = row2[0] ?? 0;
      row2[0] = i;
      for (let j = 1; j <= b.length; j += 1) {
        const above = row2[j] ?? 0;
        row2[j] = Math.min(above + 1, (row2[j - 1] ?? 0) + 1, previous + (a[i - 1] === b[j - 1] ? 0 : 1));
        previous = above;
      }
    }
    return row2[b.length] ?? 0;
  }
  var WORDS = /[^\p{L}\p{N}]+/u;
  var wordsOf = (term) => term.split(WORDS).filter((w) => w.length > 2);
  function score(item, query) {
    const term = normalize(query);
    if (term === "") return 0;
    const names = [item.name, ...item.aliases].map(normalize);
    if (names.some((n) => n.includes(term))) return 2;
    const known = wordsOf(names.join(" "));
    const asked = wordsOf(term);
    return asked.length > 0 && asked.every((w) => known.some((k) => distance(w, k) <= (w.length > 5 ? 2 : 1))) ? 1 : 0;
  }
  function search(items, query) {
    return items.map((item) => ({ item, s: score(item, query) })).filter((found) => found.s > 0).sort((a, b) => b.s - a.s).map((found) => found.item);
  }
  function entity(row2, type) {
    const id = text(row2.id);
    if (id === "") return null;
    const kind = row2.kind === "pj" || row2.kind === "pnj" ? row2.kind : void 0;
    return { type, id, name: text(row2.name), aliases: texts(row2.aliases), ...kind ? { kind } : {} };
  }
  function entities(data, type) {
    return records(data).flatMap((row2) => {
      const item = entity(row2, type);
      return item === null ? [] : [item];
    });
  }
  var parseCharacters = (data) => entities(data, "character");
  var parsePlaces = (data) => entities(data, "place");
  var parseEvents = (data) => entities(data, "event");
  function parseScenes(data) {
    return records(data).flatMap((row2) => {
      const id = text(row2.id);
      if (id === "") return [];
      const battlemap = text(row2.battlemap);
      return [{ id, title: text(row2.title), places: texts(row2.places), characters: texts(row2.characters), events: texts(row2.events), media: texts(row2.media), battlemap: battlemap === "" ? null : battlemap }];
    });
  }
  function media(data) {
    return records(data).flatMap((row2) => text(row2.id) === "" ? [] : [{ id: text(row2.id), title: text(row2.title) }]);
  }
  function parseLibrary(data) {
    const answer = isRecord(data) ? data : {};
    return { battlemaps: media(answer.battlemaps), illustrations: media(answer.illustrations) };
  }
  var medium = (m, type) => ({ type, id: m.id, name: m.title, aliases: [] });
  function mapOf(scene, library) {
    const maps = scene.media.filter((id) => library.battlemaps.some((m) => m.id === id));
    return maps.length === 1 ? maps[0] : void 0;
  }
  function instances(world) {
    const held = /* @__PURE__ */ new Map();
    for (const scene of world.scenes) {
      const map = mapOf(scene, world.library);
      if (map !== void 0 && scene.battlemap !== null && !held.has(map)) held.set(map, scene.battlemap);
    }
    return held;
  }
  function buildIndex(world) {
    const held = instances(world);
    return [
      ...world.characters,
      ...world.places,
      ...world.events,
      ...world.library.battlemaps.map((m) => {
        const instance = held.get(m.id);
        return instance === void 0 ? medium(m, "battlemap") : { ...medium(m, "battlemap"), instance };
      }),
      ...world.library.illustrations.map((m) => medium(m, "illustration"))
    ];
  }
  var named = (index, type, ids) => ids.flatMap((id) => {
    const found = index.find((item) => item.type === type && item.id === id);
    return found === void 0 ? [] : [found];
  });
  function sceneGroups(scene, world, index) {
    const map = mapOf(scene, world.library);
    const library = scene.media.flatMap((id) => index.filter((item) => item.id === id && (item.type === "battlemap" || item.type === "illustration")));
    const own = library.map((item) => item.type === "battlemap" ? item.id === map && scene.battlemap !== null ? { ...item, instance: scene.battlemap } : item : item);
    const loose = map === void 0 && scene.battlemap !== null ? [{ type: "battlemap", id: scene.battlemap, name: "Battlemap de la sc\xE8ne", aliases: [], instance: scene.battlemap }] : [];
    const groups2 = [
      { key: "lieux", label: "Lieux", items: named(index, "place", scene.places) },
      { key: "pnj", label: "PNJ", items: named(index, "character", scene.characters).filter((c) => c.kind === "pnj") },
      { key: "evenements", label: "\xC9v\xE9nements", items: named(index, "event", scene.events) },
      { key: "medias", label: "M\xE9dias", items: [...loose, ...own] }
    ];
    return groups2.filter((g) => g.items.length > 0);
  }
  function campaignGroups(index) {
    const of = (type, kind) => index.filter((item) => item.type === type && (kind === void 0 || item.kind === kind));
    return [
      { key: "pj", label: "PJ", items: of("character", "pj") },
      { key: "pnj", label: "PNJ", items: of("character", "pnj") },
      { key: "lieux", label: "Lieux", items: of("place") },
      { key: "evenements", label: "\xC9v\xE9nements", items: of("event") },
      { key: "medias", label: "M\xE9dias", items: [...of("battlemap"), ...of("illustration")] }
    ];
  }
  function commandBody(dataCapability, payload, session) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      target: session === "" ? { aggregate: AGGREGATE } : { aggregate: AGGREGATE, id: session },
      payload,
      caller: { by: "dm", from: "ElementsPanel", roles: [] }
    };
  }
  var VIOLATIONS = {
    "base-coherent": "cette carte ou cette illustration n\u2019est pas celle de la s\xE9ance",
    "window-proportional": "cette image ne peut pas \xEAtre ouverte en fen\xEAtre",
    "window-fields-together": "la fen\xEAtre n\u2019a pas pu \xEAtre ouverte",
    "single-diffusion-source": "la diffusion a chang\xE9 entre-temps",
    "unhandled-data-capability": "commande inconnue du serveur"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  var isFinal = (status) => FINAL.has(status);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    return { status: text(result.status), violations: texts(result.violations) };
  }
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/live/elements-panel/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  function skeleton() {
    return `<nav class="mf-ep" id="elements-panel" aria-label="\xC9l\xE9ments de la campagne"><div id="elements-search"><div class="kit-input"><label>Rechercher<input type="search" id="elements-search-input" placeholder="Nom ou alias, m\xEAme approximatif" autocomplete="off"></label></div></div><p class="mf-ep__status" id="elements-status" role="status" hidden></p><section id="elements-results" aria-labelledby="elements-results-title" hidden><h2 id="elements-results-title">R\xE9sultats</h2><ul class="mf-ep__list" id="elements-results-list"></ul></section><section id="elements-session" aria-labelledby="elements-session-title"><h2 id="elements-session-title">S\xE9ance en cours</h2><div id="elements-scenes" data-reads="${LIST_SCENES} ${LIST_CHARACTERS} ${LIST_MEDIA}"></div></section><section id="elements-campaign" aria-labelledby="elements-campaign-title"><h2 id="elements-campaign-title">Toute la campagne</h2><div id="elements-groups" data-reads="${LIST_CHARACTERS} ${LIST_PLACES} ${LIST_EVENTS} ${LIST_MEDIA}"></div></section></nav>`;
  }
  function actions(item) {
    if (item.type === "battlemap") {
      if (item.instance === void 0) return "";
      const instance = esc(item.instance);
      return `<button type="button" class="chip" data-open-map="${instance}">Ouvrir</button><button type="button" class="chip" data-writes="${SHOW_ON_PLAYER_VIEW}" data-send-map="${instance}">Envoyer</button>`;
    }
    if (item.type === "illustration") {
      const id = esc(item.id);
      return `<button type="button" class="chip" data-writes="${SHOW_ON_PLAYER_VIEW}" data-send-scene="${id}" title="Envoyer en plein \xE9cran">Sc\xE8ne</button><button type="button" class="chip" data-writes="${SHOW_WINDOW_ON_PLAYER_VIEW}" data-send-window="${id}" title="Envoyer dans une fen\xEAtre flottante">Fen\xEAtre</button>`;
    }
    return "";
  }
  function opener(item) {
    const label = `${esc(item.name)}${item.kind === "pj" ? "<small>PJ</small>" : ""}`;
    if (item.type === "battlemap") {
      return item.instance === void 0 ? `<button type="button" class="mf-ep__open" disabled title="Aucune instance de cette carte dans la s\xE9ance">${label}</button>` : `<button type="button" class="mf-ep__open" data-open-map="${esc(item.instance)}">${label}</button>`;
    }
    return `<button type="button" class="mf-ep__open" data-open="${esc(item.type)}:${esc(item.id)}">${label}</button>`;
  }
  function row(item, prefix) {
    const drag = item.type === "character";
    return `<li class="mf-ep__item" id="${esc(`${prefix}-${item.type}-${item.id}`)}"${drag ? ` draggable="true" data-char="${esc(item.id)}"` : ""}>${drag ? '<span class="mf-ep__grip" aria-hidden="true">\u283F</span>' : ""}${opener(item)}${actions(item)}</li>`;
  }
  var list = (items, prefix) => `<ul class="mf-ep__list">${items.map((item) => row(item, prefix)).join("")}</ul>`;
  var results = (items) => items.map((item) => row(item, "found")).join("");
  function scenes(all, groupsOf, current) {
    if (all.length === 0) return '<p class="mf-ep__empty">Aucune sc\xE8ne dans la s\xE9ance.</p>';
    return all.map((scene, i) => {
      const groups2 = groupsOf(scene).map((g) => `<div class="mf-ep__group"><h3>${esc(g.label)}</h3>${list(g.items, scene.id)}</div>`).join("");
      return `<details id="elements-scene-${esc(scene.id)}" data-scene-index="${i}"${i === current ? ' open class="is-current"' : ""}><summary>Sc\xE8ne ${i + 1} \xB7 ${esc(scene.title)}</summary>${groups2}</details>`;
    }).join("");
  }
  var groups = (all) => all.map((g) => `<details id="elements-group-${esc(normalize(g.label))}"><summary>${esc(g.label)} <small>${g.items.length}</small></summary><div class="mf-ep__group">${list(g.items, "all")}</div></details>`).join("");

  // apps/live/elements-panel/src/style.ts
  var STYLE = `
.mf-ep { box-sizing: border-box; min-width: 0; max-width: 100%; display: grid; gap: var(--space-3); align-content: start; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-ep *, .mf-ep *::before, .mf-ep *::after { box-sizing: inherit; }
.mf-ep > * { min-width: 0; }
.mf-ep h2, .mf-ep h3, .mf-ep p { margin: 0; }
.mf-ep h2 { font-size: var(--font-size-base); }
.mf-ep .kit-input { max-width: none; }
.mf-ep .kit-input input { min-width: 0; width: 100%; }
.mf-ep details { min-width: 0; border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); background: var(--color-surface); margin-top: var(--space-2); }
.mf-ep details.is-current { border-color: var(--color-primary); }
.mf-ep summary { cursor: pointer; padding: var(--space-2) var(--space-3); font-weight: var(--font-weight-heading); overflow-wrap: anywhere; }
.mf-ep summary small { display: block; font-weight: normal; color: var(--color-text-muted); }
.mf-ep__group { min-width: 0; padding: 0 var(--space-3) var(--space-2); }
.mf-ep__group h3 { font-size: var(--font-size-small); color: var(--color-text-muted); margin: var(--space-2) 0 var(--space-1); }
.mf-ep__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); min-width: 0; }
.mf-ep__item { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-1); min-width: 0; padding: var(--space-1) var(--space-2); border-radius: var(--radius-small); }
.mf-ep__item:hover { background: var(--color-surface-2); }
.mf-ep__item[draggable="true"] { cursor: grab; }
.mf-ep__open { flex: 1; min-width: 0; text-align: left; background: none; border: 0; padding: 0; color: var(--color-text); font: inherit; cursor: pointer; white-space: normal; overflow-wrap: anywhere; }
.mf-ep__open:hover { color: var(--color-accent); }
.mf-ep__open:disabled { cursor: default; color: var(--color-text-muted); }
.mf-ep__open small { color: var(--color-text-muted); margin-left: var(--space-1); }
.mf-ep .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: 0 var(--space-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; flex: 0 0 auto; white-space: nowrap; }
.mf-ep .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-ep__grip { color: var(--color-text-muted); }
.mf-ep__empty { color: var(--color-text-muted); font-size: var(--font-size-small); }
.mf-ep__status { padding: var(--space-1) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); font-size: var(--font-size-small); overflow-wrap: anywhere; }
.mf-ep__status.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-ep [hidden] { display: none; }
`;

  // apps/live/elements-panel/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60 };
  var EMPTY = { scenes: [], characters: [], places: [], events: [], library: { battlemaps: [], illustrations: [] } };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="ElementsPanel"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "ElementsPanel";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, _raw, shell2, timing = WAITING) {
    const doc = host2.ownerDocument;
    ensureStyle(doc);
    let world = EMPTY;
    let index = [];
    let generation = 0;
    let current = 0;
    let readFailed = false;
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (found === null) throw new Error(`the panel has no #${id}`);
      return found;
    };
    const named2 = (name) => {
      const value = shell2.context?.get(name);
      return typeof value === "string" ? value : "";
    };
    const session = () => named2(SESSION_CONTEXT);
    const campaign = () => named2(CAMPAIGN_CONTEXT);
    function say(message, failed = false) {
      const box = part("elements-status");
      box.textContent = message;
      box.hidden = message === "";
      box.classList.toggle("is-error", failed);
    }
    function emit(name, detail) {
      const view = doc.defaultView;
      if (view) doc.dispatchEvent(new view.CustomEvent(name, { detail }));
    }
    function paintSearch() {
      const query = part("elements-search-input").value;
      const asked = normalize(query) !== "";
      part("elements-results").hidden = !asked;
      part("elements-session").hidden = asked;
      part("elements-campaign").hidden = asked;
      if (!asked) return;
      const found = search(index, query);
      part("elements-results-title").textContent = found.length > 0 ? `${found.length} r\xE9sultat${found.length > 1 ? "s" : ""} pour \xAB ${query.trim()} \xBB` : `Rien pour \xAB ${query.trim()} \xBB`;
      part("elements-results-list").innerHTML = results(found);
    }
    function paint() {
      const unfolded = new Map([...host2.querySelectorAll("#elements-scenes details, #elements-groups details")].map((d) => [d.id, d.open]));
      part("elements-scenes").innerHTML = scenes(world.scenes, (scene) => sceneGroups(scene, world, index), current);
      part("elements-groups").innerHTML = groups(campaignGroups(index));
      host2.querySelectorAll("#elements-scenes details, #elements-groups details").forEach((d) => {
        const was = unfolded.get(d.id);
        if (was !== void 0) d.open = was;
      });
      paintSearch();
    }
    async function start() {
      generation += 1;
      const mine = generation;
      const [forSession, forCampaign] = [session() === "" ? null : { session: session() }, campaign() === "" ? null : { campaign: campaign() }];
      const read = async (id, variables) => variables === null ? [] : (await shell2.capability(id, void 0, variables)).data;
      const answers = await Promise.allSettled([
        read(LIST_SCENES, forSession),
        read(LIST_CHARACTERS, forCampaign),
        read(LIST_PLACES, forCampaign),
        read(LIST_EVENTS, forCampaign),
        read(LIST_MEDIA, forCampaign)
      ]);
      if (mine !== generation) return;
      const [s, c, p, e, m] = answers;
      world = {
        scenes: s?.status === "fulfilled" ? parseScenes(s.value) : world.scenes,
        characters: c?.status === "fulfilled" ? parseCharacters(c.value) : world.characters,
        places: p?.status === "fulfilled" ? parsePlaces(p.value) : world.places,
        events: e?.status === "fulfilled" ? parseEvents(e.value) : world.events,
        library: m?.status === "fulfilled" ? parseLibrary(m.value) : world.library
      };
      index = buildIndex(world);
      if (answers.some((a) => a.status === "rejected")) {
        readFailed = true;
        say("Une partie du panneau n\u2019a pas pu \xEAtre lue.", true);
      } else if (readFailed) {
        readFailed = false;
        say("");
      }
      paint();
    }
    async function submit(body) {
      const answer = await shell2.command(body);
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    async function send(dataCapability, payload, sent) {
      say("Envoi aux joueurs\u2026");
      try {
        const state = await submit(commandBody(dataCapability, campaign() === "" ? payload : { ...payload, campaign: campaign() }, session()));
        if (state.status === "applied") say(sent);
        else if (state.status === "rejected") say(`Pas envoy\xE9 : ${reasons(state.violations)}.`, true);
        else say(`L\u2019envoi n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`, true);
      } catch {
        say("L\u2019envoi n\u2019a pas pu \xEAtre fait.", true);
      }
    }
    const nameOf = (type, id) => index.find((item) => item.type === type && item.id === id)?.name ?? id;
    host2.addEventListener("input", (event) => {
      if (event.target?.id === "elements-search-input") paintSearch();
    });
    host2.addEventListener("dragstart", (event) => {
      const row2 = event.target?.closest("[data-char]");
      const transfer = event.dataTransfer;
      if (!row2 || !transfer) return;
      const id = row2.dataset.char ?? "";
      transfer.setData(CHARACTER_DRAG_TYPE, id);
      transfer.setData("text/plain", nameOf("character", id));
      transfer.effectAllowed = "copy";
      emit(OPEN_MAP_EVENT, {});
    });
    host2.addEventListener("click", (event) => {
      const button = event.target?.closest("button");
      if (!button || button.disabled) return;
      const { open, openMap, sendMap, sendScene, sendWindow } = button.dataset;
      if (open !== void 0) {
        const cut = open.indexOf(":");
        emit(OPEN_EVENT, { type: open.slice(0, cut), id: open.slice(cut + 1) });
      }
      if (openMap !== void 0) emit(OPEN_MAP_EVENT, { id: openMap });
      if (sendMap !== void 0) void send(SHOW_ON_PLAYER_VIEW, { kind: "battlemap", instance: sendMap }, "Battlemap envoy\xE9e aux joueurs.");
      if (sendScene !== void 0) void send(SHOW_ON_PLAYER_VIEW, { kind: "illustration", media: sendScene }, `\xAB ${nameOf("illustration", sendScene)} \xBB envoy\xE9e aux joueurs en sc\xE8ne.`);
      if (sendWindow !== void 0) void send(SHOW_WINDOW_ON_PLAYER_VIEW, { media: sendWindow }, `\xAB ${nameOf("illustration", sendWindow)} \xBB envoy\xE9e aux joueurs en fen\xEAtre.`);
    });
    doc.addEventListener(SCENE_EVENT, (event) => {
      const detail = event.detail;
      if (typeof detail?.index !== "number") return;
      current = detail.index;
      host2.querySelectorAll("#elements-scenes details").forEach((d) => {
        const at = Number(d.dataset.sceneIndex) === current;
        if (at) d.open = true;
        d.classList.toggle("is-current", at);
      });
    });
    shell2.context?.on?.((name) => {
      if (name === SESSION_CONTEXT || name === CAMPAIGN_CONTEXT) void start();
    });
    host2.innerHTML = skeleton();
    paint();
    void start();
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
        const at = (base2) => fetch(new URL(base2 + path, location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0);
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

  // mock/src/ElementsPanel.js
  var { host, props } = mountPoint();
  var base = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var shell = {
    ...base,
    context: {
      get: (name) => getContext(name) ?? { session: "s4", campaign: "cendres-valombre" }[name],
      on: (listener) => onContext(listener)
    }
  };
  mount(host, props, shell);
})();
