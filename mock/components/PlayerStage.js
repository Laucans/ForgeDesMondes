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
    return { pilot: raw.pilot === true || new URLSearchParams(search).get("pilote") === "1" };
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
    const part = (id) => host2.querySelector(`#${id}`);
    const stageSize = () => {
      const stage = part("player-stage");
      return { width: stage?.clientWidth || 1, height: stage?.clientHeight || 1 };
    };
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
        const read = await shell2.capability(GET_DIFFUSION, void 0, { view: props2.pilot ? "dm" : "player" });
        if (mine !== generation || no < readNo) return;
        const next = parseDiffusion(read.data);
        const unknown = (id) => id !== "" && illustrationOf(media, id) === void 0 && !missing.has(id);
        const wantsMedia = !mediaRead || unknown(next.window?.media ?? "") || next.base.kind === "illustration" && unknown(next.base.id);
        const [named, listed] = await Promise.all([
          campaignRead ? void 0 : shell2.capability(GET_CAMPAIGN, void 0, {}).catch(() => void 0),
          wantsMedia ? shell2.capability(LIST_MEDIA, void 0, {}).catch(() => void 0) : void 0
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
        paintAll();
      } catch {
        if (mine !== generation || no < readNo) return;
        if (props2.pilot) say("Impossible de lire la diffusion.");
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
        const campaignId = campaign?.id ?? "";
        const body = g.mode === "move" ? commandBody(MOVE_WINDOW, { campaign: campaignId, x: round(g.to.x), y: round(g.to.y) }) : commandBody(RESIZE_WINDOW, { campaign: campaignId, w: round(g.box.w), h: round(g.box.h) });
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
      if (campaign === null) {
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
        if (props2.pilot !== before.pilot) void start();
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
  var pristine = /* @__PURE__ */ new Map();
  async function instanceOf(server, id) {
    const data = await server.load("table.getBattlemapInstance");
    if (!data || id && data.id !== id) return void 0;
    const store = window.Mock && window.Mock.store;
    if (!pristine.has(data.id)) pristine.set(data.id, structuredClone(data.state));
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
  var pristineState = (id) => structuredClone(pristine.get(id));
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
      const tokens = dressedTokens.filter((t) => t.visible).map(({ hp, hpMax, ...rest }) => ({ ...rest, healthBand: rest.kind === "pnj" ? healthBand(hp, hpMax) : null }));
      const seen = new Set(tokens.map((t) => t.id));
      const combat = data.state.combat ? { ...data.state.combat, current: seen.has(data.state.combat.current) ? data.state.combat.current : "", order: data.state.combat.order.filter((o) => seen.has(o.id)) } : null;
      return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat } };
    },
    // media.getBattlemap (crates/media/capabilities/get-battlemap): one battlemap
    // by its id, its grid per HexGrid@1 — the opacity a fraction of 1, a hex 5 feet.
    "media.getBattlemap": async (server, { id } = {}) => {
      const map = await server.load("media.getBattlemap");
      if (!map || id && map.id !== id) return void 0;
      return { ...map, grid: { ...map.grid, opacity: map.grid.opacity > 1 ? map.grid.opacity / 100 : map.grid.opacity, feetPerHex: 5 } };
    },
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
      return [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === id);
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
  var onScreen = (fraction, kept) => typeof fraction === "number" && Number.isFinite(fraction) ? Math.max(0, Math.min(1, fraction)) : kept;
  async function onWindow(server, change) {
    const row = await diffusionRow(server);
    if (!row.window) return ["window-fields-together"];
    const violations = await change(row.window);
    if (violations) return violations;
    if (window.Mock) window.Mock.store.set("diffusion", row);
    return void 0;
  }
  var normalized = (term) => String(term ?? "").trim().toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "").normalize("NFC");
  var LISTS = { pj: "campaign.listCharacters", pnj: "campaign.listCharacters", lieu: "campaign.listPlaces", evenement: "campaign.listEvents" };
  var SAVED = ["kind", "name", "aliases", "description", "notes", "player", "prompt", "background", "behavior", "stats", "ca", "pvMax", "spells", "features", "actions", "portrait", "token", "crop", "color"];
  var COMMANDS = {
    // live.showCover (crates/live/data-capabilities/show-cover): the player view
    // goes back to the campaign cover, neither a media nor an instance as its base
    // (base-coherent); the floating window and the framing stay as they are.
    "live.showCover": async (server) => {
      const row = await diffusionRow(server);
      row.base = { kind: "cover" };
      if (window.Mock) window.Mock.store.set("diffusion", row);
      return void 0;
    },
    // live.moveWindow (crates/live/data-capabilities/move-window): x and y alone,
    // brought back onto the screen (0 to 1); the size and the content stay.
    "live.moveWindow": (server, { payload }) => onWindow(server, (w) => {
      w.x = onScreen(payload.x, w.x);
      w.y = onScreen(payload.y, w.y);
    }),
    // live.resizeWindow (crates/live/data-capabilities/resize-window): only the width
    // is stored, the largest that fits the box asked for at the image's ratio
    // (window-proportional); the position stays.
    "live.resizeWindow": (server, { payload }) => onWindow(server, async (w) => {
      const ratio = await ratioOfIllustration(server, w.id || w.media);
      w.w = Math.max(0.1, Math.min(payload.w, payload.h * ratio, 1 - w.x));
    }),
    // live.showWindowOnPlayerView (crates/live/data-capabilities/show-window-on-player-view):
    // the illustration opens in the floating window at its default position and
    // width, all three set together; its height follows the image's ratio and is
    // never stored (window-proportional). The base stays.
    "live.showWindowOnPlayerView": async (server, { payload }) => {
      const id = payload.media ?? payload.id;
      if (!id) return payload.kind && payload.kind !== "illustration" ? ["window-proportional"] : void 0;
      const media = await server.load("media.listMedia");
      if (!(media && media.illustrations.some((i) => i.id === id))) return ["window-proportional"];
      const row = await diffusionRow(server);
      row.window = { id, x: 0.6, y: 0.12, w: 0.3 };
      if (window.Mock) window.Mock.store.set("diffusion", row);
      return void 0;
    },
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
    // table.setTokenCondition: the badges of that token alone.
    "table.setTokenCondition": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      t.states = [...payload.conditions || []];
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
    // table.reorderInitiative: the participant takes the rank `position`; nobody else changes relative rank.
    "table.reorderInitiative": (server, { payload, targetId }) => onCombat(server, targetId, (combat) => {
      const from = combat.order.findIndex((o) => o.id === payload.token);
      if (from < 0) return ["instance-isolated"];
      const [moved2] = combat.order.splice(from, 1);
      combat.order.splice(Math.max(0, Math.min(combat.order.length, Number(payload.position) || 0)), 0, moved2);
      return void 0;
    }),
    // table.addParticipant: a token joins the running order at the place its initiative earns.
    "table.addParticipant": (server, { payload, targetId }) => onCombat(server, targetId, async (combat, state) => {
      const token = state.tokens.find((t) => t.id === payload.token);
      if (!token || combat.order.some((o) => o.id === token.id)) return ["instance-isolated"];
      const dex = dexModifier(await server.load("campaign.listCharacters") || [], token.char);
      const roll = Number(payload.roll);
      const init = token.kind === "pnj" ? roll + dex : Number(payload.initiative);
      if (!(init >= 1) || token.kind === "pnj" && !(roll >= 1 && roll <= 20)) return ["npc-initiative-bounds"];
      const at = combat.order.findIndex((o) => o.init < init || o.init === init && o.dex < dex);
      combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token.id, init, dex });
      return void 0;
    }),
    // table.removeParticipant: the token leaves the order, not the map; the turn passes on if it was its turn.
    "table.removeParticipant": (server, { payload, targetId }) => onCombat(server, targetId, (combat, state) => {
      if (!combat.order.some((o) => o.id === payload.token)) return ["instance-isolated"];
      const rest = { ...combat, order: combat.order.filter((o) => o.id !== payload.token) };
      if (combat.current === payload.token) {
        const gone = { ...state, tokens: state.tokens.map((t) => t.id === payload.token ? { ...t, dead: true } : t) };
        advance(combat, gone);
        rest.round = combat.round;
        rest.current = combat.current === payload.token ? rest.order[0] && rest.order[0].id || "" : combat.current;
      }
      if (rest.order.length) Object.assign(combat, rest);
      else state.combat = null;
      return void 0;
    }),
    // table.endCombat: the combat ends; the tokens stay as they are.
    "table.endCombat": (server, { targetId }) => onCombat(server, targetId, (combat, state) => {
      state.combat = null;
      return void 0;
    }),
    // table.resetBattlemap: state-persisted-reset — the instance goes back to the state it was opened with: no combat, no initiative.
    "table.resetBattlemap": async (server, { targetId }) => {
      const data = await instanceOf(server, targetId);
      if (!data) return ["instance-isolated"];
      data.state = pristineState(data.id);
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
      if (!pnj && mine.length) return ["token-roster"];
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
    // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
    // a battlemap instance or an illustration becomes the player view's base,
    // framed as the DM framed it (1 and 0, 0 when no framing is sent); the
    // floating window stays as it is.
    "live.showOnPlayerView": async (server, { payload }) => {
      let base3;
      if (payload.kind === "battlemap") {
        const data = await instanceOf(server, payload.instance);
        if (!data) return ["base-coherent"];
        base3 = { kind: "battlemap", id: data.id };
      } else if (payload.kind === "illustration") {
        const media = await server.load("media.listMedia");
        if (!(media && media.illustrations.some((i) => i.id === payload.media))) return ["base-coherent"];
        base3 = { kind: "illustration", id: payload.media };
      } else return ["base-coherent"];
      const row = await diffusionRow(server);
      row.base = base3;
      row.view = { z: payload.zoom ?? 1, px: payload.panX ?? 0, py: payload.panY ?? 0 };
      if (window.Mock) window.Mock.store.set("diffusion", row);
      return void 0;
    },
    // campaign.restoreEntityVersion (EntityVersion@1, restore-exact): the element `targetId` goes back to exactly
    // the version `payload.version` — or, with `payload.changes`, only the changes at those positions of the
    // version after it are undone, each field back to what it was. Either way the restoration is a new version
    // on top of the history — source restauration, restoredFrom the version — whose changes are what moved since
    // the version it replaces; versions 1..n stay as they were. A version the element does not have is refused.
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
