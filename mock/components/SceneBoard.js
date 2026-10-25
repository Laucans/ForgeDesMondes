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

  // apps/session/scene-board/src/ids.ts
  var GET_SESSION = "session.getSession";
  var LIST_SCENES = "session.listScenes";
  var LIST_CHARACTERS = "campaign.listCharacters";
  var LIST_PLACES = "campaign.listPlaces";
  var LIST_EVENTS = "campaign.listEvents";
  var LIST_MEDIA = "media.listMedia";
  var GET_BATTLEMAP_INSTANCE = "table.getBattlemapInstance";
  var ADD_PRESENT_CHARACTER = "session.addPresentCharacter";
  var REMOVE_PRESENT_CHARACTER = "session.removePresentCharacter";
  var ADD_SCENE = "session.addScene";
  var REORDER_SCENES = "session.reorderScenes";
  var CREATE_BATTLEMAP_INSTANCE = "table.createBattlemapInstance";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/session/scene-board/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var records = (value) => Array.isArray(value) ? value.filter(isRecord) : [];
  function parseSession(data) {
    const answer = isRecord(data) ? data : {};
    return {
      id: text(answer.id),
      title: text(answer.title),
      order: typeof answer.order === "number" ? answer.order : null,
      campaign: typeof answer.campaign === "string" ? answer.campaign : null,
      present: texts(answer.present)
    };
  }
  function parseScenes(data) {
    return records(data).map((scene, index) => ({
      scene: {
        id: text(scene.id),
        title: text(scene.title),
        places: texts(scene.places),
        characters: texts(scene.characters),
        events: texts(scene.events),
        media: texts(scene.media),
        toCreate: texts(scene.toCreate),
        battlemap: typeof scene.battlemap === "string" && scene.battlemap !== "" ? scene.battlemap : null,
        notes: text(scene.notes)
      },
      rank: typeof scene.order === "number" ? scene.order : index + 1
    })).filter(({ scene }) => scene.id !== "").sort((a, b) => a.rank - b.rank).map(({ scene }) => scene);
  }
  function parseCharacters(data) {
    return records(data).filter((character) => text(character.id) !== "").map((character) => ({ id: text(character.id), name: text(character.name) || text(character.id), kind: character.kind === "pj" ? "pj" : "pnj" }));
  }
  function parseEntities(data) {
    return records(data).filter((entity) => text(entity.id) !== "").map((entity) => ({ id: text(entity.id), name: text(entity.name) || text(entity.id) }));
  }
  var medias = (value) => records(value).filter((media) => text(media.id) !== "").map((media) => ({ id: text(media.id), title: text(media.title) || text(media.id) }));
  function parseMedias(data) {
    const answer = isRecord(data) ? data : {};
    return { battlemaps: medias(answer.battlemaps), illustrations: medias(answer.illustrations) };
  }
  function parseInstance(data) {
    if (!isRecord(data) || text(data.id) === "") return null;
    const tokens = isRecord(data.state) ? records(data.state.tokens) : [];
    return { id: text(data.id), battlemap: text(data.battlemap), tokens: tokens.length, hidden: tokens.filter((token) => token.visible === false).length };
  }
  var campaignAsked = (fromSession, prop) => {
    if (fromSession !== null && fromSession !== "") return fromSession;
    return typeof prop === "string" && prop !== "" ? prop : null;
  };
  var sessionAsked = (prop, context2) => {
    if (typeof prop === "string" && prop !== "") return prop;
    return typeof context2 === "string" ? context2 : "";
  };
  var pcs = (board) => board.characters.filter((character) => character.kind === "pj");
  var presentPcs = (board) => board.session.present.flatMap((id) => pcs(board).filter((character) => character.id === id));
  var absentPcs = (board) => pcs(board).filter((character) => !board.session.present.includes(character.id));
  function moved(scenes, id, delta) {
    const from = scenes.findIndex((scene) => scene.id === id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= scenes.length) return null;
    const order = scenes.map((scene) => scene.id);
    order.splice(to, 0, ...order.splice(from, 1));
    return order;
  }
  var ranks = (order) => order.map((scene, index) => ({ scene, rank: index + 1 }));
  function commandBody(dataCapability, payload, target, idempotencyKey) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload,
      target,
      ...idempotencyKey === void 0 ? {} : { idempotencyKey },
      caller: { by: "dm", from: "SceneBoard", roles: [] }
    };
  }
  var VIOLATIONS = {
    "present-only-pj": "seul un PJ de la campagne peut \xEAtre pr\xE9sent",
    "scene-order-contiguous": "les rangs des sc\xE8nes doivent aller de 1 \xE0 n",
    "one-instance-per-scene": "cette sc\xE8ne a d\xE9j\xE0 une instance de battlemap"
  };
  var reasons = (violations) => violations.map((violation) => VIOLATIONS[violation] ?? violation).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    return { status: text(result.status), violations: texts(result.violations) };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/session/scene-board/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var button = (attrs, label, variant) => `<button type="button" class="${variant}"${attrs}>${esc(label)}</button>`;
  var buttons = (inner) => `<div class="kit-button kit-button--bare kit-button--small">${inner}</div>`;
  function skeleton() {
    return `<section class="mf-scenes stack" id="scene-board" aria-labelledby="scenes-title" data-reads="${GET_SESSION}"><p class="mf-scenes__notice" id="scenes-notice" aria-live="polite"></p><section class="panel" id="scenes-present" aria-labelledby="scenes-present-title"><div class="panel__head"><h2 id="scenes-present-title">PJ pr\xE9sents</h2><span class="small muted" id="scenes-present-count"></span></div><div class="row" id="scenes-present-list" data-reads="${GET_SESSION} ${LIST_CHARACTERS}"></div><div class="row" id="scenes-present-add"></div></section><section class="panel" id="scenes-list-panel" aria-labelledby="scenes-title"><div class="panel__head"><h2 id="scenes-title">Sc\xE8nes</h2>${buttons(button(` id="scenes-add" data-writes="${ADD_SCENE}"`, "Ajouter une sc\xE8ne", "secondary"))}</div><ol class="mf-scenes__list" id="scenes-list" data-reads="${LIST_SCENES} ${LIST_PLACES} ${LIST_EVENTS} ${LIST_MEDIA}"></ol></section></section>`;
  }
  function sceneTitle(board) {
    const { order, title } = board.session;
    return ["Sc\xE8nes", order === null ? "" : `de la s\xE9ance ${order}`, title === "" ? "" : `\u2014 ${title}`].filter((part) => part !== "").join(" ");
  }
  var presentCount = (board) => `${presentPcs(board).length} sur ${pcs(board).length}`;
  function presentList(board) {
    return presentPcs(board).map(
      (pc) => `<span class="chip chip--pj" id="scenes-present-${esc(pc.id)}">${esc(pc.name)}<button type="button" class="icon-btn" data-writes="${REMOVE_PRESENT_CHARACTER}" data-remove-pj="${esc(pc.id)}" aria-label="Retirer ${esc(pc.name)}">\xD7</button></span>`
    ).join("");
  }
  function presentAdd(board) {
    const absent = absentPcs(board);
    if (absent.length === 0) return "";
    return `<div class="kit-select"><span id="scenes-present-label">Ajouter un PJ</span><select id="scenes-present-select" aria-labelledby="scenes-present-label">${absent.map((pc) => `<option value="${esc(pc.id)}">${esc(pc.name)}</option>`).join("")}</select></div>` + buttons(button(` id="scenes-present-submit" data-writes="${ADD_PRESENT_CHARACTER}"`, "Ajouter", "secondary"));
  }
  var link = (cls, href, label, id) => `<a class="chip chip--${cls}" href="${esc(href)}" id="${esc(id)}">${esc(label)}</a>`;
  function chips(scene, board) {
    const character = (id) => board.characters.find((c) => c.id === id)?.name ?? id;
    const place = (id) => board.places.find((p) => p.id === id)?.name ?? id;
    const event = (id) => board.events.find((e) => e.id === id)?.name ?? id;
    const mediaTitle = (id) => [...board.medias.battlemaps, ...board.medias.illustrations].find((m) => m.id === id)?.title ?? id;
    const rows = [
      ["Lieux", scene.places.map((id) => link("lieu", `fiche-entite.html?id=${encodeURIComponent(id)}`, place(id), `${scene.id}-place-${id}`))],
      ["PNJ", scene.characters.map((id) => link("pnj", `fiche-entite.html?id=${encodeURIComponent(id)}`, character(id), `${scene.id}-char-${id}`))],
      ["\xC9v\xE9nements", scene.events.map((id) => link("media", `fiche-entite.html?id=${encodeURIComponent(id)}`, event(id), `${scene.id}-event-${id}`))],
      ["M\xE9dias", scene.media.map((id) => link("media", "medias.html", mediaTitle(id), `${scene.id}-media-${id}`))],
      ["\xC0 cr\xE9er", scene.toCreate.map((id) => link("todo", `#to-create-${id}`, id, `${scene.id}-todo-${id}`))]
    ];
    const shown2 = rows.filter(([, items]) => items.length > 0);
    return shown2.length === 0 ? "" : `<dl class="mf-scenes__links">${shown2.map(([name, items]) => `<dt>${name}</dt><dd>${items.join("")}</dd>`).join("")}</dl>`;
  }
  var notes = (scene) => `<div class="kit-textarea is-wide"><label class="kit-textarea__label" for="scene-${esc(scene.id)}-notes">Notes</label><textarea class="kit-textarea__field" id="scene-${esc(scene.id)}-notes" rows="2" readonly>${esc(scene.notes)}</textarea><div class="kit-textarea__footer"><p class="kit-textarea__hint"></p></div></div>`;
  var plural = (n, one, many) => n > 1 ? many : one;
  function mapLine(scene, board) {
    const id = `${esc(scene.id)}-battlemap`;
    if (scene.battlemap !== null) {
      const instance = board.instances[scene.battlemap];
      const map = instance === void 0 ? void 0 : board.medias.battlemaps.find((m) => m.id === instance.battlemap);
      const hidden = instance !== void 0 && instance.hidden > 0 ? ` dont ${instance.hidden} ${plural(instance.hidden, "cach\xE9", "cach\xE9s")}` : "";
      const detail = instance === void 0 ? "instance de cette sc\xE8ne" : `instance de cette sc\xE8ne \xB7 ${instance.tokens} ${plural(instance.tokens, "token plac\xE9", "tokens plac\xE9s")}${hidden}`;
      return `<div class="mf-scenes__map is-linked" id="${id}" data-reads="${GET_BATTLEMAP_INSTANCE}"><span><b>${esc(map?.title ?? scene.battlemap)}</b> <span class="small muted">\xB7 ${esc(detail)}</span></span>` + buttons(button(' data-action="place"', "Placer les personnages", "secondary")) + "</div>";
    }
    const maps = board.medias.battlemaps;
    const create = maps.length === 0 ? '<span class="small muted">La campagne n\u2019a pas de battlemap \xE0 associer.</span>' : `<div class="row"><div class="kit-select"><span id="${id}-label">Battlemap</span><select id="${id}-select" data-map-select aria-labelledby="${id}-label">${maps.map((m) => `<option value="${esc(m.id)}">${esc(m.title)}</option>`).join("")}</select></div>` + buttons(button(` data-create-instance data-writes="${CREATE_BATTLEMAP_INSTANCE}"`, "Cr\xE9er l\u2019instance", "secondary")) + "</div>";
    return `<div class="mf-scenes__map" id="${id}"><span class="muted small">Aucune battlemap pour cette sc\xE8ne</span>${create}<span class="small muted">Une nouvelle instance de la carte est cr\xE9\xE9e pour cette sc\xE8ne : ses tokens et son combat lui sont propres.</span></div>`;
  }
  function sceneList(board) {
    const last = board.scenes.length - 1;
    return board.scenes.map(
      (scene, index) => `<li class="mf-scenes__scene" id="scene-${esc(scene.id)}" data-scene="${esc(scene.id)}"><div class="mf-scenes__order"><button type="button" class="icon-btn" data-writes="${REORDER_SCENES}" data-move="-1" aria-label="Monter"${index === 0 ? " disabled" : ""}>\u25B2</button><b>${index + 1}</b><button type="button" class="icon-btn" data-writes="${REORDER_SCENES}" data-move="1" aria-label="Descendre"${index === last ? " disabled" : ""}>\u25BC</button></div><div class="mf-scenes__body"><h3>${esc(scene.title)}</h3>${chips(scene, board)}${notes(scene)}${mapLine(scene, board)}</div></li>`
    ).join("");
  }

  // apps/session/scene-board/src/style.ts
  var STYLE = `
.mf-scenes { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-scenes *, .mf-scenes *::before, .mf-scenes *::after { box-sizing: inherit; }
.mf-scenes h2, .mf-scenes h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-scenes h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-scenes h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-scenes p { margin: 0; }
.mf-scenes.stack, .mf-scenes .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-scenes .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-scenes .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-scenes .muted { color: var(--color-text-muted); }
.mf-scenes .small { font-size: var(--font-size-small); }
.mf-scenes .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-scenes .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); text-decoration: none; cursor: pointer; }
.mf-scenes .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-scenes .chip--todo { border-style: dashed; border-color: var(--color-accent); color: var(--color-accent); }
.mf-scenes .chip--pj { border-color: var(--color-success); }
.mf-scenes .chip--pnj { border-color: var(--color-secondary); }
.mf-scenes .chip--lieu { border-color: var(--color-accent); }
.mf-scenes .chip--media { border-color: var(--color-primary); }
.mf-scenes .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-scenes .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-scenes .icon-btn:disabled { opacity: 0.55; cursor: not-allowed; border-color: var(--color-border); color: var(--color-text-muted); }
.mf-scenes .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-scenes .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-scenes .kit-textarea.is-wide, .mf-scenes .kit-select.is-wide { max-width: none; }
.mf-scenes .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-scenes .kit-select select:hover { border-color: var(--color-primary); }
.mf-scenes .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-scenes__notice:empty { display: none; }
.mf-scenes__notice { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-danger); color: var(--color-danger); }
.mf-scenes__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-3); }
.mf-scenes__scene { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-scenes__order { display: grid; gap: var(--space-1); justify-items: center; align-content: start; }
.mf-scenes__order b { font-family: var(--font-family-heading); font-size: var(--font-size-heading); color: var(--color-primary); line-height: 1; }
.mf-scenes__body { display: grid; gap: var(--space-2); min-width: 0; }
.mf-scenes__links { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-1) var(--space-3); align-items: center; font-size: var(--font-size-small); margin: 0; }
.mf-scenes__links dt { color: var(--color-text-muted); text-transform: uppercase; letter-spacing: 0.05em; }
.mf-scenes__links dd { margin: 0; display: flex; flex-wrap: wrap; gap: var(--space-1); }
.mf-scenes__map { display: flex; flex-wrap: wrap; gap: var(--space-2); align-items: center; justify-content: space-between; padding: var(--space-2) var(--space-3); background: var(--color-surface-2); border-radius: var(--radius-medium); border: var(--border-width) dashed var(--color-border); }
.mf-scenes__map.is-linked { border-style: solid; border-color: var(--color-success); }
`;

  // apps/session/scene-board/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="SceneBoard"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "SceneBoard";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, props2, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`SceneBoard: #${id} is not in the board`);
      return found;
    };
    const wanted = sessionAsked(props2.session, shell2.context?.get("session"));
    let board = null;
    let busy = false;
    const say = (text2) => {
      part("scenes-notice").textContent = text2;
    };
    const paint = () => {
      if (board === null) return;
      part("scenes-title").textContent = sceneTitle(board);
      part("scenes-present-count").textContent = presentCount(board);
      part("scenes-present-list").innerHTML = presentList(board);
      part("scenes-present-add").innerHTML = presentAdd(board);
      part("scenes-list").innerHTML = sceneList(board);
    };
    async function read() {
      const session = parseSession((await shell2.capability(GET_SESSION, void 0, { id: wanted })).data);
      const campaign = campaignAsked(session.campaign, props2.campaign);
      const forCampaign = campaign === null ? {} : { campaign };
      const [scenes, characters, places, events, medias2] = await Promise.all([
        shell2.capability(LIST_SCENES, void 0, { session: wanted }).then((answer) => parseScenes(answer.data)),
        shell2.capability(LIST_CHARACTERS, void 0, forCampaign).then((answer) => parseCharacters(answer.data)),
        shell2.capability(LIST_PLACES, void 0, forCampaign).then((answer) => parseEntities(answer.data)),
        shell2.capability(LIST_EVENTS, void 0, forCampaign).then((answer) => parseEntities(answer.data)),
        shell2.capability(LIST_MEDIA, void 0, forCampaign).then((answer) => parseMedias(answer.data))
      ]);
      const instances = {};
      await Promise.all(
        scenes.flatMap(
          (scene) => scene.battlemap === null ? [] : [
            shell2.capability(GET_BATTLEMAP_INSTANCE, void 0, { instance: scene.battlemap, view: "dm" }).then((answer) => parseInstance(answer.data)).catch(() => null).then((instance) => {
              if (instance !== null) instances[instance.id] = instance;
            })
          ]
        )
      );
      return { session, scenes, characters, places, events, medias: medias2, instances };
    }
    async function send(dataCapability, payload, aggregate, id, key) {
      const answer = await shell2.command(commandBody(dataCapability, payload, { aggregate, id }, key));
      const commandSent = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && commandSent !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(commandSent));
      }
      return state;
    }
    async function act(wrong, run) {
      if (busy || board === null) return;
      if (board.session.id === "") return say("La s\xE9ance n\u2019est pas identifi\xE9e : rien ne peut \xEAtre modifi\xE9.");
      busy = true;
      host2.querySelector("#scene-board")?.setAttribute("aria-busy", "true");
      say("");
      try {
        const state = await run();
        if (state.status === "rejected") say(`Action refus\xE9e : ${reasons(state.violations)}.`);
        else {
          if (state.status !== "applied") say(`L\u2019action n\u2019est pas encore appliqu\xE9e (\xE9tat : ${state.status || "inconnu"}).`);
          try {
            board = await read();
            paint();
          } catch {
            say("L\u2019action a eu lieu, mais la s\xE9ance n\u2019a pas pu \xEAtre relue.");
          }
        }
      } catch {
        say(wrong);
      } finally {
        busy = false;
        host2.querySelector("#scene-board")?.removeAttribute("aria-busy");
      }
    }
    host2.addEventListener("click", (event) => {
      const clicked = event.target?.closest("button");
      if (!clicked || board === null) return;
      const current = board;
      const session = current.session.id;
      switch (clicked.dataset.writes) {
        case REMOVE_PRESENT_CHARACTER: {
          const character = clicked.dataset.removePj ?? "";
          void act("Impossible de retirer ce PJ.", () => send(REMOVE_PRESENT_CHARACTER, { session, character }, "Session", session));
          return;
        }
        case ADD_PRESENT_CHARACTER: {
          const character = host2.querySelector("#scenes-present-select")?.value ?? "";
          if (character === "") return;
          void act("Impossible d\u2019ajouter ce PJ.", () => send(ADD_PRESENT_CHARACTER, { session, character }, "Session", session));
          return;
        }
        case ADD_SCENE:
          void act("Impossible d\u2019ajouter une sc\xE8ne.", () => send(ADD_SCENE, { session, title: "Nouvelle sc\xE8ne" }, "Session", session));
          return;
        case REORDER_SCENES: {
          const order = moved(current.scenes, clicked.closest("[data-scene]")?.dataset.scene ?? "", Number(clicked.dataset.move));
          if (order === null) return;
          void act("Impossible de r\xE9ordonner les sc\xE8nes.", () => send(REORDER_SCENES, { session, order: ranks(order) }, "Session", session));
          return;
        }
        case CREATE_BATTLEMAP_INSTANCE: {
          const item = clicked.closest("[data-scene]");
          const scene = current.scenes.find((s) => s.id === item?.dataset.scene);
          const battlemap = item?.querySelector("select[data-map-select]")?.value ?? "";
          if (!scene || scene.battlemap !== null || battlemap === "") return;
          void act("Impossible de cr\xE9er l\u2019instance.", () => send(CREATE_BATTLEMAP_INSTANCE, { session, scene: scene.id, battlemap }, "BattlemapInstance", scene.id, `${CREATE_BATTLEMAP_INSTANCE}:${scene.id}:${Date.now()}`));
          return;
        }
      }
      if (clicked.dataset.action === "place") host2.ownerDocument.getElementById("scene-battlemap")?.scrollIntoView({ behavior: "smooth", block: "start" });
    });
    if (wanted === "") {
      say("Aucune s\xE9ance n\u2019est indiqu\xE9e : rien \xE0 afficher.");
      return;
    }
    read().then(
      (loaded) => {
        board = loaded;
        paint();
      },
      () => say("Impossible de lire la s\xE9ance.")
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
  var json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  var notFound = () => json({ error: "not-found" }, 404);
  var refused = (why) => json({ error: why }, 409);
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
    const answer = (entry, replayed = false) => json({ commandId: entry.commandId, partition: entry.partition, replayed, warnings: replayed ? [] : entry.warnings || [], result: resultOf(entry) }, 202);
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
      const aggregate = target.aggregate || null;
      const declared = (aggregate && await s.load(`aggregates/${aggregate}`) || { fields: {} }).fields || {};
      const derived = Object.keys(payload).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      const known = (await s.manifests())[dataCapability];
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
      try {
        await s.ready;
        if (s.problems.length) return json({ error: `the mock server did not start: ${s.problems.join("; ")}` }, 500);
        return await respond(input, init);
      } catch (e) {
        if (!(e instanceof HandlerError)) throw e;
        console.error(e.message);
        return json({ error: e.message }, 500);
      }
    };
    const respond = async (input, init) => {
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

  // mock/src/SceneBoard.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
