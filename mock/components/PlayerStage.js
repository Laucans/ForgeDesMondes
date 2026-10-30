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
      if (this.held.has(name)) {
        if (this.values.get(name) !== value) console.warn(`shell: "${name}" is held by the address, a write of it was ignored`);
        return;
      }
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
  var SETTLING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 100, polls: 50 };
  var ADVANCING = /* @__PURE__ */ new Set(["queued", "confirmed"]);
  var record = (value) => value && typeof value === "object" && !Array.isArray(value) ? value : {};
  var stateOf = (answer) => {
    const result = record(record(answer).result);
    const id = record(answer).commandId ?? result.commandId;
    return { id: typeof id === "string" ? id : "", status: typeof result.status === "string" ? result.status : "" };
  };
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
      try {
        const screen = await product(page, `apps/screens/${mocked.screen}.json`);
        const pilot = screen.pilot;
        surface = pilot && typeof pilot.query === "string" && here.searchParams.get(pilot.query) === "1" ? pilot.surface : screen.surface;
      } catch (error) {
        if (surface === void 0) throw error;
      }
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
    const from = sent && typeof sent === "object" && !Array.isArray(sent) ? sent.from : void 0;
    return { ...body, caller: { ...from === void 0 ? {} : { from }, by: caller.by, roles: caller.roles, permissions: caller.permissions } };
  }
  function createShell(fetcher = (input, init) => fetch(input, init), page = browserPage(), settling = SETTLING) {
    const writing = /* @__PURE__ */ new Set();
    const follow = async (sent) => {
      try {
        const first = stateOf(await sent);
        const { id } = first;
        let { status } = first;
        for (let asked = 0; ADVANCING.has(status) && id !== "" && asked < settling.polls; asked += 1) {
          await settling.sleep(settling.everyMs);
          ({ status } = stateOf(await call(fetcher, "GET", `/commands/${encodeURIComponent(id)}`)));
        }
      } catch {
      }
    };
    const track = (sent) => {
      const settled2 = follow(sent).finally(() => writing.delete(settled2));
      writing.add(settled2);
    };
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
      capability: async (id, sha256, variables = {}) => {
        await Promise.all([...writing]);
        return await call(fetcher, "POST", `/capabilities/${encodeURIComponent(id)}`, { sha256, variables });
      },
      command: (body) => {
        const sent = (async () => call(fetcher, "POST", "/commands", withCaller(body, await callerOf())))();
        track(sent);
        return sent;
      },
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

  // apps/live/player-stage/src/ids.ts
  var GET_DIFFUSION = "live.getDiffusion";
  var GET_CAMPAIGN = "campaign.getCampaign";
  var GET_LIVE_SESSION = "session.getLiveSession";
  var LIST_MEDIA = "media.listMedia";
  var MOVE_WINDOW = "live.moveWindow";
  var RESIZE_WINDOW = "live.resizeWindow";
  var AGGREGATE = "Diffusion";
  var BATTLEMAP = "Battlemap";
  var BATTLEMAP_CONTEXT = "battlemap";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/live/player-stage/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var num = (value) => typeof value === "number" && Number.isFinite(value) ? value : null;
  var MIN_WIDTH = 0.1;
  var KINDS = ["cover", "illustration", "battlemap"];
  var COVER = { kind: "cover", id: "" };
  function base(value) {
    if (!isRecord(value)) return COVER;
    const kind = KINDS.find((k) => k === value.kind);
    return kind === void 0 ? COVER : { kind, id: text(value.id) };
  }
  function windowOf(value) {
    if (!isRecord(value)) return null;
    const [x, y, w] = [num(value.x), num(value.y), num(value.w)];
    const media = text(value.media) || text(value.id);
    if (media === "" || x === null || y === null || w === null || w <= 0) return null;
    const width = Math.min(w, 1);
    return { media, x: Math.max(0, Math.min(x, 1 - width)), y: Math.max(0, Math.min(y, 1)), w: width };
  }
  function framing(value) {
    if (!isRecord(value)) return null;
    const [zoom, panX, panY] = [num(value.zoom ?? value.z), num(value.panX ?? value.px), num(value.panY ?? value.py)];
    return zoom === null || panX === null || panY === null ? null : { zoom, panX, panY };
  }
  function parseDiffusion(data) {
    const answer = isRecord(data) ? data : {};
    return { base: base(answer.base), window: windowOf(answer.window), view: framing(answer.view) };
  }
  function parseCampaign(data) {
    if (!isRecord(data) || text(data.name) === "") return null;
    const next = isRecord(data.nextSession) ? data.nextSession : null;
    const order = next === null ? null : num(next.order);
    return {
      id: text(data.id),
      name: text(data.name),
      cover: text(data.cover),
      art: text(data.art),
      glyph: text(data.glyph),
      session: next !== null && order !== null && text(next.title) !== "" ? { order, title: text(next.title) } : null
    };
  }
  function parseLiveCampaign(data) {
    const session = isRecord(data) && isRecord(data.session) ? data.session : null;
    return session === null ? "" : text(session.campaign);
  }
  function ratioOf(entry) {
    const [width, height] = [num(entry.width), num(entry.height)];
    if (width !== null && height !== null && width > 0 && height > 0) return width / height;
    const ratio = num(entry.ratio);
    return ratio !== null && ratio > 0 ? ratio : 1;
  }
  function parseMedia(data) {
    const answer = isRecord(data) ? data : {};
    const illustrations = [];
    if (Array.isArray(answer.illustrations)) {
      for (const entry of answer.illustrations) {
        if (isRecord(entry) && text(entry.id) !== "") {
          illustrations.push({ id: text(entry.id), title: text(entry.title), art: text(entry.art), glyph: text(entry.glyph), ratio: ratioOf(entry) });
        }
      }
    }
    return { cover: text(answer.cover), illustrations };
  }
  function parseProps(raw, search) {
    return { pilot: raw.pilot === true || new URLSearchParams(search).get("pilote") === "1", campaign: text(raw.campaign) };
  }
  var illustrationOf = (media, id) => media.illustrations.find((i) => i.id === id);
  var clamp = (value, low, high) => Math.max(low, Math.min(high, value));
  var heightOf = (box, ratio, stage) => box.w * stage.width / ratio / stage.height;
  function moved(box, dx, dy, ratio, stage) {
    const height = heightOf(box, ratio, stage);
    return { ...box, x: clamp(box.x + dx / stage.width, 0, Math.max(0, 1 - box.w)), y: clamp(box.y + dy / stage.height, 0, Math.max(0, 1 - height)) };
  }
  var widest = (box, ratio, stage) => Math.max(MIN_WIDTH, Math.min(1 - box.x, (1 - box.y) * stage.height * ratio / stage.width));
  function dragged(box, dx, dy, ratio, stage) {
    const [w, h] = [box.w + dx / stage.width, box.w / ratio + dy / stage.width];
    const limit = widest(box, ratio, stage);
    return { w: clamp(w, MIN_WIDTH, limit), h: clamp(h, MIN_WIDTH / ratio, limit / ratio) };
  }
  function resized(box, drag, ratio, stage) {
    return { ...box, w: clamp(Math.min(drag.w, drag.h * ratio), MIN_WIDTH, widest(box, ratio, stage)) };
  }
  function commandBody(dataCapability, payload) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, target: { aggregate: AGGREGATE }, payload, caller: { by: "dm", from: "PlayerStage", roles: [] } };
  }
  var VIOLATIONS = {
    "window-fields-together": "aucune fen\xEAtre n\u2019est ouverte",
    "window-proportional": "la fen\xEAtre doit garder les proportions de l\u2019image",
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

  // apps/live/player-stage/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  function art(image) {
    const name = /^[a-z]+$/.test(image.art) ? image.art : "ember";
    return `<div class="art art--${name}" role="img" aria-label="${esc(image.title)}">${image.glyph === "" ? "" : `<span class="art__glyph">${esc(image.glyph)}</span>`}</div>`;
  }
  function skeleton(pilot) {
    const bar = pilot ? `<figcaption class="pv-window__bar" id="player-window-bar" data-writes="${MOVE_WINDOW}"></figcaption>` : "";
    const corner = `<span class="pv-window__resize" id="player-window-resize" data-writes="${RESIZE_WINDOW}" aria-hidden="true"></span>`;
    const badge = pilot ? '<p class="pv-pilot" id="player-pilot-badge">Vue pilot\xE9e par le MJ \xB7 d\xE9placez les tokens et la fen\xEAtre, zoomez la carte</p>' : "";
    return `<div class="pv${pilot ? " pv--pilot" : ""}" id="player-stage" data-reads="${GET_LIVE_SESSION}"><div class="pv-base" id="player-base" data-reads="${GET_DIFFUSION}"></div><figure class="pv-window" id="player-window" hidden>${bar}<div class="pv-window__art" id="player-window-art" data-reads="${LIST_MEDIA}"></div>${corner}</figure>` + badge + '<p class="pv-status" id="player-status" role="status" hidden></p></div>';
  }
  function cover(campaign, media, titled) {
    const image = illustrationOf(media, campaign?.cover || media.cover);
    const picture = image ?? { art: campaign?.art ?? "", glyph: campaign?.glyph ?? "", title: "Couverture de la campagne" };
    const title = campaign === null || !titled ? "" : `<h1 id="player-cover-title">${esc(campaign.name)}</h1>${campaign.session === null ? "" : `<p>S\xE9ance ${campaign.session.order} \u2014 ${esc(campaign.session.title)}</p>`}`;
    return `<div class="pv-cover" id="player-cover" data-reads="${GET_CAMPAIGN}">${art({ ...picture, title: "Couverture de la campagne" })}<div class="pv-cover__title">${title}</div></div>`;
  }
  function scene(image) {
    return `<div class="pv-scene" id="player-scene"><div class="pv-scene__art" data-reads="${LIST_MEDIA}" style="aspect-ratio: ${image.ratio}">${art(image)}</div></div>`;
  }
  function mapSlot(instance, pilot) {
    const props2 = JSON.stringify({ instance, view: "player", pilot, fill: true });
    return `<div class="pv-map" id="player-map" data-component="${BATTLEMAP}" data-props="${esc(props2)}"></div>`;
  }

  // apps/live/player-stage/src/style.ts
  var STYLE = `
.pv { box-sizing: border-box; position: relative; width: 100%; height: 100%; min-height: max(calc(var(--space-5) * 12), calc(100vh - var(--space-4) * 2)); overflow: hidden; background: var(--color-surface); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.pv *, .pv *::before, .pv *::after { box-sizing: inherit; }
.pv h1, .pv p { margin: 0; }
.pv .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.pv .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.pv .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.pv .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.pv .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.pv .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.pv .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.pv .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.pv .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.pv .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.pv-base, .pv-map { position: absolute; inset: 0; }
.pv-cover { position: absolute; inset: 0; }
.pv-cover .art__glyph { font-size: calc(var(--font-size-heading) * 4); opacity: 0.35; }
.pv-cover__title { position: absolute; left: var(--space-5); bottom: var(--space-5); z-index: 1; display: grid; gap: var(--space-2); }
.pv-cover__title h1 { font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: calc(var(--font-size-heading) * 2); text-shadow: var(--shadow-high); }
.pv-cover__title p { font-size: var(--font-size-large); color: var(--color-text); text-shadow: var(--shadow-low); }
.pv-scene { position: absolute; inset: var(--space-4); display: grid; place-items: center; }
.pv-scene__art { height: 100%; max-width: 100%; border-radius: var(--radius-large); overflow: hidden; box-shadow: var(--shadow-high); }
.pv-scene__art .art__glyph { font-size: calc(var(--font-size-heading) * 3); }
.pv-window { position: absolute; z-index: 20; margin: 0; display: block; background: var(--color-surface-2); border: calc(var(--border-width) * 2) solid var(--color-accent); border-radius: var(--radius-medium); box-shadow: var(--shadow-high); overflow: hidden; touch-action: none; }
.pv-window__bar { position: absolute; top: 0; left: 0; right: 0; z-index: 1; padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); font-family: var(--font-family-heading); text-transform: uppercase; color: var(--color-accent); background: color-mix(in srgb, var(--color-surface) 85%, transparent); cursor: move; user-select: none; }
.pv-window__art { position: absolute; inset: 0; }
.pv-window__resize { position: absolute; z-index: 1; right: 0; bottom: 0; width: var(--space-4); height: var(--space-4); background: linear-gradient(135deg, transparent 50%, var(--color-accent) 50%); cursor: nwse-resize; }
.pv-pilot { position: absolute; left: var(--space-3); bottom: var(--space-3); z-index: 30; padding: var(--space-1) var(--space-3); background: var(--color-primary); color: var(--color-primary-contrast); border-radius: var(--radius-pill); font-size: var(--font-size-small); font-weight: var(--font-weight-heading); }
.pv-status { position: absolute; left: var(--space-3); bottom: calc(var(--space-3) + var(--space-5)); z-index: 30; padding: var(--space-1) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-danger); color: var(--color-danger); border-radius: var(--radius-medium); font-size: var(--font-size-small); }
.pv-status[hidden], .pv-window[hidden] { display: none; }
.pv-map .bm-carousel { position: absolute; top: var(--space-3); left: 50%; transform: translateX(-50%); z-index: 10; max-width: calc(100% - var(--space-5)); }
`;

  // apps/live/player-stage/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 500 };
  var round = (value) => Number(value.toFixed(4));
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="PlayerStage"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "PlayerStage";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, raw, shell2, timing = WAITING, composed2 = () => void 0) {
    ensureStyle(host2.ownerDocument);
    let props2 = parseProps(raw, host2.ownerDocument.defaultView?.location.search ?? "");
    let diffusion = parseDiffusion(void 0);
    let campaign = null;
    let libraryCampaign = "";
    let media = parseMedia(void 0);
    let campaignRead = false;
    let mediaRead = false;
    let loaded = false;
    let local = null;
    let inFlight = 0;
    let gesture = null;
    let baseKey = "";
    let generation = 0;
    let readNo = 0;
    let readAt = null;
    let missing = /* @__PURE__ */ new Set();
    let stopWatching = () => void 0;
    let readFailed = false;
    const part = (id) => host2.querySelector(`#${id}`);
    const stageSize = () => {
      const stage = part("player-stage");
      return { width: stage?.clientWidth || 1, height: stage?.clientHeight || 1 };
    };
    const scope = () => props2.campaign === "" ? {} : { campaign: props2.campaign };
    const campaignId = () => props2.campaign || campaign?.id || "";
    function say(text2) {
      if (!props2.pilot) return;
      const box = part("player-status");
      if (!box) return;
      box.textContent = text2;
      box.hidden = text2 === "";
    }
    function paintBase() {
      const box = part("player-base");
      if (!box) return;
      const { base: base3, view } = diffusion;
      let instance = "";
      if (base3.kind === "battlemap" && base3.id !== "") {
        instance = base3.id;
        const key = `battlemap:${base3.id}`;
        if (key !== baseKey) {
          baseKey = key;
          box.innerHTML = mapSlot(base3.id, props2.pilot);
          composed2(box);
        }
        const slot = box.querySelector("#player-map");
        if (slot && view !== null) {
          slot.dataset.zoom = String(view.zoom);
          slot.dataset.panX = String(view.panX);
          slot.dataset.panY = String(view.panY);
        }
      } else {
        const image = base3.kind === "illustration" ? illustrationOf(media, base3.id) : void 0;
        const html = image ? scene(image) : cover(campaign, media, windowImage() === void 0);
        if (html !== baseKey) {
          baseKey = html;
          box.innerHTML = html;
        }
      }
      shell2.context?.set(BATTLEMAP_CONTEXT, instance);
    }
    function current() {
      return diffusion.window === null ? null : { ...diffusion.window, ...local };
    }
    function windowImage() {
      return diffusion.window === null ? void 0 : illustrationOf(media, diffusion.window.media);
    }
    function paintWindow() {
      const figure = part("player-window");
      if (!figure) return;
      const win = current();
      const image = win && illustrationOf(media, win.media);
      if (!win || !image) {
        figure.hidden = true;
        return;
      }
      figure.hidden = false;
      figure.style.left = `${win.x * 100}%`;
      figure.style.top = `${win.y * 100}%`;
      figure.style.width = `${Math.min(win.w, widest(win, image.ratio, stageSize())) * 100}%`;
      figure.style.aspectRatio = String(image.ratio);
      const bar = part("player-window-bar");
      if (bar) bar.textContent = image.title;
      const picture = part("player-window-art");
      if (picture && picture.dataset.media !== image.id) {
        picture.dataset.media = image.id;
        picture.innerHTML = art(image);
      }
    }
    function paintAll() {
      paintBase();
      paintWindow();
    }
    async function readCampaign() {
      let id = props2.campaign;
      try {
        if (id === "") id = parseLiveCampaign((await shell2.capability(GET_LIVE_SESSION, void 0, {})).data);
        return id === "" ? { id } : { id, overview: await shell2.capability(GET_CAMPAIGN, void 0, { id }) };
      } catch {
        return { id };
      }
    }
    async function refresh() {
      const mine = generation;
      const no = readNo += 1;
      try {
        const read = await shell2.capability(GET_DIFFUSION, void 0, { view: props2.pilot ? "dm" : "player", ...scope() });
        if (mine !== generation || no < readNo) return;
        const next = parseDiffusion(read.data);
        const named = campaignRead ? void 0 : await readCampaign();
        if (mine !== generation || no < readNo) return;
        if (named) {
          libraryCampaign = named.id;
          if (named.overview) {
            campaign = parseCampaign(named.overview.data);
            campaignRead = true;
          }
        }
        const unknown = (id) => id !== "" && illustrationOf(media, id) === void 0 && !missing.has(id);
        const wantsMedia = libraryCampaign !== "" && (!mediaRead || unknown(next.window?.media ?? "") || next.base.kind === "illustration" && unknown(next.base.id));
        const listed = wantsMedia ? await shell2.capability(LIST_MEDIA, void 0, { campaign: libraryCampaign }).catch(() => void 0) : void 0;
        if (mine !== generation || no < readNo) return;
        if (listed) {
          media = parseMedia(listed.data);
          mediaRead = true;
          missing = new Set([next.window?.media ?? "", next.base.id].filter((id) => illustrationOf(media, id) === void 0));
        }
        diffusion = next;
        readAt = read.asOf;
        loaded = true;
        if (readFailed) {
          readFailed = false;
          say("");
        }
        paintAll();
      } catch {
        if (mine !== generation || no < readNo) return;
        if (props2.pilot) {
          readFailed = true;
          say("Impossible de lire la diffusion.");
        }
        if (!loaded) {
          loaded = true;
          paintAll();
        }
      }
    }
    async function start() {
      generation += 1;
      const mine = generation;
      stopWatching();
      diffusion = parseDiffusion(void 0);
      campaign = null;
      libraryCampaign = "";
      media = parseMedia(void 0);
      campaignRead = false;
      mediaRead = false;
      missing = /* @__PURE__ */ new Set();
      readAt = null;
      loaded = false;
      readFailed = false;
      local = null;
      inFlight = 0;
      gesture = null;
      baseKey = "";
      host2.innerHTML = skeleton(props2.pilot);
      await refresh();
      if (mine !== generation) return;
      stopWatching = shell2.onDataVersion((version) => {
        if (version !== readAt) void refresh();
      }, timing.watchMs);
    }
    async function send(body) {
      const answer = await shell2.command(body);
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    async function commit(g) {
      const mine = generation;
      inFlight += 1;
      say("");
      try {
        const named = campaignId();
        const body = g.mode === "move" ? commandBody(MOVE_WINDOW, { campaign: named, x: round(g.to.x), y: round(g.to.y) }) : commandBody(RESIZE_WINDOW, { campaign: named, w: round(g.box.w), h: round(g.box.h) });
        const state = await send(body);
        if (state.status === "rejected") say(`Fen\xEAtre non modifi\xE9e : ${reasons(state.violations)}.`);
        else if (state.status !== "applied") say(`La commande n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`);
      } catch {
        say("La commande n\u2019a pas pu \xEAtre envoy\xE9e.");
      } finally {
        if (mine === generation) {
          inFlight -= 1;
          if (inFlight === 0) {
            if (gesture === null) local = null;
            await refresh();
          }
        }
      }
    }
    host2.addEventListener("pointerdown", (event) => {
      if (gesture || event.button !== 0) return;
      const target = event.target;
      const mode = target?.closest("#player-window-resize") ? "resize" : props2.pilot && target?.closest("#player-window-bar") ? "move" : null;
      const win = current();
      const image = win === null ? void 0 : illustrationOf(media, win.media);
      if (mode === null || win === null || image === void 0) return;
      event.preventDefault();
      if (campaignId() === "") {
        campaignRead = false;
        say("La campagne n\u2019a pas pu \xEAtre lue : la fen\xEAtre ne peut pas \xEAtre modifi\xE9e.");
        void refresh();
        return;
      }
      const from = { x: win.x, y: win.y, w: win.w };
      gesture = { mode, x: event.clientX, y: event.clientY, from, ratio: image.ratio, stage: stageSize(), moved: false, to: from, box: { w: from.w, h: from.w / image.ratio } };
      part("player-window")?.setPointerCapture?.(event.pointerId);
    });
    host2.addEventListener("pointermove", (event) => {
      const g = gesture;
      if (!g) return;
      const [dx, dy] = [event.clientX - g.x, event.clientY - g.y];
      if (dx === 0 && dy === 0 && !g.moved) return;
      g.moved = true;
      if (g.mode === "move") g.to = moved(g.from, dx, dy, g.ratio, g.stage);
      else {
        g.box = dragged(g.from, dx, dy, g.ratio, g.stage);
        g.to = resized(g.from, g.box, g.ratio, g.stage);
      }
      local = g.to;
      paintWindow();
    });
    const release = (event) => {
      const g = gesture;
      if (!g) return;
      gesture = null;
      if (event.type === "pointercancel" || !g.moved) {
        if (inFlight === 0) local = null;
        paintWindow();
        return;
      }
      void commit(g);
    };
    host2.addEventListener("pointerup", release);
    host2.addEventListener("pointercancel", release);
    void start();
    return {
      update(next) {
        const before = props2;
        props2 = parseProps(next, host2.ownerDocument.defaultView?.location.search ?? "");
        if (props2.pilot !== before.pilot || props2.campaign !== before.campaign) void start();
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
        const at = (base3) => fetch(new URL(base3 + path, location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0);
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

  // mock/src/PlayerStage.js
  var { host, props } = mountPoint();
  var base2 = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var shell = {
    ...base2,
    onDataVersion(listener) {
      const server = mockServer();
      const source = server.eventSource();
      let watching = true;
      source.addEventListener("dataVersion", (message) => {
        if (watching) listener(JSON.parse(message.data).dataVersion);
      });
      listener(server.dataVersion);
      return () => {
        watching = false;
        source.close();
      };
    }
  };
  var composed = (box) => {
    if (window.Mock) window.Mock.mount(box);
  };
  compose(props, (resolved) => mount(host, resolved, shell, void 0, composed));
})();
