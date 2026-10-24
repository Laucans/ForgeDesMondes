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

  // apps/media/media-library/src/ids.ts
  var LIST_MEDIA = "media.listMedia";
  var LIST_SCENES = "session.listScenes";
  var LIST_PLACES = "campaign.listPlaces";
  var UPLOAD_MEDIA = "media.uploadMedia";
  var SET_MEDIA_PLACE = "media.setMediaPlace";
  var ADD_PLACE_TO_SESSION = "session.addPlaceToSession";
  var READS = `${LIST_MEDIA} ${LIST_SCENES} ${LIST_PLACES}`;
  var DATA_CAPABILITY_VERSION = 1;
  var BATTLEMAP_CONTEXT = "battlemap";

  // apps/media/media-library/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var list = (value) => Array.isArray(value) ? value.filter(isRecord) : [];
  function medium(entry, kind) {
    const grid = isRecord(entry.grid) ? { orient: text(entry.grid.orient), size: Number(entry.grid.size) || 0 } : null;
    return {
      id: text(entry.id),
      kind,
      title: text(entry.title),
      file: text(entry.file),
      size: text(entry.size),
      width: kind === "battlemap" && Number(entry.width) > 0 ? Number(entry.width) : null,
      height: kind === "battlemap" && Number(entry.height) > 0 ? Number(entry.height) : null,
      place: text(entry.place) || null,
      art: text(entry.art) || (kind === "battlemap" ? "forest" : "ember"),
      glyph: text(entry.glyph) || (kind === "battlemap" ? "\u2B21" : ""),
      grid: kind === "battlemap" ? grid : null,
      ratio: typeof entry.ratio === "number" ? entry.ratio : null
    };
  }
  function parseMedia(data) {
    const answer = isRecord(data) ? data : {};
    return [...list(answer.battlemaps).map((m) => medium(m, "battlemap")), ...list(answer.illustrations).map((m) => medium(m, "illustration"))].filter((m) => m.id !== "");
  }
  function parsePlaces(data) {
    return list(data).map((p) => ({ id: text(p.id), name: text(p.name) })).filter((p) => p.id !== "");
  }
  function parseSessionPlaces(data) {
    return new Set(list(data).flatMap((scene) => texts(scene.places)));
  }
  function parseProps(raw) {
    const number = Number(raw.sessionNumber);
    return { campaign: text(raw.campaign), session: text(raw.session), sessionNumber: Number.isInteger(number) && number > 0 ? number : null };
  }
  function association(m, places) {
    if (m.place === null || places === null) return { kind: "none" };
    return places.has(m.place) ? { kind: "in", place: m.place } : { kind: "out", place: m.place };
  }
  var byKind = (media, kind) => media.filter((m) => m.kind === kind);
  function titleFor(draft, file) {
    return draft.title.trim() || (file === null ? "" : stem(file.name)) || "Sans titre";
  }
  var stem = (name) => name.replace(/\.[^.]+$/, "").replace(/[-_]+/g, " ");
  function commandBody(dataCapability, payload, target, idempotencyKey) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload,
      ...target === void 0 ? {} : { target },
      idempotencyKey,
      caller: { by: "dm", from: "MediaLibrary", roles: [] }
    };
  }
  var setPlaceCommand = (media, place, key) => commandBody(SET_MEDIA_PLACE, { media, place }, { aggregate: "Media", id: media }, key);
  var addPlaceCommand = (session, place, key) => commandBody(ADD_PLACE_TO_SESSION, { session, place }, { aggregate: "Session", id: session }, key);
  function uploadCommand(upload, key) {
    const payload = { campaign: upload.campaign, kind: upload.kind, title: upload.title, file: upload.file };
    if (upload.size !== null) Object.assign(payload, { width: upload.size.width, height: upload.size.height });
    if (upload.place !== null) payload.place = upload.place;
    return commandBody(UPLOAD_MEDIA, payload, void 0, key);
  }
  var VIOLATIONS = {
    "media-place-is-lieu": "ce lieu n\u2019est pas un lieu de la campagne",
    "scene-links-same-campaign": "ce lieu n\u2019est pas de la campagne de la s\xE9ance",
    "grid-only-for-battlemap": "seule une battlemap a une grille",
    "unhandled-data-capability": "cette action n\u2019est pas disponible"
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

  // apps/media/media-library/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var attr = (name, value) => value === "" ? "" : ` ${name}="${esc(value)}"`;
  function button(p) {
    return `<div class="kit-button kit-button--bare${p.small ? " kit-button--small" : ""}"><button type="button" class="${p.variant}"${attr("id", p.id ?? "")}${attr("data-action", p.action ?? "")}${attr("data-writes", p.writes ?? "")}${attr("title", p.title ?? "")}>${esc(p.label)}</button></div>`;
  }
  function select(p) {
    const id = esc(p.id);
    const options = [{ id: "", name: p.none }, ...p.options].map((o) => `<option value="${esc(o.id)}"${o.id === p.value ? " selected" : ""}>${esc(o.name)}</option>`).join("");
    return `<div class="kit-select is-wide"><span id="${id}-label">${esc(p.label)}</span><select id="${id}" aria-labelledby="${id}-label"${attr("data-writes", p.writes ?? "")}${attr("data-reads", p.reads ?? "")}>${options}</select></div>`;
  }
  function skeleton() {
    return `<section class="panel mf-media" id="media-library" aria-labelledby="media-title"><div class="panel__head"><h2 id="media-title">Biblioth\xE8que de m\xE9dias</h2><span class="small muted" id="media-count" data-reads="${READS}"></span></div><form class="mf-media__upload" id="media-upload"><div id="media-upload-drop">${fileDrop()}</div><div class="stack" id="media-upload-fields" data-reads="${LIST_PLACES}"></div></form><div class="mf-media__message" id="media-message" aria-live="polite"></div><section id="media-battlemaps" aria-labelledby="media-battlemaps-title" class="stack"><h3 id="media-battlemaps-title">Battlemaps</h3><ul class="mf-media__grid" id="media-battlemaps-list" data-reads="${READS}"></ul></section><section id="media-illustrations" aria-labelledby="media-illustrations-title" class="stack"><h3 id="media-illustrations-title">Illustrations</h3><ul class="mf-media__grid" id="media-illustrations-list" data-reads="${READS}"></ul></section></section>`;
  }
  var ICON_UPLOAD = '<svg class="kit-filedrop__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>';
  var TYPES = ["JPG", "PNG", "WEBP"];
  function fileDrop() {
    return `<div class="kit-filedrop is-wide" id="media-upload-drop-zone"><label class="kit-filedrop__zone" tabindex="0" id="media-upload-zone">${ICON_UPLOAD}<span class="kit-filedrop__title">D\xE9posez une image ou <b>parcourez</b></span><span class="kit-filedrop__hint">Battlemap pr\xE9construite ou illustration, jusqu\u2019\xE0 25 Mo</span><span class="kit-filedrop__types">${TYPES.map((t) => `<span class="kit-filedrop__tag">${t}</span>`).join("")}</span><input class="kit-filedrop__input" id="media-upload-file" type="file" accept="${TYPES.map((t) => `.${t.toLowerCase()}`).join(",")},.jpeg"></label><div class="kit-filedrop__error">Type de fichier non accept\xE9.</div><div class="kit-filedrop__file"><div class="kit-filedrop__thumb"></div><div class="kit-filedrop__info"><div class="kit-filedrop__name"></div><div class="kit-filedrop__row"><span class="kit-filedrop__size"></span></div></div></div></div>`;
  }
  function uploadFields(draft, places) {
    const radio = (kind, label, hint) => `<label><input type="radio" name="media-kind" value="${kind}"${draft.kind === kind ? " checked" : ""}><span>${label}</span><small>${hint}</small></label>`;
    return '<div class="kit-radio"><fieldset id="media-upload-kind"><legend>Type</legend>' + radio("battlemap", "Battlemap", "grille hexagonale") + radio("illustration", "Illustration", "sc\xE8ne ou fen\xEAtre") + "</fieldset></div>" + select({ id: "media-upload-place", label: "Lieu (facultatif)", value: draft.place, options: places, none: "Aucun lieu" }) + `<div class="kit-input is-wide"><label>Titre<input type="text" id="media-upload-title" placeholder="La cave du Sanglier"${attr("value", draft.title)}></label></div>` + button({ id: "media-upload-submit", label: "Ajouter \xE0 la campagne", variant: "primary", writes: UPLOAD_MEDIA });
  }
  var ratio = (value) => String(value).replace(".", ",");
  function caption(m) {
    const weight = m.size !== "" ? m.size : m.width !== null && m.height !== null ? `${m.width} \xD7 ${m.height} px` : "";
    const grid = m.kind === "battlemap" ? m.grid === null ? "" : `grille ${m.grid.orient === "pointy" ? "pointe" : "c\xF4t\xE9 plat"} ${m.grid.size} px` : m.ratio === null ? "" : `ratio ${ratio(m.ratio)}`;
    return [m.file, weight, grid].filter((part) => part !== "").join(" \xB7 ");
  }
  function placeLink(m, places, sessionPlaces, sessionNumber) {
    const standing = association(m, sessionPlaces);
    const id = `id="media-session-${esc(m.id)}"`;
    const number = sessionNumber === null ? "" : ` ${sessionNumber}`;
    const name = (place) => places.find((p) => p.id === place)?.name ?? place;
    switch (standing.kind) {
      case "in":
        return `<small ${id}><span class="chip chip--lieu">Associ\xE9 \xE0 la s\xE9ance${number}</span> via ${esc(name(standing.place))}</small>`;
      case "out":
        return `<small class="muted" ${id}>Lieu hors de la s\xE9ance${number}</small>` + button({ label: "Ajouter le lieu \xE0 la s\xE9ance", variant: "ghost", small: true, action: `add-place:${standing.place}`, writes: ADD_PLACE_TO_SESSION, title: `Ajouter \xAB ${name(standing.place)} \xBB \xE0 la s\xE9ance${number}` });
      case "none":
        return sessionPlaces === null ? "" : `<small class="muted" ${id}>Sans lieu : non rattach\xE9 \xE0 la s\xE9ance</small>`;
    }
  }
  var withPlace = (places, place) => place === null || places.some((p) => p.id === place) ? places : [...places, { id: place, name: place }];
  function card(m, places, sessionPlaces, sessionNumber, selected) {
    return `<li class="mf-media__item${selected ? " is-selected" : ""}" id="media-${esc(m.id)}" data-id="${esc(m.id)}"><div class="mf-media__thumb"><div class="art art--${esc(m.art)}" role="img" aria-label="${esc(m.title)}"><span class="art__glyph">${esc(m.glyph || m.title.charAt(0).toUpperCase())}</span></div></div><b>${esc(m.title)}</b><small>${esc(caption(m))}</small>` + select({ id: `media-place-${m.id}`, label: "Lieu", value: m.place ?? "", options: withPlace(places, m.place), none: "Aucun lieu", writes: SET_MEDIA_PLACE }) + placeLink(m, places, sessionPlaces, sessionNumber) + (m.kind === "battlemap" ? button({ label: "R\xE9gler la grille", variant: selected ? "primary" : "secondary", small: true, action: `grid:${m.id}` }) : "") + "</li>";
  }
  function count(battlemaps, illustrations) {
    const plural = (n, one) => `${n} ${one}${n > 1 ? "s" : ""}`;
    return `${plural(battlemaps, "battlemap")} \xB7 ${plural(illustrations, "illustration")}`;
  }
  var message = (kind, words) => `<p class="is-${kind}" id="media-message-text">${esc(words)}</p>`;

  // apps/media/media-library/src/style.ts
  var STYLE = `
.mf-media { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-media *, .mf-media *::before, .mf-media *::after { box-sizing: inherit; }
.mf-media h2, .mf-media h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-media h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-media h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-media .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-media .muted { color: var(--color-text-muted); }
.mf-media .small { font-size: var(--font-size-small); }
.mf-media .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-media .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); }
.mf-media .chip--lieu { border-color: var(--color-accent); }
.mf-media .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-media .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-media .kit-input.is-wide, .mf-media .kit-select.is-wide, .mf-media .kit-filedrop.is-wide { max-width: none; }
.mf-media .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-media .kit-select select:hover { border-color: var(--color-primary); }
.mf-media .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-media .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-media .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-media .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-media .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-media .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-media .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-media .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-media .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-media .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-media .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-media__upload { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: var(--space-3); align-items: start; }
.mf-media__grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 4.5), 1fr)); gap: var(--space-3); }
.mf-media__item { display: grid; gap: var(--space-1); padding: var(--space-2); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-media__item.is-selected { border-color: var(--color-primary); box-shadow: var(--shadow-high); }
.mf-media__thumb { height: calc(var(--space-5) * 2.5); border-radius: var(--radius-small); overflow: hidden; }
.mf-media__item b { font-size: var(--font-size-small); }
.mf-media__item small { color: var(--color-text-muted); }
.mf-media__message { min-height: var(--space-4); }
.mf-media__message p { margin: 0; padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-media__message .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-media__message .is-error { border-color: var(--color-danger); color: var(--color-danger); }
@media (max-width: 900px) { .mf-media__upload { grid-template-columns: minmax(0, 1fr); } }
`;

  // apps/media/media-library/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  var BROWSER = {
    key: () => crypto.randomUUID(),
    async measure(file) {
      if (typeof createImageBitmap !== "function") return null;
      try {
        const bitmap = await createImageBitmap(file);
        const size = { width: bitmap.width, height: bitmap.height };
        bitmap.close();
        return size;
      } catch {
        return null;
      }
    }
  };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="MediaLibrary"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "MediaLibrary";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var fmtSize = (bytes) => {
    if (bytes < 1024) return `${bytes} o`;
    if (bytes < 1048576) return `${(bytes / 1024).toFixed(1).replace(".", ",")} Ko`;
    return `${(bytes / 1048576).toFixed(1).replace(".", ",")} Mo`;
  };
  var accepted = (name) => TYPES.some((type) => name.toLowerCase().endsWith(`.${type.toLowerCase()}`)) || /\.jpeg$/i.test(name);
  function mount(host2, rawProps, shell2, timing = WAITING, io = BROWSER) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`MediaLibrary: #${id} is not in the card`);
      return found;
    };
    let props2 = parseProps(rawProps);
    let media = [];
    let places = [];
    let sessionPlaces = null;
    let selected = null;
    let file = null;
    let draft = { kind: "battlemap", title: "", place: "" };
    let busy = false;
    let ticket = 0;
    let asOf = 0;
    let dirty = false;
    let newest = 0;
    let autoTitle = "";
    const say = (kind, words = "") => {
      part("media-message").innerHTML = kind === "none" ? "" : message(kind, words);
    };
    const paintCards = () => {
      const cards = (list2) => list2.map((m) => card(m, places, sessionPlaces, props2.sessionNumber, m.id === selected)).join("");
      const battlemaps = byKind(media, "battlemap");
      const illustrations = byKind(media, "illustration");
      part("media-battlemaps-list").innerHTML = cards(battlemaps);
      part("media-illustrations-list").innerHTML = cards(illustrations);
      part("media-count").textContent = count(battlemaps.length, illustrations.length);
    };
    const paintUpload = () => {
      part("media-upload-fields").innerHTML = uploadFields(draft, places);
    };
    const paintAll = () => {
      paintCards();
      paintUpload();
    };
    const lock = (locked) => {
      for (const control of host2.querySelectorAll("#media-library input, #media-library select, #media-library button")) control.disabled = locked;
    };
    async function read() {
      const mine = ticket += 1;
      const withSession = props2.session !== "";
      if (props2.campaign === "") {
        media = [];
        places = [];
        sessionPlaces = null;
        selected = null;
        shell2.context?.set(BATTLEMAP_CONTEXT, "");
        return "Aucune campagne n\u2019est ouverte.";
      }
      const [library, campaignPlaces, scenes] = await Promise.allSettled([
        shell2.capability(LIST_MEDIA, void 0, { campaignId: props2.campaign }),
        shell2.capability(LIST_PLACES, void 0, { campaign: props2.campaign }),
        withSession ? shell2.capability(LIST_SCENES, void 0, { session: props2.session }) : Promise.resolve(null)
      ]);
      if (mine !== ticket) return null;
      if (library.status === "rejected") throw new Error("the library cannot be read");
      asOf = Math.max(0, ...[library, campaignPlaces, scenes].flatMap((r) => r.status === "fulfilled" && r.value !== null ? [r.value.asOf] : []));
      media = parseMedia(library.value.data);
      places = campaignPlaces.status === "fulfilled" ? parsePlaces(campaignPlaces.value.data) : [];
      sessionPlaces = withSession && scenes.status === "fulfilled" && scenes.value !== null ? parseSessionPlaces(scenes.value.data) : null;
      if (selected === null || !media.some((m) => m.id === selected)) {
        selected = byKind(media, "battlemap")[0]?.id ?? null;
        shell2.context?.set(BATTLEMAP_CONTEXT, selected ?? "");
      }
      if (campaignPlaces.status === "rejected") return "Les lieux n\u2019ont pas pu \xEAtre lus.";
      if (withSession && scenes.status === "rejected") return "Les sc\xE8nes de la s\xE9ance n\u2019ont pas pu \xEAtre lues.";
      return "";
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
    async function act(wrong, body, done) {
      if (busy) return "failed";
      busy = true;
      const focused = host2.ownerDocument.activeElement?.id ?? "";
      lock(true);
      let outcome = "failed";
      try {
        const state = await send(await body());
        if (state.status === "rejected") say("error", `${wrong} Refus\xE9 : ${reasons(state.violations)}.`);
        else if (!isFinal(state.status) && state.status !== "") {
          outcome = "pending";
          say("busy", `La commande est en file d\u2019attente (\xE9tat : ${state.status}) : elle s\u2019appliquera d\xE8s que possible.`);
          await read().catch(() => "");
        } else if (state.status !== "applied") say("error", `${wrong} (\xE9tat : ${state.status || "inconnu"}).`);
        else {
          outcome = "applied";
          let unread = "";
          try {
            unread = await read() ?? "";
          } catch {
            unread = "La biblioth\xE8que n\u2019a pas pu \xEAtre relue.";
          }
          say(unread === "" ? "ok" : "error", unread === "" ? done() : `${done()} ${unread}`);
        }
      } catch {
        say("error", wrong);
      } finally {
        busy = false;
        paintAll();
        lock(false);
        if (focused !== "") host2.ownerDocument.getElementById(focused)?.focus();
        if (dirty || newest > asOf) {
          dirty = false;
          refresh();
        }
      }
      return outcome;
    }
    const selectBattlemap = (id) => {
      selected = id;
      paintCards();
      shell2.context?.set(BATTLEMAP_CONTEXT, id);
    };
    const changePlace = (mediumId, value) => {
      const m = media.find((x) => x.id === mediumId);
      const place = value === "" ? null : value;
      if (m === void 0 || m.place === place) return Promise.resolve("failed");
      const name = places.find((p) => p.id === place)?.name ?? place;
      return act("Le lieu n\u2019a pas pu \xEAtre chang\xE9.", () => Promise.resolve(setPlaceCommand(m.id, place, io.key())), () => place === null ? `\xAB ${m.title} \xBB n\u2019a plus de lieu.` : `\xAB ${m.title} \xBB est rattach\xE9 au lieu \xAB ${name ?? ""} \xBB.`);
    };
    const addPlace = (place) => {
      const name = places.find((p) => p.id === place)?.name ?? place;
      return act("Le lieu n\u2019a pas pu \xEAtre ajout\xE9 \xE0 la s\xE9ance.", () => Promise.resolve(addPlaceCommand(props2.session, place, io.key())), () => `\xAB ${name} \xBB est dans la s\xE9ance : ses m\xE9dias y sont associ\xE9s.`);
    };
    async function upload() {
      if (busy) return;
      if (props2.campaign === "") return say("error", "Aucune campagne n\u2019est ouverte : le m\xE9dia ne peut pas lui \xEAtre ajout\xE9.");
      if (file === null) return say("error", "Choisissez d\u2019abord une image \xE0 t\xE9l\xE9verser.");
      const chosen = file;
      const title = titleFor(draft, chosen);
      const before = new Set(media.map((m) => m.id));
      const { kind } = draft;
      const { campaign: campaign2 } = props2;
      const place = draft.place === "" ? null : draft.place;
      const sent = async () => uploadCommand({ campaign: campaign2, kind, title, file: chosen.name, place, size: await io.measure(chosen) }, io.key());
      const outcome = await act("Le t\xE9l\xE9versement a \xE9chou\xE9.", sent, () => `\xAB ${title} \xBB est ajout\xE9 \xE0 la campagne.`);
      if (outcome === "failed") return;
      file = null;
      draft = { ...draft, title: "", place: "" };
      autoTitle = "";
      part("media-upload-drop").innerHTML = fileDrop();
      paintUpload();
      const fresh = byKind(media, "battlemap").find((m) => !before.has(m.id));
      if (fresh !== void 0) selectBattlemap(fresh.id);
    }
    function choose(chosen) {
      if (chosen === void 0) return;
      const zone = part("media-upload-drop-zone");
      if (!accepted(chosen.name)) {
        file = null;
        zone.classList.remove("has-file");
        zone.classList.add("has-error");
        return;
      }
      file = chosen;
      zone.classList.remove("has-error");
      zone.classList.add("has-file");
      const text2 = (selector, value) => {
        const target = zone.querySelector(selector);
        if (target) target.textContent = value;
      };
      text2(".kit-filedrop__name", chosen.name);
      text2(".kit-filedrop__size", fmtSize(chosen.size));
      text2(".kit-filedrop__thumb", (chosen.name.split(".").pop() ?? "").toUpperCase());
      if (draft.title.trim() === "" || draft.title === autoTitle) {
        autoTitle = stem(chosen.name);
        draft = { ...draft, title: autoTitle };
        const field = host2.querySelector("#media-upload-title");
        if (field) field.value = draft.title;
      }
    }
    host2.addEventListener("submit", (event) => event.preventDefault());
    host2.addEventListener("change", (event) => {
      const target = event.target;
      if (target?.dataset?.writes === SET_MEDIA_PLACE) {
        const li = target.closest?.("li[data-id]");
        if (li?.dataset.id !== void 0) void changePlace(li.dataset.id, target.value ?? "");
      } else if (target?.id === "media-upload-file") choose(target.files?.[0]);
      else if (target?.id === "media-upload-place") draft = { ...draft, place: target.value ?? "" };
      else if (target?.name === "media-kind") draft = { ...draft, kind: target.value === "illustration" ? "illustration" : "battlemap" };
    });
    host2.addEventListener("input", (event) => {
      const target = event.target;
      if (target?.id === "media-upload-title") draft = { ...draft, title: target.value ?? "" };
    });
    host2.addEventListener("dragover", (event) => event.preventDefault());
    host2.addEventListener("drop", (event) => {
      if (!event.target?.closest("#media-upload-zone")) return;
      event.preventDefault();
      choose(event.dataTransfer?.files[0]);
    });
    host2.addEventListener("click", (event) => {
      const button2 = event.target?.closest("button");
      if (!button2) return;
      const action = button2.dataset.action ?? "";
      if (button2.dataset.writes === UPLOAD_MEDIA) void upload();
      else if (button2.dataset.writes === ADD_PLACE_TO_SESSION && action.startsWith("add-place:")) void addPlace(action.slice("add-place:".length));
      else if (action.startsWith("grid:")) selectBattlemap(action.slice("grid:".length));
    });
    function refresh() {
      lock(true);
      read().then(
        (unread) => {
          if (unread === null) return;
          paintAll();
          say(unread === "" ? "none" : "error", unread);
          lock(false);
        },
        () => {
          say("error", "Impossible de lire la biblioth\xE8que de m\xE9dias.");
          lock(false);
        }
      );
    }
    const following = {};
    following.stop = shell2.onDataVersion?.((version) => {
      newest = Math.max(newest, version);
      if (!host2.isConnected) following.stop?.();
      else if (!busy && version > asOf) refresh();
    });
    paintAll();
    say("busy", "Chargement des m\xE9dias\u2026");
    refresh();
    return {
      update(next) {
        const before = props2;
        props2 = parseProps(next);
        if (props2.campaign === before.campaign && props2.session === before.session && props2.sessionNumber === before.sessionNumber) return;
        if (busy) dirty = true;
        else refresh();
      },
      destroy() {
        following.stop?.();
      }
    };
  }

  // mock/src/screen.js
  var context = () => window.__mockContext = window.__mockContext || {};
  function setContext(name, value) {
    const ctx = context();
    if (ctx[name] === value) return;
    ctx[name] = value;
    document.dispatchEvent(new CustomEvent("mock:context", { detail: { name, value } }));
  }
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
  var normalized = (term) => String(term ?? "").trim().toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "").normalize("NFC");
  var LISTS = { pj: "campaign.listCharacters", pnj: "campaign.listCharacters", lieu: "campaign.listPlaces", evenement: "campaign.listEvents" };
  var SAVED = ["kind", "name", "aliases", "description", "notes", "player", "prompt", "background", "behavior", "stats", "ca", "pvMax", "spells", "features", "actions", "portrait", "token", "crop", "color"];
  var elementOf = async (server, id) => {
    for (const name of new Set(Object.values(LISTS))) {
      const element = (await server.load(name) || []).find((e) => e.id === id);
      if (element) return { element };
    }
    return {};
  };
  var changedBy = async (server, element, patch, origin, source, changes) => {
    Object.assign(element, patch);
    const history = await server.load("campaign.listEntityVersions") || {};
    const entry = history[element.id] || (history[element.id] = { name: element.name, versions: [] });
    const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
    entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin, source, restoredFrom: null, seen: false, changes });
  };
  var soundTags = (tags) => [...new Set((Array.isArray(tags) ? tags : []).map((t) => [...String(t).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "")].map((c) => c.toLowerCase()).join("")).filter(Boolean))];
  var COMMANDS = {
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
      const campaign2 = (await server.load("campaign.listCampaigns") || []).find((c) => c.id === payload.campaign);
      const present = Array.isArray(payload.present) ? payload.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
      if (campaign2 && campaign2.type === "oneshot" && (campaign2.sessions ?? 0) > 0) return ["oneshot-single-session"];
      if (campaign2) campaign2.sessions = (campaign2.sessions ?? 0) + 1;
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
      const base = { id: server.uuid(), title: String(payload.title || "").trim() || "Sans titre", file: payload.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      for (const name of ["media", "media.listMedia"]) {
        const media = await server.load(name);
        if (!media) continue;
        if (kind === "battlemap") media.battlemaps.push({ ...base, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 } });
        else media.illustrations.push({ ...base, ratio: payload.width > 0 && payload.height > 0 ? Math.round(payload.width / payload.height * 100) / 100 : null });
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
      for (const medium2 of found) medium2.place = place;
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
      const scene = payload.scene ? scenes.find((sc) => sc.id === payload.scene) : first;
      if (!scene) return ["not-found"];
      if (!(await server.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["scene-links-same-campaign"];
      if (!scene.places.includes(place)) scene.places.push(place);
      const library = await server.load("media.listMedia") || { battlemaps: [], illustrations: [] };
      for (const medium2 of library.battlemaps.concat(library.illustrations)) {
        if (medium2.place === place && !scene.media.includes(medium2.id)) scene.media.push(medium2.id);
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
      const scene = (await server.load("session.listScenes")).find((sc) => sc.id === payload.scene);
      const map = (await server.load("media.listMedia")).battlemaps.find((m) => m.id === payload.battlemap);
      if (!session || session.id !== payload.session || !scene || !map) return ["not-found"];
      if (scene.battlemap) return ["one-instance-per-scene"];
      const id = `bmi-${map.id}-${scene.id}`;
      server.instances = server.instances || /* @__PURE__ */ new Map();
      server.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
      scene.battlemap = id;
      return void 0;
    },
    // table.placeTokens (crates/table/data-capabilities/place-tokens): a
    // character dropped on the instance `targetId` (or `payload.instance`) at a
    // hex, `count` times (1 to 12, 1 by default). An NPC's tokens are numbered
    // after the highest its tokens hold (`multi-drop-numbering`), named « Bandit
    // 4 », and copy the sheet's HP max as their HP; a PC is dropped once, only
    // when present at the session, with no number and no HP (`token-roster`,
    // `pj-hp-not-tracked`). Every token stands on the hex, visible, with no
    // condition. An unknown instance or character is `not-found`; a hex that is
    // no pair of whole numbers is refused by `single-position-source`.
    "table.placeTokens": async (server, { payload, targetId }) => {
      const count2 = payload.count ?? 1;
      if (!Number.isInteger(payload.q) || !Number.isInteger(payload.r)) return ["single-position-source"];
      if (!Number.isInteger(count2) || count2 < 1 || count2 > 12) return ["multi-drop-numbering"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instance = id === fixture.id ? fixture : server.instances && server.instances.get(id);
      const character = (await server.load("campaign.listCharacters")).find((c) => c.id === payload.character);
      if (!instance || !character || !["pj", "pnj"].includes(character.kind)) return ["not-found"];
      const tokens = instance.state.tokens;
      const placed = tokens.filter((t) => t.char === character.id);
      const pnj = character.kind === "pnj";
      if (!pnj) {
        const session = await server.load("session.getSession");
        if (count2 !== 1 || placed.length || !session.present.includes(character.id)) return ["token-roster"];
      }
      let number = Math.max(0, ...placed.map((t) => t.num || 0));
      for (let i = 0; i < count2; i++) {
        const num = pnj ? ++number : null;
        tokens.push({
          id: server.uuid(),
          char: character.id,
          kind: character.kind,
          name: pnj ? `${character.name.trim()} ${num}` : character.name.trim(),
          num,
          q: payload.q,
          r: payload.r,
          visible: true,
          hp: pnj ? character.pvMax ?? null : null,
          hpMax: pnj ? character.pvMax ?? null : null,
          states: [],
          dead: false
        });
      }
      if (!instance.roster.includes(character.id)) instance.roster.push(character.id);
      return void 0;
    },
    // table.renameToken (crates/table/data-capabilities/rename-token): one token
    // renamed, `payload.token` on the instance `targetId` — or the token itself
    // as the target. Its number and the other tokens of its character are
    // untouched (`multi-drop-numbering`); a name of blanks alone is refused. Ids
    // are compared in any case, as the server does.
    "table.renameToken": async (server, { payload, targetId }) => {
      const same2 = (a, b) => String(a ?? "").toLowerCase() === String(b ?? "").toLowerCase();
      const name = typeof payload.name === "string" ? payload.name.trim() : "";
      if (!name) return ["multi-drop-numbering"];
      const tokenId = payload.token ?? targetId;
      const instance = payload.token != null && targetId && !same2(targetId, tokenId) ? targetId : null;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const inst = instances.find((i) => (instance == null || same2(i.id, instance)) && i.state.tokens.some((t) => same2(t.id, tokenId)));
      if (!inst) return ["not-found"];
      inst.state.tokens.find((t) => same2(t.id, tokenId)).name = name;
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
      const campaign2 = await server.load("campaign.getCampaign");
      if (!campaign2 || campaign2.id !== payload.id) return ["not-found"];
      const now = /* @__PURE__ */ new Date();
      const pad = (n) => String(n).padStart(2, "0");
      campaign2.lastExport = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
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
      const message2 = payload.mode === "api" && settings.testResults?.[provider]?.message || `Connexion r\xE9ussie (binaire ${provider})`;
      const latency = message2.match(/répond en ([\d,]+ s)/)?.[1] || "0,01 s";
      settings.lastTest = { ok: true, at, provider, latency, message: message2 };
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
    // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
    // one element per accepted proposal, the raw note kept as an import listed
    // newest first, the proposals no longer awaiting.
    "campaign.createEntitiesFromNote": async (server, { payload }) => {
      const proposals = server.noteProposals || [];
      const accepted2 = Array.isArray(payload.accepted) ? payload.accepted : [];
      const created = proposals.filter((p) => accepted2.includes(p.id));
      const fixture = await server.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture.imports;
      const note = server.noteText || "";
      imports.unshift({ id: `imp-${server.uuid()}`, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), title: note.split("\n")[0].replace(/[.\s]+$/, "") || "Notes import\xE9es", chars: note.length, proposed: proposals.length, created: created.length });
      server.noteProposals = [];
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
      const all = lists.flatMap(([name, list3]) => list3.map((element) => ({ name, element })));
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
      const list2 = await server.load(LISTS[kind]) || [];
      if (found && found.name !== LISTS[kind]) {
        const old = lists.find(([name]) => name === found.name)[1];
        old.splice(old.indexOf(before), 1);
      }
      const at = list2.indexOf(before);
      if (at >= 0) list2[at] = after;
      else list2.push(after);
      const history = await server.load("campaign.listEntityVersions") || {};
      const entry = history[id] || (history[id] = { name: after.name, versions: [] });
      entry.name = after.name;
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => f !== "id").sort().filter((f) => JSON.stringify(before[f] ?? null) !== JSON.stringify(after[f] ?? null)).map((field) => ({ field, old: before[field] ?? null, new: after[field] ?? null }));
      const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
      entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin: "dm", source: "Fiche enregistr\xE9e par le DM", seen: true, changes });
      return void 0;
    },
    // ai.proposeCharacter (crates/ai/data-capabilities/propose-character, planned): the
    // initial prompt told or typed gives the character's background and behaviour and, for an NPC,
    // a statblock — the proposals of fixtures/EntityEditor.json, the fake llm's answers. It applies
    // at once and is a version like any other (source ia), unseen until the DM looks. A prompt with
    // no text, or an element the lists do not hold, is refused.
    "ai.proposeCharacter": async (server, { payload, targetId }) => {
      const { element } = await elementOf(server, targetId);
      if (!element || !String(payload.prompt ?? "").trim()) return ["not-found"];
      const { proposals } = await server.load("EntityEditor");
      const proposal = proposals[element.id] || proposals.default;
      const patch = { prompt: payload.prompt, background: proposal.background, behavior: proposal.behavior };
      if (element.kind === "pnj") Object.assign(patch, { stats: { ...proposal.stats }, ca: proposal.ca, pvMax: proposal.pvMax, features: proposal.features.map((f) => ({ ...f })) });
      await changedBy(server, element, patch, "ia", "Prompt initial", element.kind === "pnj" ? proposal.changes : proposal.changes.slice(0, 1));
      return void 0;
    },
    // dndbeyond.resyncCharacter (crates/dndbeyond/data-capabilities/resync-character, planned): the
    // stats and hit points of a PC linked to D&D Beyond are read again — the patch of
    // fixtures/EntityEditor.json, whose changes the history keeps — and the aliases, the notes and the token are left as they are
    // (`dndbeyond-resync-scope`). A sheet with no link is refused.
    "dndbeyond.resyncCharacter": async (server, { targetId }) => {
      const { element } = await elementOf(server, targetId);
      if (!element || !element.ddb || !element.ddb.url) return ["not-found"];
      const { ddbResync } = await server.load("EntityEditor");
      const resync = ddbResync[element.id] || ddbResync.default;
      const { stats, pvMax } = resync.patch;
      await changedBy(server, element, { ...stats ? { stats: { ...element.stats, ...stats } } : {}, ...pvMax ? { pvMax } : {}, ddb: { ...element.ddb, syncedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19) } }, "dndbeyond", "Resynchronisation D&D Beyond", resync.changes.filter((c) => c.field === "PV max" || stats && c.field in stats));
      return void 0;
    },
    // dndbeyond.importCharacter (crates/dndbeyond/data-capabilities/import-character, planned): a
    // PC of the campaign linked to that D&D Beyond URL is updated (upsert); a URL that is not
    // D&D Beyond's is refused, and a URL no sheet is linked to creates nothing on the mock-up.
    "dndbeyond.importCharacter": async (server, { payload }) => {
      if (!/^https:\/\/(www\.)?dndbeyond\.com\/characters\/\d+/.test(String(payload.url ?? ""))) return ["not-found"];
      return void 0;
    },
    // table.moveToken (crates/table/data-capabilities/move-token): the token
    // `payload.token` of the instance `targetId` (or `payload.instance`) dropped
    // on the hex (q, r) — its position alone, the one both views read
    // (`single-position-source`); no other token, nor the battlemap and its
    // grid, moves. A hex that is no pair of whole numbers an `integer` holds is
    // refused by `single-position-source`; an unknown token is `not-found`; a
    // token that stands on another instance than the one named — or on none
    // named — is refused by `instance-isolated`, as the DataGuard judges it.
    "table.moveToken": async (server, { payload, targetId }) => {
      const axis = (n) => Number.isInteger(n) && n >= -(2 ** 31) && n < 2 ** 31;
      if (!axis(payload.q) || !axis(payload.r)) return ["single-position-source"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => t.id === payload.token));
      if (!holder) return ["not-found"];
      if (holder.id !== id) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => t.id === payload.token);
      token.q = payload.q;
      token.r = payload.r;
      return void 0;
    },
    // table.setTokenVisibility (crates/table/data-capabilities/set-token-visibility):
    // the token `payload.token` of the instance `targetId` (or
    // `payload.instance`) hidden from the players or revealed — its `visible`
    // alone; its initiative rank and every other stay as they are
    // (`hidden-keeps-rank`). A visibility that is no boolean is refused by
    // `players-see-band-only`; an unknown token is `not-found`; a token that
    // stands on another instance than the one named — or on none named — is
    // refused by `instance-isolated`, as the DataGuard judges it. Ids are
    // compared in any case and without the blanks around them, as the server
    // does.
    "table.setTokenVisibility": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      if (typeof payload.visible !== "boolean") return ["players-see-band-only"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      holder.state.tokens.find((t) => same2(t.id, payload.token)).visible = payload.visible;
      return void 0;
    },
    // table.setTokenCondition (crates/table/data-capabilities/set-token-condition):
    // the badge `payload.state` put on the token `payload.token` of the
    // instance `targetId` (or `payload.instance`) when `payload.on`, after the
    // others and once, or taken off, the others keeping their order — its
    // conditions alone; its HP, its death and the other tokens of the same
    // character stay as they are. A condition that is no name without the
    // blanks around it, or holds U+0000, or an `on` that is no boolean, is
    // refused by `state-persisted-reset`; an unknown token is `not-found`; a
    // token that stands on another instance than the one named — or on none
    // named — is refused by `instance-isolated`, as the DataGuard judges it.
    "table.setTokenCondition": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const state = typeof payload.state === "string" ? payload.state.trim() : "";
      if (!state || state.includes("\0") || typeof payload.on !== "boolean") return ["state-persisted-reset"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => same2(t.id, payload.token));
      const held = token.states || [];
      token.states = payload.on ? held.includes(state) ? held : [...held, state] : held.filter((s) => s !== state);
      return void 0;
    },
    // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
    // the NPC token `payload.token` of the instance `targetId` (or
    // `payload.instance`) wounded or healed by `payload.delta` — its HP alone,
    // landing between 0 and its HP max (`pj-hp-not-tracked`); the other tokens
    // of its character are untouched (`hp-change-one-token`). At 0 it is dead,
    // derived from its HP alone (`dead-derived`). A change that is no whole
    // number, or one on a PC's token, which tracks no HP, is refused by
    // `pj-hp-not-tracked`; an unknown token is `not-found`; a token that stands
    // on another instance than the one named — or on none named — is refused
    // by `instance-isolated`, as the DataGuard judges it. Ids are compared in
    // any case and without the blanks around them, as the server does.
    "table.changeHitPoints": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      if (!Number.isInteger(payload.delta)) return ["pj-hp-not-tracked"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => same2(t.id, payload.token));
      if (!Number.isInteger(token.hp) || !Number.isInteger(token.hpMax)) return ["pj-hp-not-tracked"];
      token.hp = Math.max(0, Math.min(token.hpMax, token.hp + payload.delta));
      token.dead = token.hp === 0;
      return void 0;
    },
    // table.removeToken (crates/table/data-capabilities/remove-token): one
    // token taken off the map, `payload.token` on the instance `targetId` — or
    // the token itself as the target when `payload.token` is absent or null.
    // Its partition is tombstoned and the tables follow the aggregate's
    // relations: its entry leaves the order, whose places close up, and a turn
    // still on it is no one's — the Battlemap sends table.nextTurn first when
    // it holds the turn. An unknown token, or one of another instance than the
    // one named, is `not-found`. Ids are compared in any case and without the
    // blanks around them, as the server does.
    "table.removeToken": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const named = payload.token != null;
      const tokenId = named ? payload.token : targetId;
      if (typeof tokenId !== "string") return ["not-found"];
      const instance = named && targetId && !same2(targetId, tokenId) ? targetId : null;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => (instance == null || same2(i.id, instance)) && i.state.tokens.some((t) => same2(t.id, tokenId)));
      if (!holder) return ["not-found"];
      const id = holder.state.tokens.find((t) => same2(t.id, tokenId)).id;
      holder.state.tokens = holder.state.tokens.filter((t) => t.id !== id);
      const combat = holder.state.combat;
      if (combat) {
        combat.order = (combat.order || []).filter((o) => o.id !== id);
        if (combat.current === id) combat.current = null;
      }
      return void 0;
    },
    // table.nextTurn (crates/table/data-capabilities/next-turn, Initiative@1,
    // « Suivant »): the open combat of the instance `targetId` — or
    // `payload.instance` — passes the turn to the next living participant after
    // the current one in its order, a hidden one like any other
    // (`hidden-keeps-rank`), a dead one or one off the map skipped
    // (`dead-derived`); the round goes up when the search wraps past the last
    // (`next-turn`). With no current turn, the first living one takes it and no
    // round is added. An instance with no open combat is refused by
    // `next-turn`, a combat with no one alive by `dead-derived`. Ids are
    // compared in any case and without the blanks around them, as the server
    // does.
    "table.nextTurn": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload?.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => same2(i.id, id));
      const combat = holder?.state.combat;
      if (!combat) return ["next-turn"];
      const order = combat.order || [];
      const alive = order.map((p) => holder.state.tokens.some((t) => same2(t.id, p.id) && !t.dead));
      const at = order.findIndex((p) => same2(p.id, combat.current));
      const after = alive.findIndex((living, i) => living && i > at);
      const wrapped = at === -1 ? -1 : alive.findIndex((living, i) => living && i <= at);
      if (after === -1 && wrapped === -1) return ["dead-derived"];
      combat.current = order[after === -1 ? wrapped : after].id;
      if (after === -1) combat.round += 1;
      return void 0;
    },
    // table.startCombat (crates/table/data-capabilities/start-combat): the combat
    // of the instance `targetId` (or `payload.instance`), every token in it, a
    // hidden one like any other (`hidden-keeps-rank`). An NPC rolls
    // `payload.rolls[token]` or a d20, plus its DEX modifier (DEX 10 when its
    // sheet gives none); a PC rolls nothing and takes the initiative
    // `payload.initiative[character]` the DM entered (`npc-initiative-bounds`).
    // Ranked by initiative, then DEX score — an unknown one last —, then a random
    // draw (`initiative-tiebreak`); round 1 on rank 1. An unknown instance is
    // `not-found`; one whose combat is open starts none (`one-active-combat`);
    // one with no token, a token that is no PC nor NPC any more, a PC with no
    // initiative entered, or a roll out of 1 to 20 or of no NPC token is
    // refused by `npc-initiative-bounds`, as the server refuses them (422). Ids are
    // compared in any case and without the blanks around them, as the server
    // does.
    "table.startCombat": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      if (instance.state.combat) return ["one-active-combat"];
      const tokens = instance.state.tokens;
      if (!tokens.length) return ["npc-initiative-bounds"];
      const figure = (figures, key) => Object.entries(figures || {}).find(([k]) => same2(k, key))?.[1];
      if (Object.keys(payload.rolls || {}).some((k) => !tokens.some((t) => same2(t.id, k) && t.kind === "pnj"))) return ["npc-initiative-bounds"];
      const characters = await server.load("campaign.listCharacters");
      const seated = [];
      for (const t of tokens) {
        if (t.kind !== "pj" && t.kind !== "pnj") return ["npc-initiative-bounds"];
        const score = characters.find((c) => same2(c.id, t.char))?.stats?.DEX;
        const pc = t.kind === "pj";
        const dexScore = pc ? score ?? null : score ?? 10;
        const roll = pc ? null : figure(payload.rolls, t.id) ?? 1 + Math.floor(Math.random() * 20);
        const init = pc ? figure(payload.initiative, t.char) : roll + Math.floor((dexScore - 10) / 2);
        if (!Number.isInteger(init) || roll !== null && (!Number.isInteger(roll) || roll < 1 || roll > 20)) return ["npc-initiative-bounds"];
        seated.push({ id: t.id, init, roll, dexScore, draw: Math.random() });
      }
      const order = seated.sort((a, b) => b.init - a.init || (b.dexScore ?? -Infinity) - (a.dexScore ?? -Infinity) || b.draw - a.draw).map(({ id: token, init, dexScore }) => ({ id: token, init, dex: dexScore === null ? 0 : Math.floor((dexScore - 10) / 2) }));
      instance.state.combat = { round: 1, current: order[0].id, order };
      return void 0;
    },
    // table.addParticipant (crates/table/data-capabilities/add-participant,
    // Initiative@1): the token `payload.token` of the instance `targetId` (or
    // `payload.instance`) joins its open combat. An NPC rolls a d20 plus its DEX
    // modifier (DEX 10 when its sheet gives none); a PC rolls nothing and takes
    // the initiative `payload.initiative` the DM entered — an NPC is given none
    // (`npc-initiative-bounds`). It goes after the last participant that ranks
    // before it or ties with it, by initiative then DEX, the others keeping their
    // order (`initiative-tiebreak`); the turn and the round stay. A token not on
    // the instance is `not-found`; an instance with no open combat, or a token
    // already in it, is refused by `initiative-tiebreak`, a token that is no PC
    // nor NPC, a PC with no figure or an NPC given one by `npc-initiative-bounds`,
    // as the server refuses them (422). Ids are compared in any case and without
    // the blanks around them, as the server does.
    "table.addParticipant": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      const token = instance?.state.tokens.find((t) => same2(t.id, payload.token));
      if (!token) return ["not-found"];
      const combat = instance.state.combat;
      if (!combat) return ["initiative-tiebreak"];
      const order = combat.order || [];
      if (order.some((o) => same2(o.id, token.id))) return ["initiative-tiebreak"];
      if (token.kind !== "pj" && token.kind !== "pnj") return ["npc-initiative-bounds"];
      const pc = token.kind === "pj";
      if (pc ? !Number.isInteger(payload.initiative) : payload.initiative != null) return ["npc-initiative-bounds"];
      const characters = await server.load("campaign.listCharacters");
      const score = characters.find((c) => same2(c.id, token.char))?.stats?.DEX;
      const dexScore = pc ? score ?? null : score ?? 10;
      const dex = dexScore === null ? 0 : Math.floor((dexScore - 10) / 2);
      const init = pc ? payload.initiative : 1 + Math.floor(Math.random() * 20) + dex;
      const before = order.findLastIndex((o) => o.init > init || o.init === init && (dexScore === null || o.dex >= dex));
      order.splice(before + 1, 0, { id: token.id, init, dex });
      combat.order = order;
      return void 0;
    },
    // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative):
    // the open combat of the instance `targetId` (or `payload.instance`)
    // reordered as `payload.order` gives it, every participant once, named by
    // its token — the ranks 1 to n in that order, a hidden participant like any
    // other (`hidden-keeps-rank`); the turn and the round stay. An unknown
    // instance is `not-found`; one with no open combat, or an order that is no
    // list, omits a participant, repeats one or names one not in the combat, is
    // refused by `initiative-tiebreak`, as the server refuses them (422). Ids
    // are compared in any case and without the blanks around them, as the
    // server does.
    "table.reorderInitiative": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      const combat = instance.state.combat;
      const held = combat && combat.order || [];
      const named = Array.isArray(payload.order) ? payload.order : [];
      if (!combat || !named.length || named.length !== held.length) return ["initiative-tiebreak"];
      const order = [];
      for (const n of named) {
        const at = held.findIndex((o) => same2(o.id, n) && !order.includes(o));
        if (at < 0) return ["initiative-tiebreak"];
        order.push(held[at]);
      }
      combat.order = order;
      return void 0;
    },
    // table.removeParticipant (crates/table/data-capabilities/remove-participant):
    // one participant taken out of the open combat, `payload.participant` — named
    // by its token, as the carousel shows it — on the instance `targetId`, or the
    // participant itself as the target when `payload.participant` is absent or
    // null. Its entry is tombstoned and nothing else is written: its token stays
    // on the map, and the order closes up. The participant holding the turn is
    // refused by `next-turn`: the Battlemap sends table.nextTurn first. A
    // participant in no open combat, or in one of another instance than the one
    // named, is `not-found`. Ids are compared in any case and without the blanks
    // around them, as the server does.
    "table.removeParticipant": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const named = payload?.participant != null;
      const participant = named ? payload.participant : targetId;
      if (typeof participant !== "string") return ["not-found"];
      const instance = named && targetId && !same2(targetId, participant) ? targetId : null;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => (instance == null || same2(i.id, instance)) && (i.state.combat?.order || []).some((o) => same2(o.id, participant)));
      if (!holder) return ["not-found"];
      const combat = holder.state.combat;
      if (same2(combat.current, participant)) return ["next-turn"];
      combat.order = combat.order.filter((o) => !same2(o.id, participant));
      return void 0;
    },
    // table.endCombat (crates/table/data-capabilities/end-combat): the open
    // combat of the instance `targetId` (or `payload.instance`) ends — no turn
    // is current and the carousel goes; its tokens, their HP and their
    // conditions stay as they are, and a new combat may start
    // (`one-active-combat`). An unknown instance, or one with no open combat,
    // is refused by `one-active-combat`, as the server refuses it (422). Ids
    // are compared in any case and without the blanks around them, as the
    // server does.
    "table.endCombat": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload?.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance?.state.combat) return ["one-active-combat"];
      instance.state.combat = null;
      return void 0;
    },
    // table.resetBattlemap (crates/table/data-capabilities/reset-battlemap,
    // « Réinitialiser la carte »): the instance `targetId` — or
    // `payload.instance` — loses every token, its combat and its initiative;
    // the instance itself, its battlemap and its grid stay, and no other
    // instance is touched (`instance-isolated`). When it was reset is the
    // command journal's, which no read shows. An unknown instance is
    // `not-found`; one with nothing on its map is refused by
    // `state-persisted-reset`, as the server refuses it (422). Ids are compared
    // in any case and without the blanks around them, as the server does.
    "table.resetBattlemap": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload?.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      if (!instance.state.tokens.length && !instance.state.combat) return ["state-persisted-reset"];
      instance.state.tokens = [];
      instance.state.combat = null;
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

  // mock/src/reads.js
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e" };
  var READS2 = {
    // table.getBattlemapInstance: the fixture's instance, or one a command made.
    "table.getBattlemapInstance": async (server, { instance }) => {
      const fixture = await server.load("table.getBattlemapInstance");
      if (!instance || instance === fixture.id) return fixture;
      return server.instances && server.instances.get(instance);
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
    // One element's history, newest first: the version list of `entity` in the fixture; an element the fixture
    // does not know but `entities` does has never been saved (an empty history), any other has no answer (null).
    "campaign.listEntityVersions": async (server, { entity }) => {
      const history = (await server.load("campaign.listEntityVersions"))[entity];
      if (history) return history;
      const entities = await server.load("entities");
      const known = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === entity);
      return known ? { name: known.name, versions: [] } : null;
    },
    // campaign.getEntity (crates/campaign/capabilities/get-entity): one element's sheet, as the
    // Capability answers it — the lists `campaign.saveEntity` writes, so a save is read back. A PC or
    // an NPC has its stats with their modifier computed from the score (AbilityModifier@1), its token
    // (`tokenImage`: a dedicated image) and its D&D Beyond link; a place or an event has its kind
    // (`lieu`, `evenement`), aliases, description and notes alone. An unknown element is `null`.
    "campaign.getEntity": async (server, { id }) => {
      const lists = [["campaign.listCharacters", null], ["campaign.listPlaces", "lieu"], ["campaign.listEvents", "evenement"]];
      for (const [name, kind] of lists) {
        const found = (await server.load(name) || []).find((e) => e.id === id);
        if (!found) continue;
        const base = { id: found.id, name: found.name, aliases: found.aliases || [], kind: kind || found.kind, description: found.description || "", notes: found.notes || "" };
        if (kind) return base;
        const modifiers = Object.fromEntries(Object.entries(found.stats).map(([ability, score]) => [ability, Math.floor((score - 10) / 2)]));
        const initials = found.initials || found.name.split(/\s+/).map((w) => w.charAt(0)).join("").slice(0, 2).toUpperCase();
        return { ...base, player: found.kind === "pj" ? found.player || null : null, prompt: found.prompt || "", background: found.background || "", behavior: found.behavior || "", stats: found.stats, modifiers, ca: found.ca, pvMax: found.pvMax, features: found.features || [], color: found.color || "accent", initials, portrait: Boolean(found.portrait), tokenImage: Boolean(found.token || found.tokenImage), crop: found.crop || null, ddb: found.ddb && found.ddb.url ? found.ddb : null };
      }
      return null;
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
    s.tell = (message2, to, commandId2, extra = {}) => {
      const m = { message: message2, to: Array.isArray(to) ? to : [to], commandId: commandId2, at: (/* @__PURE__ */ new Date()).toISOString(), ...extra };
      s.messages.push(m);
      return m;
    };
    const read = async (identifier, variables) => {
      const handler = READS2[identifier];
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

  // mock/src/MediaLibrary.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var onPage = { ...shell, context: { set: (name, value) => setContext(name, value) } };
  var campaign = new URLSearchParams(location.search).get("id") || "cendres-valombre";
  compose(props, (resolved) => mount(host, { campaign, ...resolved }, onPage));
})();
