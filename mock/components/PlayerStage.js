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

  // apps/live/player-stage/src/ids.ts
  var GET_DIFFUSION = "live.getDiffusion";
  var GET_CAMPAIGN = "campaign.getCampaign";
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
    const corner = pilot ? `<span class="pv-window__resize" id="player-window-resize" data-writes="${RESIZE_WINDOW}" aria-hidden="true"></span>` : "";
    const badge = pilot ? '<p class="pv-pilot" id="player-pilot-badge">Vue pilot\xE9e par le MJ \xB7 d\xE9placez les tokens et la fen\xEAtre, zoomez la carte</p>' : "";
    return `<div class="pv${pilot ? " pv--pilot" : ""}" id="player-stage"><div class="pv-base" id="player-base" data-reads="${GET_DIFFUSION}"></div><figure class="pv-window" id="player-window" hidden>${bar}<div class="pv-window__art" id="player-window-art" data-reads="${LIST_MEDIA}"></div>${corner}</figure>` + badge + '<p class="pv-status" id="player-status" role="status" hidden></p></div>';
  }
  function cover(campaign, media) {
    const image = illustrationOf(media, campaign?.cover || media.cover);
    const picture = image ?? { art: campaign?.art ?? "", glyph: campaign?.glyph ?? "", title: "Couverture de la campagne" };
    const title = campaign === null ? "" : `<h1 id="player-cover-title">${esc(campaign.name)}</h1>${campaign.session === null ? "" : `<p>S\xE9ance ${campaign.session.order} \u2014 ${esc(campaign.session.title)}</p>`}`;
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
.pv { box-sizing: border-box; position: relative; width: 100%; height: 100%; min-height: calc(var(--space-5) * 12); overflow: hidden; background: var(--color-surface); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
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
        const html = image ? scene(image) : cover(campaign, media);
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
    function paintWindow() {
      const figure = part("player-window");
      if (!figure) return;
      const win = current();
      const image = win === null ? void 0 : illustrationOf(media, win.media);
      if (win === null || image === void 0) {
        figure.hidden = true;
        return;
      }
      figure.hidden = false;
      figure.style.left = `${win.x * 100}%`;
      figure.style.top = `${win.y * 100}%`;
      figure.style.width = `${win.w * 100}%`;
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
    async function refresh() {
      const mine = generation;
      const no = readNo += 1;
      try {
        const read = await shell2.capability(GET_DIFFUSION, void 0, { view: props2.pilot ? "dm" : "player", ...scope() });
        if (mine !== generation || no < readNo) return;
        const next = parseDiffusion(read.data);
        const unknown = (id) => id !== "" && illustrationOf(media, id) === void 0 && !missing.has(id);
        const wantsMedia = !mediaRead || unknown(next.window?.media ?? "") || next.base.kind === "illustration" && unknown(next.base.id);
        const [named, listed] = await Promise.all([
          campaignRead ? void 0 : shell2.capability(GET_CAMPAIGN, void 0, props2.campaign === "" ? {} : { id: props2.campaign }).catch(() => void 0),
          wantsMedia ? shell2.capability(LIST_MEDIA, void 0, scope()).catch(() => void 0) : void 0
        ]);
        if (mine !== generation || no < readNo) return;
        if (named) {
          campaign = parseCampaign(named.data);
          campaignRead = true;
        }
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
      if (!props2.pilot || gesture || event.button !== 0) return;
      const target = event.target;
      const mode = target?.closest("#player-window-resize") ? "resize" : target?.closest("#player-window-bar") ? "move" : null;
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

  // mock/src/reads.js
  var bridged = /* @__PURE__ */ new WeakSet();
  async function instanceOf(server, id) {
    const data = await server.load("table.getBattlemapInstance");
    if (!data) return void 0;
    if (id && data.id !== id) return server.instances ? server.instances.get(id) : void 0;
    const store = window.Mock && window.Mock.store;
    if (store && !bridged.has(data)) {
      bridged.add(data);
      data.state = store.get(`map:${data.id}`, data.state);
      store.on(`map:${data.id}`, (state) => {
        if (state && state !== data.state) {
          data.state = state;
          server.bump();
        }
      });
    }
    return data;
  }
  function shareInstance(data) {
    if (window.Mock) window.Mock.store.set(`map:${data.id}`, data.state);
  }
  function healthBand(hp, hpMax) {
    if (typeof hp !== "number" || typeof hpMax !== "number") return null;
    const current = Math.max(0, Math.min(hpMax, hp));
    return current === 0 ? "mort" : current === hpMax ? "indemne" : 2 * current > hpMax ? "bless\xE9" : "en sang";
  }
  async function dressed(server, tokens) {
    const characters = await server.load("campaign.listCharacters") || [];
    return tokens.map((t) => {
      const c = characters.find((x) => x.id === t.char);
      return c ? { ...t, initials: c.initials, color: c.color, portrait: c.portrait } : t;
    });
  }
  var diffusionBridged = false;
  async function diffusionRow(server) {
    const fixture = await server.load("live.getDiffusion");
    const store = window.Mock && window.Mock.store;
    if (store && !diffusionBridged) {
      diffusionBridged = true;
      store.on("diffusion", () => server.bump());
    }
    return store ? store.get("diffusion", fixture) : structuredClone(fixture);
  }
  async function ratioOfIllustration(server, id) {
    const media = await server.load("media.listMedia");
    const found = media && media.illustrations.find((i) => i.id === id);
    return found && found.ratio > 0 ? found.ratio : 1;
  }
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e" };
  var READS = {
    // live.getDiffusion (crates/live/capabilities/get-diffusion): the one row both
    // player views read, so view 'dm' and view 'player' answer the same values —
    // position, size, zoom and pan. The window's x and w are fractions of the stage's
    // width, y of its height; h, derived from the image's ratio, is in the width's unit
    // and never stored. Nothing else leaves the row: no token, no HP figure, no note.
    "live.getDiffusion": async (server) => {
      const row = await diffusionRow(server);
      const win = row.window;
      const ratio = win ? await ratioOfIllustration(server, win.id || win.media) : 1;
      return {
        base: { kind: row.base.kind, id: row.base.id || "" },
        window: win ? { media: win.id || win.media, x: win.x, y: win.y, w: win.w, h: win.w / ratio } : null,
        view: row.view ? { zoom: row.view.z ?? row.view.zoom, panX: row.view.px ?? row.view.panX, panY: row.view.py ?? row.view.panY } : null
      };
    },
    // table.getBattlemapInstance (crates/table/capabilities/get-battlemap-instance):
    // the DM sees everything; the players no hidden token, no hit point figure, no
    // note — each visible NPC gives its HealthBand@1 band instead.
    "table.getBattlemapInstance": async (server, { instance, view } = {}) => {
      const data = await instanceOf(server, instance);
      if (!data) return data;
      const dressedTokens = (await dressed(server, data.state.tokens)).map((t) => t.kind === "pnj" ? { ...t, dead: t.hp === 0 } : t);
      if (view !== "player") return { ...data, state: { ...data.state, tokens: dressedTokens } };
      const tokens = dressedTokens.filter((t) => t.visible).map((t) => ({ id: t.id, char: t.char, kind: t.kind, name: t.name, num: t.num, q: t.q, r: t.r, visible: true, states: t.states, dead: t.dead, initials: t.initials, color: t.color, portrait: t.portrait, healthBand: t.kind === "pnj" ? healthBand(t.hp, t.hpMax) : null }));
      const seen = new Set(tokens.map((t) => t.id));
      const combat = data.state.combat ? { ...data.state.combat, current: seen.has(data.state.combat.current) ? data.state.combat.current : "", order: data.state.combat.order.filter((o) => seen.has(o.id)) } : null;
      return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat } };
    },
    // session.getSession (crates/session/capabilities/get-session): one session's
    // header and the PCs present, a session of `session.listSessions` (the ones
    // created since included), its status labelled as SessionLifecycle@1 says. An
    // unknown id is no session: `null`, as the server answers it.
    "session.getSession": async (server, { id }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === id);
      if (!session) return null;
      const status = session.status === "played" ? "closed" : session.status;
      return { id: session.id, order: session.order, title: session.title, date: session.date, status, statusLabel: STATUS_LABELS[status] ?? null, present: session.present };
    },
    // campaign.listImportedNotes (crates/campaign/capabilities/list-imported-notes):
    // the notes already imported, newest first, and the proposals of the latest
    // analysed note still awaiting validation. The imports are the fixture's
    // list; the proposals are what `ai.extractFromNotes` recorded (commands.js).
    "campaign.listImportedNotes": async (server) => {
      const fixture = await server.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture && fixture.imports || [];
      return { imports: [...imports].sort((x, y) => String(y.date).localeCompare(String(x.date))), proposals: server.noteProposals || [] };
    },
    // campaign.listEntitiesToCreate (crates/campaign/capabilities/list-entities-to-create):
    // the to-create list, answered as `{ items }`; the fixture is the bare list of entries.
    "campaign.listEntitiesToCreate": async (server) => {
      const fixture = await server.load("campaign.listEntitiesToCreate");
      return { items: Array.isArray(fixture) ? fixture : fixture && fixture.items || [] };
    },
    // soundbox.listSounds (crates/soundbox/capabilities/list-sounds): the
    // campaign's sounds with their effective gain — the own volume times the
    // master volume, both percents (EffectiveVolume@1) — and whether one plays
    // now (the fixture's `playing`, which soundbox.playSound and soundbox.stopAll move). The optional `search` keeps the
    // names that hold it and `tag` the sounds that carry it, whatever the case
    // (SoundTag@1); the fixture's `duration` is kept, the store has none yet.
    "soundbox.listSounds": async (server, { search, tag }) => {
      const { masterVolume, sounds } = await server.load("soundbox.listSounds");
      const lower = (text2) => String(text2 ?? "").trim().toLowerCase();
      return {
        masterVolume,
        sounds: sounds.filter((s) => lower(s.name).includes(lower(search)) && (!lower(tag) || s.tags.some((t) => lower(t) === lower(tag)))).map((s) => ({ ...s, gain: s.volume * masterVolume / 1e4, playing: s.playing === true }))
      };
    },
    // media.getBattlemap (crates/media/capabilities/get-battlemap): one
    // battlemap of the media library, by its id, its grid as HexGrid@1 states
    // it — the opacity a fraction of 1 where the mock-up's fixtures write a
    // percentage, and a hex always 5 feet. An illustration or an unknown id
    // is no battlemap: `null`, as the server answers it, not a `not-found`.
    "media.getBattlemap": async (server, { id }) => {
      const battlemap = (await server.load("media")).battlemaps.find((b) => b.id === id);
      if (!battlemap) return null;
      const { opacity, ...grid } = battlemap.grid;
      return { ...battlemap, grid: { ...grid, opacity: opacity / 100, feetPerHex: 5 } };
    },
    // session.listScenes (crates/session/capabilities/list-scenes): a session's scenes by rank. The fixture
    // is the scenes of session 4, the one in preparation; another session has none yet.
    "session.listScenes": async (server, { session }) => session === "s4" ? await server.load("session.listScenes") : [],
    // session.getLastBrief (crates/session/capabilities/get-last-brief): a session's last oral brief and what its
    // analysis found. The fixture is the brief of session 4; another session has none: the same shape, empty.
    "session.getLastBrief": async (server, { session }) => session === "s4" ? await server.load("session.getLastBrief") : { session, lastRun: null, duration: null, transcript: [], result: { scenes: [], recognized: [], missing: [] } },
    // One element's history, newest first: the version list of `entity` in the fixture; an element the fixture
    // does not know but `entities` does has never been saved (an empty history), any other has no answer (null).
    "campaign.listEntityVersions": async (server, { entity }) => {
      const history = (await server.load("campaign.listEntityVersions"))[entity];
      if (history) return history;
      const entities = await server.load("entities");
      const known = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === entity);
      return known ? { name: known.name, versions: [] } : null;
    },
    // One element's sheet: a character, a place or an event of `entities`.
    "campaign.getEntity": async (server, { id }) => {
      const entities = await server.load("entities");
      const entity = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === id);
      if (!entity?.stats) return entity;
      return { ...entity, modifiers: Object.fromEntries(Object.entries(entity.stats).map(([ability, score]) => [ability, typeof score === "number" ? Math.floor((score - 10) / 2) : null])) };
    }
  };

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
  var normalized = (term) => String(term ?? "").trim().toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "").normalize("NFC");
  var LISTS = { pj: "campaign.listCharacters", pnj: "campaign.listCharacters", lieu: "campaign.listPlaces", evenement: "campaign.listEvents" };
  var SAVED = ["kind", "name", "aliases", "description", "notes", "player", "prompt", "background", "behavior", "stats", "ca", "pvMax", "spells", "features", "actions", "portrait", "token", "crop", "color"];
  var onScreen = (fraction, kept) => typeof fraction === "number" && Number.isFinite(fraction) ? Math.max(0, Math.min(1, fraction)) : kept;
  async function onWindow(server, change) {
    const row = await diffusionRow(server);
    if (!row.window) return ["window-fields-together"];
    const violations = await change(row.window);
    if (violations) return violations;
    if (window.Mock) window.Mock.store.set("diffusion", row);
    return void 0;
  }
  var AROUND = [[1, 0], [1, -1], [0, -1], [-1, 0], [-1, 1], [0, 1]];
  function freeHexes(start, taken, count) {
    const busy = new Set(taken.map((t) => `${t.q},${t.r}`));
    const seen = /* @__PURE__ */ new Set([`${start.q},${start.r}`]);
    const queue = [start];
    const out = [];
    for (let i = 0; i < queue.length && out.length < count; i += 1) {
      const hex = queue[i];
      if (!busy.has(`${hex.q},${hex.r}`)) out.push(hex);
      for (const [dq, dr] of AROUND) {
        const next = { q: hex.q + dq, r: hex.r + dr };
        if (!seen.has(`${next.q},${next.r}`)) {
          seen.add(`${next.q},${next.r}`);
          queue.push(next);
        }
      }
    }
    return out;
  }
  async function onToken(server, target, token, change) {
    const data = await instanceOf(server, target && target.id);
    const found = data && data.state.tokens.find((t) => t.id === token);
    if (!found) return ["instance-isolated"];
    const violations = change(found, data.state);
    if (violations) return violations;
    shareInstance(data);
    return void 0;
  }
  function dexModifier(characters, id) {
    const sheet = characters.find((c) => c.id === id);
    return Math.floor(((sheet && sheet.stats ? sheet.stats.DEX : 10) - 10) / 2);
  }
  function advance(combat, state) {
    const { order } = combat;
    const from = Math.max(0, order.findIndex((o) => o.id === combat.current));
    for (let step = 1; step <= order.length; step += 1) {
      const at = (from + step) % order.length;
      const token = state.tokens.find((t) => t.id === order[at].id);
      if (token && !token.dead) {
        if (at <= from) combat.round += 1;
        combat.current = order[at].id;
        return;
      }
    }
  }
  async function onCombat(server, id, change) {
    const data = await instanceOf(server, id);
    if (!data) return ["instance-isolated"];
    if (!data.state.combat) return ["one-active-combat"];
    const violations = await change(data.state.combat, data.state);
    if (violations) return violations;
    shareInstance(data);
    return void 0;
  }
  var soundTags = (tags) => [...new Set((Array.isArray(tags) ? tags : []).map((t) => [...String(t).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "")].map((c) => c.toLowerCase()).join("")).filter(Boolean))];
  var COMMANDS = {
    // table.moveToken (crates/table/data-capabilities/move-token): (q, r) is the
    // one position both views read.
    "table.moveToken": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      t.q = payload.q;
      t.r = payload.r;
    }),
    // table.renameToken: only that token's name; an empty name is refused.
    "table.renameToken": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      if (!String(payload.name || "").trim()) return ["name-required"];
      t.name = String(payload.name).trim();
      return void 0;
    }),
    // table.setTokenVisibility: hidden tokens keep their place; the players do not read them.
    "table.setTokenVisibility": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      t.visible = payload.visible === true;
    }),
    // table.setTokenCondition (crates/table/data-capabilities/set-token-condition): the badge
    // `payload.state` put on that token when `payload.on`, after the others and once, or taken off,
    // the others keeping their order — its conditions alone. A state that is no name without the
    // blanks around it, or holds U+0000, or an `on` that is no boolean, is `state-persisted-reset`.
    "table.setTokenCondition": (server, { payload, targetId }) => onToken(server, { id: targetId || payload.instance }, payload.token, (t) => {
      const state = typeof payload.state === "string" ? payload.state.trim() : "";
      if (!state || state.includes("\0") || typeof payload.on !== "boolean") return ["state-persisted-reset"];
      const held = t.states || [];
      t.states = payload.on ? held.includes(state) ? held : [...held, state] : held.filter((s) => s !== state);
      return void 0;
    }),
    // table.removeToken: the token leaves the map.
    "table.removeToken": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t, state) => {
      state.tokens = state.tokens.filter((x) => x !== t);
    }),
    // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
    // hp-change-one-token — the targeted token only; pj-hp-not-tracked — a PC has
    // no hit points; the current ones stay within 0 and the maximum, and an NPC
    // is dead iff they are 0 (dead-derived).
    "table.changeHitPoints": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      if (t.kind !== "pnj" || typeof t.hp !== "number") return ["pj-hp-not-tracked"];
      t.hp = Math.max(0, Math.min(t.hpMax, t.hp + Math.trunc(Number(payload.delta) || 0)));
      t.dead = t.hp === 0;
      return void 0;
    }),
    // table.startCombat: the NPC roll a d20 plus the DEX modifier of their sheet,
    // the DM enters the PCs' initiative; the order is the initiative, then the DEX,
    // then at random (initiative-tiebreak). A hidden token is in the order and keeps its rank.
    "table.startCombat": async (server, { payload, targetId }) => {
      const data = await instanceOf(server, targetId);
      if (!data) return ["instance-isolated"];
      if (data.state.combat) return ["one-active-combat"];
      const characters = await server.load("campaign.listCharacters") || [];
      const rolls = payload.rolls || {};
      const entered = payload.initiative || {};
      const order = [];
      for (const t of data.state.tokens.filter((x) => !x.dead)) {
        const dex = dexModifier(characters, t.char);
        if (t.kind === "pnj") {
          const roll = Number(rolls[t.id]);
          if (!(roll >= 1 && roll <= 20)) return ["npc-initiative-bounds"];
          order.push({ id: t.id, init: roll + dex, dex, tie: Math.random() });
        } else {
          const init = Number(entered[t.char]);
          if (!(init >= 1)) return ["npc-initiative-bounds"];
          order.push({ id: t.id, init, dex, tie: Math.random() });
        }
      }
      if (!order.length) return ["one-active-combat"];
      order.sort((a, b) => b.init - a.init || b.dex - a.dex || b.tie - a.tie);
      data.pjInitiative = { ...data.pjInitiative, ...entered };
      data.state.combat = { round: 1, current: order[0].id, order: order.map(({ id, init, dex }) => ({ id, init, dex })) };
      shareInstance(data);
      return void 0;
    },
    // table.nextTurn: next-turn — the next living participant by rank, the round counting up when it comes back to the first.
    "table.nextTurn": (server, { targetId }) => onCombat(server, targetId, (combat, state) => {
      advance(combat, state);
      return void 0;
    }),
    // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative): the open combat
    // reordered as `payload.order` gives it, every participant once, named by its token — the ranks
    // 1 to n in that order; the turn and the round stay. An order that is no list, omits a
    // participant, repeats one or names one not in the combat is `initiative-tiebreak`.
    "table.reorderInitiative": (server, { payload, targetId }) => onCombat(server, targetId || payload.instance, (combat) => {
      const named = Array.isArray(payload.order) ? payload.order : [];
      if (named.length !== combat.order.length) return ["initiative-tiebreak"];
      const order = [];
      for (const id of named) {
        const found = combat.order.find((o) => o.id === id && !order.includes(o));
        if (!found) return ["initiative-tiebreak"];
        order.push(found);
      }
      combat.order = order;
      return void 0;
    }),
    // table.addParticipant (crates/table/data-capabilities/add-participant): a token joins the running
    // order at the place its initiative earns. An NPC rolls a d20 plus its DEX modifier and is given no
    // figure; a PC takes the `payload.initiative` the DM entered (`npc-initiative-bounds`). A token not
    // on the instance is `not-found`; one already in the order is `initiative-tiebreak`.
    "table.addParticipant": (server, { payload, targetId }) => onCombat(server, targetId || payload.instance, async (combat, state) => {
      const token = state.tokens.find((t) => t.id === payload.token);
      if (!token) return ["not-found"];
      if (combat.order.some((o) => o.id === token.id)) return ["initiative-tiebreak"];
      const pc = token.kind === "pj";
      if (pc ? !Number.isInteger(payload.initiative) : payload.initiative != null) return ["npc-initiative-bounds"];
      const dex = dexModifier(await server.load("campaign.listCharacters") || [], token.char);
      const init = pc ? payload.initiative : 1 + Math.floor(Math.random() * 20) + dex;
      const at = combat.order.findIndex((o) => o.init < init || o.init === init && o.dex < dex);
      combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token.id, init, dex });
      return void 0;
    }),
    // table.removeParticipant (crates/table/data-capabilities/remove-participant): the participant
    // `payload.participant` — named by its token, as the carousel shows it — taken out of the open combat;
    // its token stays on the map and the order closes up. The participant that holds the turn is refused
    // by `next-turn`: the Battlemap sends table.nextTurn first. One not in the combat is `not-found`.
    "table.removeParticipant": (server, { payload, targetId }) => onCombat(server, targetId, (combat) => {
      if (!combat.order.some((o) => o.id === payload.participant)) return ["not-found"];
      if (combat.current === payload.participant) return ["next-turn"];
      combat.order = combat.order.filter((o) => o.id !== payload.participant);
      return void 0;
    }),
    // table.endCombat: the combat ends; the tokens stay as they are.
    "table.endCombat": (server, { targetId }) => onCombat(server, targetId, (combat, state) => {
      state.combat = null;
      return void 0;
    }),
    // table.resetBattlemap: state-persisted-reset — the instance's tokens, combat and initiative are removed: an empty map.
    "table.resetBattlemap": async (server, { targetId }) => {
      const data = await instanceOf(server, targetId);
      if (!data) return ["instance-isolated"];
      data.state = { tokens: [], combat: null };
      data.pjInitiative = {};
      shareInstance(data);
      return void 0;
    },
    // table.placeTokens: one character dropped, once or « × n »; the new tokens
    // are numbered after those already taken for it (multi-drop-numbering), a
    // monster takes its hit points from its sheet, a PC has one token.
    "table.placeTokens": async (server, { payload, targetId }) => {
      const data = await instanceOf(server, targetId);
      const character = (await server.load("campaign.listCharacters") || []).find((c) => c.id === payload.character);
      if (!data || !character) return ["instance-isolated"];
      const { tokens } = data.state;
      const mine = tokens.filter((t) => t.char === character.id);
      const pnj = character.kind === "pnj";
      if (!pnj && (mine.length || !(data.roster || []).includes(character.id))) return ["token-roster"];
      const count = pnj ? Math.max(1, Math.min(12, Number(payload.count) || 1)) : 1;
      const base3 = character.tokenName || String(character.name).split(" ")[0];
      let num2 = Math.max(0, ...mine.map((t) => t.num || 0));
      const numbered = pnj && (count > 1 || mine.length > 0);
      for (const hex of freeHexes({ q: payload.q, r: payload.r }, tokens, count)) {
        num2 += 1;
        tokens.push({
          id: `t-${character.id}${pnj ? `-${num2}` : ""}`,
          char: character.id,
          kind: character.kind,
          name: numbered ? `${base3} ${num2}` : base3,
          num: pnj ? num2 : null,
          q: hex.q,
          r: hex.r,
          visible: true,
          hp: pnj ? character.pvMax : null,
          hpMax: pnj ? character.pvMax : null,
          states: [],
          dead: false
        });
      }
      shareInstance(data);
      return void 0;
    },
    // live.moveWindow (crates/live/data-capabilities/move-window): x and y alone,
    // brought back onto the screen (0 to 1); the size and the content stay.
    "live.moveWindow": (server, { payload }) => onWindow(server, (w) => {
      w.x = onScreen(payload.x, w.x);
      w.y = onScreen(payload.y, w.y);
    }),
    // live.closeWindow (crates/live/data-capabilities/close-window): the window's
    // media, position and width are emptied together (window-fields-together);
    // the base and the framing stay. A window already closed is closed again.
    "live.closeWindow": async (server) => {
      const row = await diffusionRow(server);
      row.window = null;
      if (window.Mock) window.Mock.store.set("diffusion", row);
      return void 0;
    },
    // live.resizeWindow (crates/live/data-capabilities/resize-window): only the width
    // is stored, the largest that fits the box asked for at the image's ratio — w
    // alone with no h — from 0.1 to the screen's width, to 4 decimals
    // (window-proportional); the position stays. A w that is no number, which the
    // server refuses, keeps the width.
    "live.resizeWindow": (server, { payload }) => onWindow(server, async (w) => {
      if (typeof payload.w !== "number" || !Number.isFinite(payload.w)) return void 0;
      const ratio = await ratioOfIllustration(server, w.id || w.media);
      const fit = typeof payload.h === "number" && Number.isFinite(payload.h) ? Math.min(payload.w, payload.h * ratio) : payload.w;
      w.w = Math.round(Math.max(0.1, Math.min(1, fit)) * 1e4) / 1e4;
      return void 0;
    }),
    // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
    // a battlemap instance of the session, or an illustration of the campaign,
    // becomes the player view's base, framed as the DM framed it (1 and 0, 0
    // when no framing is sent); the floating window stays as it is. Any other
    // base would leave it incoherent (`base-coherent`).
    "live.showOnPlayerView": async (server, { payload }) => {
      let base3;
      if (payload.kind === "battlemap") {
        const data = await instanceOf(server, payload.instance);
        if (!data) return ["base-coherent"];
        base3 = { kind: "battlemap", id: data.id };
      } else if (payload.kind === "illustration") {
        const listed = [await server.load("media"), await server.load("media.listMedia")];
        if (!listed.some((media) => media && media.illustrations.some((i) => i.id === payload.media))) return ["base-coherent"];
        base3 = { kind: "illustration", id: payload.media };
      } else return ["base-coherent"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.base = base3;
        diffusion.view = { z: payload.zoom ?? 1, px: payload.panX ?? 0, py: payload.panY ?? 0 };
        store.set("diffusion", diffusion);
      }
      return void 0;
    },
    // live.showCover (crates/live/data-capabilities/show-cover): the player
    // view goes back to the campaign's cover, with neither a media nor an
    // instance as its base (`base-coherent`); the floating window and the
    // framing stay as they are.
    "live.showCover": async () => {
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.base = { kind: "cover" };
        store.set("diffusion", diffusion);
      }
      return void 0;
    },
    // live.showWindowOnPlayerView (crates/live/data-capabilities/show-window-on-player-view):
    // an illustration of the campaign opens in the floating window at its
    // default position and width, the three set together
    // (`window-fields-together`); its height follows the image's ratio and is
    // never stored (`window-proportional`). Another illustration replaces the
    // window's; the base and the framing stay as they are.
    "live.showWindowOnPlayerView": async (server, { payload }) => {
      const id = payload.media ?? payload.id;
      if (!id) return payload.kind && payload.kind !== "illustration" ? ["window-proportional"] : void 0;
      const listed = [await server.load("media"), await server.load("media.listMedia")];
      if (!listed.some((media) => media && media.illustrations.some((i) => i.id === id))) return ["window-proportional"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.window = { id, x: 0.6, y: 0.12, w: 0.3 };
        store.set("diffusion", diffusion);
      }
      return void 0;
    },
    // soundbox.addSound (crates/soundbox/data-capabilities/add-sound): a sound
    // added to the library. One with no name or no file is refused before
    // anything is queued, as the server refuses it. Its tags, a list of texts,
    // are normalised (`tags-normalized`); its own volume is a percentage from
    // 0 to 100 — a number or the text of one — full when none is given
    // (`sound-volume-range`).
    "soundbox.addSound": async (server, { payload }) => {
      const name = typeof payload.name === "string" ? payload.name.trim() : "";
      const file = typeof payload.file === "string" ? payload.file.trim() : "";
      if (!name || !file) return ["unprocessable"];
      const tags = payload.tags ?? [];
      if (!Array.isArray(tags) || !tags.every((t) => typeof t === "string")) return ["tags-normalized"];
      const given = payload.volume ?? 100;
      const volume = typeof given === "number" || typeof given === "string" && given.trim() !== "" ? Number(given) : NaN;
      if (!(volume >= 0 && volume <= 100)) return ["sound-volume-range"];
      const library = await server.load("soundbox.listSounds");
      library.sounds.unshift({ id: server.uuid(), name, tags: soundTags(tags), duration: 0, volume: Math.round(volume), file });
      return void 0;
    },
    // session.createSession (crates/session/data-capabilities/create-session):
    // the next session of the campaign, at the position after the last, in
    // preparation, with the PCs ticked present. `present-only-pj` refuses a
    // character that is not a PC of the fixtures; `oneshot-single-session` a
    // second session of a oneshot.
    "session.createSession": async (server, { payload }) => {
      const sessions = await server.load("session.listSessions");
      const characters = await server.load("campaign.listCharacters");
      const campaign = (await server.load("campaign.listCampaigns") || []).find((c) => c.id === payload.campaign);
      const present = Array.isArray(payload.present) ? payload.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
      if (campaign && campaign.type === "oneshot" && (campaign.sessions ?? 0) > 0) return ["oneshot-single-session"];
      if (campaign) campaign.sessions = (campaign.sessions ?? 0) + 1;
      sessions.push({
        id: server.uuid(),
        order: sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1,
        title: String(payload.title || ""),
        date: payload.plannedOn || null,
        status: "prep",
        label: "En pr\xE9paration",
        present,
        summary: null
      });
      return void 0;
    },
    // session.addPresentCharacter (crates/session/data-capabilities/add-present-character):
    // one more PC present at the session — `targetId`, or `payload.session` —
    // listed in its `present` from then on. `present-only-pj` refuses a
    // character that is not a PC of the fixtures, which hold one campaign; a
    // session they do not list is `not-found`. A PC present already stays
    // present once.
    "session.addPresentCharacter": async (server, { payload, targetId }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
      if (!session) return ["not-found"];
      const characters = await server.load("campaign.listCharacters");
      if (!characters.some((c) => c.kind === "pj" && c.id === payload.character)) return ["present-only-pj"];
      if (!session.present.includes(payload.character)) session.present.push(payload.character);
      return void 0;
    },
    // session.reorderScenes (crates/session/data-capabilities/reorder-scenes):
    // the session's scenes in the order the DM gives, every rank rewritten at
    // once, 1 to n. An order that omits a scene of the fixtures' session,
    // repeats one or names one it does not hold is `scene-order-contiguous`,
    // and no rank changes.
    "session.reorderScenes": async (server, { payload }) => {
      const scenes = await server.load("session.listScenes");
      const order = Array.isArray(payload.order) ? payload.order : [];
      const known = new Set(scenes.map((sc) => sc.id));
      if (order.length !== known.size || new Set(order).size !== order.length || order.some((id) => !known.has(id))) return ["scene-order-contiguous"];
      for (const sc of scenes) sc.order = order.indexOf(sc.id) + 1;
      scenes.sort((a, b) => a.order - b.order);
      return void 0;
    },
    // session.removePresentCharacter (crates/session/data-capabilities/remove-present-character):
    // a PC no longer present at the session — `targetId`, or `payload.session`
    // — taken out of its `present` (which `session.getSession` reads too), and
    // so no longer offered (`present-only-pj`); the character itself stays in
    // the fixtures. A PC who is not present changes nothing; a session they do
    // not list is `not-found`.
    "session.removePresentCharacter": async (server, { payload, targetId }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
      if (!session) return ["not-found"];
      session.present = session.present.filter((pc) => pc !== payload.character);
      return void 0;
    },
    // media.uploadMedia (crates/media/data-capabilities/upload-media): a
    // battlemap or an illustration added to the library, the path the DM chose
    // recorded and no file copied. A battlemap arrives with its default grid
    // (`grid-only-for-battlemap`) — a 32 px hex, pointy, 45 % opaque — at 5
    // feet per hex; an illustration has none. A place, when one is named, is a
    // lieu of the campaign (`media-place-is-lieu`); a kind that is neither is
    // refused (`media-kind`, the table's CHECK).
    "media.uploadMedia": async (server, { payload }) => {
      const { kind, place = null } = payload;
      if (kind !== "battlemap" && kind !== "illustration") return ["media-kind"];
      if (place && !(await server.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["media-place-is-lieu"];
      const base3 = { id: server.uuid(), title: String(payload.title || "").trim() || "Sans titre", file: payload.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      for (const name of ["media", "media.listMedia"]) {
        const media = await server.load(name);
        if (!media) continue;
        if (kind === "battlemap") media.battlemaps.push({ ...base3, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 } });
        else media.illustrations.push({ ...base3, ratio: payload.width > 0 && payload.height > 0 ? Math.round(payload.width / payload.height * 100) / 100 : null });
      }
      return void 0;
    },
    // media.saveGridSettings (crates/media/data-capabilities/save-grid-settings):
    // a battlemap's grid kept — the six fields of HexGrid@1, the opacity a
    // fraction of 1 where the mock-up's fixtures write a percentage. A hex size
    // of 0 or an opacity outside 0..1 is refused by `grid-values-range`; an
    // illustration or an unknown id has no grid (`grid-only-for-battlemap`).
    "media.saveGridSettings": async (server, { payload, targetId }) => {
      const battlemap = (await server.load("media")).battlemaps.find((b) => b.id === targetId);
      if (!battlemap) return ["grid-only-for-battlemap"];
      const { size, ox, oy, orient, color, opacity } = payload;
      if (!(size > 0) || !(opacity >= 0 && opacity <= 1)) return ["grid-values-range"];
      battlemap.grid = { size, ox, oy, orient, color, opacity: Math.round(opacity * 100) };
      return void 0;
    },
    // media.setMediaPlace (crates/media/data-capabilities/set-media-place): a
    // media — battlemap or illustration — attached to one of the campaign's
    // places, or detached with `place: null`, and nothing else of it changes. A
    // place that is no lieu the fixtures list is refused by
    // `media-place-is-lieu`; an unknown media is `not-found`.
    "media.setMediaPlace": async (server, { payload, targetId }) => {
      const id = targetId || payload.media;
      const listed = [await server.load("media"), await server.load("media.listMedia")];
      const found = listed.flatMap((media) => media.battlemaps.concat(media.illustrations)).filter((m) => m.id === id);
      if (!found.length) return ["not-found"];
      const place = payload.place ?? null;
      if (place !== null && !(await server.load("campaign.listPlaces")).some((p) => p.id === place)) return ["media-place-is-lieu"];
      for (const medium of found) medium.place = place;
      return void 0;
    },
    // session.addScene (crates/session/data-capabilities/add-scene): a new
    // scene, ranked after the session's last (`scene-order-contiguous`), with
    // its title, its notes and the places it links — each a place of the
    // fixtures' campaign, else `scene-links-same-campaign` — whose media
    // follow it to the session (`session-media-by-place`, derived, never
    // stored). Its notes are kept as written. A blank title is
    // `scene-title-required`: the mock answers violation ids, where the
    // server answers 422.
    "session.addScene": async (server, { payload }) => {
      const title = String(payload.title || "").trim();
      if (!title) return ["scene-title-required"];
      const known = new Set((await server.load("campaign.listPlaces")).map((p) => p.id));
      const places = [...new Set(Array.isArray(payload.places) ? payload.places : [])];
      if (places.some((id) => !known.has(id))) return ["scene-links-same-campaign"];
      const library = await server.load("media.listMedia");
      const media = library.battlemaps.concat(library.illustrations).filter((m) => places.includes(m.place)).map((m) => m.id);
      const scenes = await server.load("session.listScenes");
      scenes.push({
        id: server.uuid(),
        order: scenes.reduce((last, sc) => Math.max(last, sc.order), 0) + 1,
        title,
        places,
        characters: [],
        events: [],
        media,
        toCreate: [],
        battlemap: null,
        notes: typeof payload.notes === "string" ? payload.notes : null
      });
      return void 0;
    },
    // session.addPlaceToSession (crates/session/data-capabilities/add-place-to-session):
    // a place of the campaign joins a scene of the session — `payload.scene`,
    // else the first — after its last element, and stays there once (a place
    // already in the scene is not linked twice). Its media follow by
    // `session-media-by-place`: they are listed in the scene's media, never
    // stored apart. A place that is no lieu of the campaign is refused by
    // `scene-links-same-campaign`; a scene the session does not have is
    // `not-found` — the server answers 422 `scene-links-same-campaign` for a
    // scene of another session, which the fixtures, one session's, never list.
    "session.addPlaceToSession": async (server, { payload }) => {
      const place = payload.place;
      const scenes = await server.load("session.listScenes") || [];
      const first = scenes.reduce((low, sc) => !low || sc.order < low.order ? sc : low, null);
      const scene2 = payload.scene ? scenes.find((sc) => sc.id === payload.scene) : first;
      if (!scene2) return ["not-found"];
      if (!(await server.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["scene-links-same-campaign"];
      if (!scene2.places.includes(place)) scene2.places.push(place);
      const library = await server.load("media.listMedia") || { battlemaps: [], illustrations: [] };
      for (const medium of library.battlemaps.concat(library.illustrations)) {
        if (medium.place === place && !scene2.media.includes(medium.id)) scene2.media.push(medium.id);
      }
      return void 0;
    },
    // campaign.restoreEntityVersion (EntityVersion@1, restore-exact): the element `targetId` goes back to exactly
    // the version `payload.version` — or, with `payload.changes`, only the changes at those positions of the
    // version after it are undone, each field back to what it was. Either way the restoration is a new version
    // on top of the history — source restauration, restoredFrom the version — whose changes are what moved since
    // the version it replaces; versions 1..n stay as they were. A version the element does not have is refused.
    // table.createBattlemapInstance (crates/table/data-capabilities/create-battlemap-instance):
    // the scene's own copy of a battlemap of the campaign, empty; the battlemap, its grid and
    // the scene's media are left as they are (`instance-isolated`). A scene of another session,
    // or a battlemap the campaign's library does not list, is `not-found`; a scene that has its
    // instance is refused (`one-instance-per-scene`).
    "table.createBattlemapInstance": async (server, { payload }) => {
      const session = await server.load("session.getSession");
      const scene2 = (await server.load("session.listScenes")).find((sc) => sc.id === payload.scene);
      const map = (await server.load("media.listMedia")).battlemaps.find((m) => m.id === payload.battlemap);
      if (!session || session.id !== payload.session || !scene2 || !map) return ["not-found"];
      if (scene2.battlemap) return ["one-instance-per-scene"];
      const id = `bmi-${map.id}-${scene2.id}`;
      server.instances = server.instances || /* @__PURE__ */ new Map();
      server.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene2.id, label: scene2.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
      scene2.battlemap = id;
      return void 0;
    },
    "campaign.restoreEntityVersion": async (server, { payload, targetId }) => {
      const history = (await server.load("campaign.listEntityVersions"))[targetId];
      const target = history && history.versions.find((v) => v.v === payload.version);
      if (!target) return ["not-found"];
      const sheet = (upTo) => history.versions.slice().sort((a, b) => a.v - b.v).filter((v) => v.v <= upTo).reduce((fields, v) => {
        for (const c of v.changes) fields[c.field] = c.new;
        return fields;
      }, {});
      const before = sheet(history.versions[0].v);
      let after = sheet(target.v);
      if (Array.isArray(payload.changes)) {
        const undone = (history.versions.find((v) => v.v === payload.version + 1) || { changes: [] }).changes;
        after = { ...before };
        for (const i of payload.changes) if (undone[i]) after[undone[i].field] = undone[i].old;
      }
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].sort().filter((field) => (before[field] ?? "") !== (after[field] ?? "")).map((field) => ({ field, old: before[field] ?? "", new: after[field] ?? "" }));
      history.versions.unshift({ v: history.versions[0].v + 1, date: (/* @__PURE__ */ new Date()).toISOString(), origin: "manuelle", source: "restauration", restoredFrom: target.v, changes });
      return void 0;
    },
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
    // soundbox.updateSound (crates/soundbox/data-capabilities/update-sound): the
    // name, tags and volume of one sound, tags normalised and volume ranged as
    // for an addition; a sound that is not there is `not-found`.
    "soundbox.updateSound": async (server, { payload, targetId }) => {
      const sound = (await server.load("soundbox.listSounds")).sounds.find((s) => s.id === targetId);
      if (!sound) return ["not-found"];
      if ("volume" in payload && !(Number(payload.volume) >= 0 && Number(payload.volume) <= 100)) return ["sound-volume-range"];
      if ("name" in payload) sound.name = String(payload.name);
      if ("tags" in payload) sound.tags = soundTags(payload.tags);
      if ("volume" in payload) sound.volume = Number(payload.volume);
      if ("file" in payload) sound.file = String(payload.file);
      return void 0;
    },
    // soundbox.removeSound (crates/soundbox/data-capabilities/remove-sound): the
    // sound leaves the library's readers (a soft delete, its tombstone is the
    // server's).
    "soundbox.removeSound": async (server, { targetId }) => {
      const library = await server.load("soundbox.listSounds");
      const at = library.sounds.findIndex((s) => s.id === targetId);
      if (at < 0) return ["not-found"];
      library.sounds.splice(at, 1);
      return void 0;
    },
    // soundbox.setMasterVolume (crates/soundbox/data-capabilities/set-master-volume):
    // the master slider, in percent, kept to the whole percent; every sound
    // follows it, keeping their ratio (EffectiveVolume@1). A volume outside
    // 0..100, or no number, is refused by `master-volume-range`, never clamped;
    // a request naming no campaign is `not-found`.
    "soundbox.setMasterVolume": async (server, { payload, targetId }) => {
      if (!String(targetId || payload.campaign || "").trim()) return ["not-found"];
      const { volume } = payload;
      if (typeof volume !== "number" || !(volume >= 0 && volume <= 100)) return ["master-volume-range"];
      for (const name of ["soundbox.listSounds", "sounds"]) {
        const fixture = await server.load(name);
        if (fixture) fixture.masterVolume = Math.round(volume);
      }
      return void 0;
    },
    // soundbox.stopAll (crates/soundbox/data-capabilities/stop-all): every
    // running playback gets its stop (`stop-all`); with none running it still
    // succeeds and changes nothing.
    "soundbox.stopAll": async (server) => {
      for (const sound of (await server.load("soundbox.listSounds")).sounds) sound.playing = false;
      return void 0;
    },
    // soundbox.setSoundVolume (crates/soundbox/data-capabilities/set-sound-volume):
    // one sound's own volume, in percent, from 0 to 100; out of range it is
    // refused by `sound-volume-range`, never clamped. Nothing else of the sound
    // changes; its effective gain follows (EffectiveVolume@1).
    "soundbox.setSoundVolume": async (server, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const fixtures = (await Promise.all(["soundbox.listSounds", "sounds"].map((name) => server.load(name)))).filter(Boolean);
      const found = fixtures.map((fixture) => fixture.sounds.find((candidate) => candidate.id === id)).filter(Boolean);
      if (!found.length) return ["not-found"];
      const volume = payload?.volume;
      if (typeof volume !== "number" || !(volume >= 0 && volume <= 100)) return ["sound-volume-range"];
      for (const sound of found) sound.volume = volume;
      return void 0;
    },
    // campaign.createCampaign (crates/campaign/data-capabilities/create-campaign):
    // a new campaign at the top of the home page, its master volume at full
    // (`master-volume-range`); a oneshot comes with its single session, in
    // preparation (`oneshot-single-session`), a campagne with none. A campaign
    // with no name is refused, as one of a kind there is not.
    "campaign.createCampaign": async (server, { payload }) => {
      const name = String(payload.name ?? "").trim();
      if (!name) return ["campaign-name-required"];
      if (!["campagne", "oneshot"].includes(payload.type)) return ["campaign-type-unknown"];
      const text2 = (field) => typeof payload[field] === "string" ? payload[field] : null;
      (await server.load("campaign.listCampaigns")).unshift({
        id: server.uuid(),
        name,
        type: payload.type,
        art: text2("art"),
        glyph: text2("glyph"),
        pitch: text2("pitch"),
        sessions: payload.type === "oneshot" ? 1 : 0,
        lastPlayed: null,
        next: null
      });
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
    },
    // ai.extractFromNotes (crates/ai/data-capabilities/extract-from-notes): the
    // note analysed into proposals, nothing created (`no-creation-before-
    // validation`). The mock's AI proposes what the fixture NoteImport lists; a
    // proposal that names an existing element starts unchecked.
    "ai.extractFromNotes": async (server, { payload }) => {
      const text2 = String(payload.text || "");
      const { proposals } = await server.load("NoteImport");
      server.noteText = text2;
      server.noteProposals = proposals.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
      return void 0;
    },
    // ai.analyzeBrief (crates/ai/data-capabilities/analyze-brief): the brief told
    // or typed, kept as text (`text-only-no-audio`), cut into scenes with the
    // elements recognised and the ones to create — nothing created
    // (`brief-no-auto-creation`). The mock's AI finds in any brief what the
    // fixture OralBrief lists; the brief becomes session 4's last, and the
    // existing NPC it changes gets a version, as the real analysis writes one.
    "ai.analyzeBrief": async (server, { payload }) => {
      const text2 = String(payload.text || "").trim();
      const brief = await server.load("session.getLastBrief");
      const lines = text2.split("\n").map((line) => line.trim()).filter(Boolean);
      Object.assign(brief, { transcript: lines, lastRun: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19) });
      const histories = await server.load("campaign.listEntityVersions");
      const history = histories.morlox;
      const change = (await server.load("OralBrief")).result.recognized.find((x) => x.updated);
      if (history && change) {
        const latest = history.versions[0];
        history.versions.unshift({ v: latest ? latest.v + 1 : 1, date: brief.lastRun, origin: "ia", source: `Brief oral \u2014 s\xE9ance ${String(payload.session).replace(/^s/, "")}`, restoredFrom: null, seen: false, changes: latest ? latest.changes : [] });
      }
      return void 0;
    },
    // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
    // one element per accepted proposal, the raw note kept as an import listed
    // newest first, the proposals no longer awaiting.
    "campaign.createEntitiesFromNote": async (server, { payload }) => {
      const proposals = server.noteProposals || [];
      const accepted = Array.isArray(payload.accepted) ? payload.accepted : [];
      const created = proposals.filter((p) => accepted.includes(p.id));
      const fixture = await server.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture.imports;
      const note = server.noteText || "";
      imports.unshift({ id: `imp-${server.uuid()}`, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), title: note.split("\n")[0].replace(/[.\s]+$/, "") || "Notes import\xE9es", chars: note.length, proposed: proposals.length, created: created.length });
      server.noteProposals = [];
      return void 0;
    },
    // ai.createFromBrief (crates/ai/data-capabilities/create-from-brief): the
    // to-create entry becomes an element of the campaign, from its excerpt alone
    // or from the excerpt and the DM's refinement (the fixture ToCreateList
    // gives the AI's proposal for each); the entry is created once its element
    // is set (`to-create-leaves-once-created`), and an entry already created is
    // refused. An NPC gets its background, behaviour and sheet. The element has
    // its first version, and the entry keeps the refinement it was made from.
    "ai.createFromBrief": async (server, { payload }) => {
      const entries = await server.load("campaign.listEntitiesToCreate");
      const entry = (Array.isArray(entries) ? entries : entries?.items || []).find((candidate) => candidate.id === payload.entry);
      if (!entry) return ["not-found"];
      if (entry.status === "created") return ["to-create-leaves-once-created"];
      const given = (await server.load("ToCreateList")).proposals?.[entry.id] || {};
      const refined = typeof payload.refinement === "string" && payload.refinement.trim() !== "";
      const proposal = (refined ? given.refined : given.fromBrief) || { description: entry.excerpt };
      const { statblock, ...text2 } = proposal;
      const kind = { pnj: "pnj", lieu: "lieu", "\xE9v\xE9nement": "evenement" }[entry.type];
      const entities = await server.load("entities");
      const list = { pnj: entities.characters, lieu: entities.places, evenement: entities.events }[kind];
      const id = `${entry.id.replace(/^tc-/, "")}-${server.uuid().slice(0, 8)}`;
      const figure = (value) => Number.isNaN(parseInt(value, 10)) ? null : parseInt(value, 10);
      const vital = (label) => statblock?.vitals?.find((v) => v.label === label)?.value;
      list.push({
        id,
        name: entry.name,
        aliases: [],
        kind,
        notes: "",
        ...text2,
        ...statblock ? { type: statblock.type, stats: statblock.abilities, ca: figure(vital("CA")), pvMax: figure(vital("PV")), speed: vital("Vitesse"), features: statblock.features } : {}
      });
      const history = await server.load("campaign.listEntityVersions") || {};
      history[id] = { name: entry.name, versions: [{ v: 1, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin: "dm", source: "Cr\xE9\xE9 depuis le brief", seen: true, changes: [] }] };
      entry.status = "created";
      entry.entity = id;
      entry.refinement = refined ? payload.refinement.trim() : null;
      return void 0;
    },
    // campaign.saveEntity (crates/dataguard/data-capabilities/save-entity): the
    // DM's explicit save of an element, its sheet and its aliases, which adds a
    // version (EntityVersion@1, source `dm`) whose changes are the fields that
    // moved. An alias naming another element, by its name or an alias
    // (AliasMatch@1), is refused by `alias-unique-in-campaign`; a player on
    // anything but a PC by `character-is-pj-or-pnj`. Only the editor's fields
    // are kept: a DEX modifier given is never written.
    "campaign.saveEntity": async (server, { payload }) => {
      const given = payload.entity || payload;
      const lists = await Promise.all(Object.values(LISTS).filter((v2, i, all2) => all2.indexOf(v2) === i).map(async (name) => [name, await server.load(name) || []]));
      const all = lists.flatMap(([name, list2]) => list2.map((element) => ({ name, element })));
      const found = all.find(({ element }) => given.id && element.id === given.id);
      const before = found ? found.element : {};
      const kind = given.kind || before.kind || (found && found.name === "campaign.listPlaces" ? "lieu" : found && found.name === "campaign.listEvents" ? "evenement" : void 0);
      if (!LISTS[kind]) return ["character-is-pj-or-pnj"];
      const after = { ...before };
      for (const field of SAVED) if (field in given) after[field] = given[field];
      after.kind = kind;
      if (Array.isArray(after.aliases)) {
        const seen = /* @__PURE__ */ new Set();
        after.aliases = after.aliases.filter((a) => typeof a === "string" && normalized(a) && !seen.has(normalized(a)) && seen.add(normalized(a)));
      }
      if (after.player && kind !== "pj") return ["character-is-pj-or-pnj"];
      const own = [[normalized(after.name), false], ...(after.aliases || []).map((a) => [normalized(a), true])].filter(([t]) => t);
      const others = all.filter(({ element }) => element !== before).flatMap(({ element }) => [[normalized(element.name), false], ...(element.aliases || []).map((a) => [normalized(a), true])]);
      if (own.some(([t, alias]) => others.some(([o, otherAlias]) => o === t && (alias || otherAlias)))) return ["alias-unique-in-campaign"];
      if ((kind === "pj" || kind === "pnj") && !after.color) after.color = "accent";
      const id = before.id || given.id || server.uuid();
      after.id = id;
      const list = await server.load(LISTS[kind]) || [];
      if (found && found.name !== LISTS[kind]) {
        const old = lists.find(([name]) => name === found.name)[1];
        old.splice(old.indexOf(before), 1);
      }
      const at = list.indexOf(before);
      if (at >= 0) list[at] = after;
      else list.push(after);
      const history = await server.load("campaign.listEntityVersions") || {};
      const entry = history[id] || (history[id] = { name: after.name, versions: [] });
      entry.name = after.name;
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => f !== "id").sort().filter((f) => JSON.stringify(before[f] ?? null) !== JSON.stringify(after[f] ?? null)).map((field) => ({ field, old: before[field] ?? null, new: after[field] ?? null }));
      const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
      entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin: "dm", source: "Fiche enregistr\xE9e par le DM", seen: true, changes });
      return void 0;
    },
    // soundbox.playSound (crates/soundbox/data-capabilities/play-sound): a
    // playback of the sound starts, whatever else plays (`simultaneous-playback`),
    // from the soundbox (the default) or a suggestion's pastille; any other
    // source, the player view's, is refused (`dm-side-only`). The sound plays
    // as soundbox.listSounds says it: `playing`.
    "soundbox.playSound": async (server, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const sound = (await server.load("soundbox.listSounds")).sounds.find((candidate) => candidate.id === id);
      if (!sound) return ["not-found"];
      const source = payload?.source ?? "soundbox";
      if (!["soundbox", "suggestion"].includes(source)) return ["dm-side-only"];
      sound.playing = true;
      return void 0;
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
      const aggregate = target.aggregate || null;
      const declared = (aggregate && await s.load(`aggregates/${aggregate}`) || { fields: {} }).fields || {};
      const derived = Object.keys(payload).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
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
      const needsReview = verdict.review.length > 0 || Object.keys(payload).some((f) => declared[f] && declared[f].criticality === "critical");
      const merging = new Set(Object.keys(declared).filter((f) => declared[f].merge));
      const ahead = declaredAhead(entry, merging);
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
