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
    return { session: text(raw.session), sessionNumber: Number.isInteger(number) && number > 0 ? number : null };
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
    const payload = { kind: upload.kind, title: upload.title, filePath: upload.filePath };
    if (upload.size !== null) Object.assign(payload, { widthPx: upload.size.width, heightPx: upload.size.height });
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
    const tail = m.kind === "battlemap" ? m.grid === null ? "" : ` \xB7 grille ${m.grid.orient === "pointy" ? "pointe" : "c\xF4t\xE9 plat"} ${m.grid.size} px` : m.ratio === null ? "" : ` \xB7 ratio ${ratio(m.ratio)}`;
    return `${m.file} \xB7 ${m.size}${tail}`;
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
  function card(m, places, sessionPlaces, sessionNumber, selected) {
    return `<li class="mf-media__item${selected ? " is-selected" : ""}" id="media-${esc(m.id)}" data-id="${esc(m.id)}"><div class="mf-media__thumb"><div class="art art--${esc(m.art)}" role="img" aria-label="${esc(m.title)}"><span class="art__glyph">${esc(m.glyph || m.title.charAt(0).toUpperCase())}</span></div></div><b>${esc(m.title)}</b><small>${esc(caption(m))}</small>` + select({ id: `media-place-${m.id}`, label: "Lieu", value: m.place ?? "", options: places, none: "Aucun lieu", writes: SET_MEDIA_PLACE }) + placeLink(m, places, sessionPlaces, sessionNumber) + (m.kind === "battlemap" ? button({ label: "R\xE9gler la grille", variant: selected ? "primary" : "secondary", small: true, action: `grid:${m.id}` }) : "") + "</li>";
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
.mf-media .kit-filedrop.has-file .kit-filedrop__file { display: flex; }
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
      const [library, campaignPlaces, scenes] = await Promise.allSettled([
        shell2.capability(LIST_MEDIA, void 0),
        shell2.capability(LIST_PLACES, void 0),
        withSession ? shell2.capability(LIST_SCENES, void 0, { session: props2.session }) : Promise.resolve(null)
      ]);
      if (mine !== ticket) return "";
      if (library.status === "rejected") throw new Error("the library cannot be read");
      asOf = Math.max(0, ...[library, campaignPlaces, scenes].flatMap((r) => r.status === "fulfilled" && r.value !== null ? [r.value.asOf] : []));
      media = parseMedia(library.value.data);
      places = campaignPlaces.status === "fulfilled" ? parsePlaces(campaignPlaces.value.data) : [];
      sessionPlaces = withSession && scenes.status === "fulfilled" && scenes.value !== null ? parseSessionPlaces(scenes.value.data) : null;
      if (selected === null || !media.some((m) => m.id === selected)) {
        selected = byKind(media, "battlemap")[0]?.id ?? null;
        if (selected !== null) shell2.context?.set(BATTLEMAP_CONTEXT, selected);
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
      if (busy) return false;
      busy = true;
      lock(true);
      let applied = false;
      try {
        const state = await send(await body());
        if (state.status === "rejected") say("error", `${wrong} Refus\xE9 : ${reasons(state.violations)}.`);
        else if (!isFinal(state.status) && state.status !== "") {
          say("busy", `La commande est en file d\u2019attente (\xE9tat : ${state.status}) : elle s\u2019appliquera d\xE8s que possible.`);
          await read().catch(() => "");
        } else if (state.status !== "applied") say("error", `${wrong} (\xE9tat : ${state.status || "inconnu"}).`);
        else {
          applied = true;
          let unread = "";
          try {
            unread = await read();
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
        if (dirty) {
          dirty = false;
          refresh();
        }
      }
      return applied;
    }
    const selectBattlemap = (id) => {
      selected = id;
      paintCards();
      shell2.context?.set(BATTLEMAP_CONTEXT, id);
    };
    const changePlace = (mediumId, value) => {
      const m = media.find((x) => x.id === mediumId);
      const place = value === "" ? null : value;
      if (m === void 0 || m.place === place) return Promise.resolve(false);
      const name = places.find((p) => p.id === place)?.name ?? place;
      return act("Le lieu n\u2019a pas pu \xEAtre chang\xE9.", () => Promise.resolve(setPlaceCommand(m.id, place, io.key())), () => place === null ? `\xAB ${m.title} \xBB n\u2019a plus de lieu.` : `\xAB ${m.title} \xBB est rattach\xE9 au lieu \xAB ${name ?? ""} \xBB.`);
    };
    const addPlace = (place) => {
      const name = places.find((p) => p.id === place)?.name ?? place;
      return act("Le lieu n\u2019a pas pu \xEAtre ajout\xE9 \xE0 la s\xE9ance.", () => Promise.resolve(addPlaceCommand(props2.session, place, io.key())), () => `\xAB ${name} \xBB est dans la s\xE9ance : ses m\xE9dias y sont associ\xE9s.`);
    };
    async function upload() {
      if (busy) return;
      if (file === null) return say("error", "Choisissez d\u2019abord une image \xE0 t\xE9l\xE9verser.");
      const chosen = file;
      const title = titleFor(draft, chosen);
      const before = new Set(media.map((m) => m.id));
      const { kind } = draft;
      const place = draft.place === "" ? null : draft.place;
      const sent = async () => uploadCommand({ kind, title, filePath: chosen.name, place, size: await io.measure(chosen) }, io.key());
      const applied = await act("Le t\xE9l\xE9versement a \xE9chou\xE9.", sent, () => `\xAB ${title} \xBB est ajout\xE9 \xE0 la campagne.`);
      if (!applied) return;
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
    const stop = shell2.onDataVersion?.((version) => {
      if (!host2.isConnected) stop?.();
      else if (!busy && version > asOf) refresh();
    });
    paintAll();
    say("busy", "Chargement des m\xE9dias\u2026");
    refresh();
    return {
      update(next) {
        const before = props2;
        props2 = parseProps(next);
        if (props2.session === before.session && props2.sessionNumber === before.sessionNumber) return;
        if (busy) dirty = true;
        else refresh();
      },
      destroy() {
        stop?.();
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
  var COMMANDS = {
    // session.createSession (planned, #157): the next session of the campaign, at
    // the position after the last, in preparation, with the PCs ticked present.
    // `present-only-pj` refuses a character that is not a PC of the fixtures.
    "session.createSession": async (server, { payload }) => {
      const sessions = await server.load("session.listSessions");
      const characters = await server.load("campaign.listCharacters");
      const present = Array.isArray(payload.present) ? payload.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
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
    // media.setMediaPlace (milestone #19): a medium's place set, or cleared with `place: null` — a medium
    // with no place stays valid. The place must be a lieu of the campaign (`media-place-is-lieu`).
    "media.setMediaPlace": async (server, { payload }) => {
      const library = await server.load("media.listMedia");
      const medium2 = library.battlemaps.concat(library.illustrations).find((m) => m.id === payload.media);
      if (!medium2) return ["not-found"];
      const place = payload.place || null;
      if (place !== null && !(await server.load("campaign.listPlaces")).some((p) => p.id === place)) return ["media-place-is-lieu"];
      medium2.place = place;
      return void 0;
    },
    // media.uploadMedia (milestone #19): a medium recorded, its place optional; a battlemap arrives with its
    // default grid (`grid-only-for-battlemap`, `hex-is-five-feet`).
    "media.uploadMedia": async (server, { payload }) => {
      const library = await server.load("media.listMedia");
      const place = payload.place || null;
      if (place !== null && !(await server.load("campaign.listPlaces")).some((p) => p.id === place)) return ["media-place-is-lieu"];
      const base = { id: `media-${server.uuid()}`, title: payload.title, file: payload.filePath, size: "2,2 Mo", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      if (payload.kind === "battlemap") {
        library.battlemaps.push({ ...base, art: "night", glyph: "\u2B21", width: payload.widthPx || 900, height: payload.heightPx || 600, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 }, terrain: [] });
      } else library.illustrations.push({ ...base, art: "dusk", glyph: String(payload.title).charAt(0).toUpperCase(), ratio: 1.5 });
      return void 0;
    },
    // session.addPlaceToSession (task #162): the place joins a scene — the one the payload names, else the
    // first — once, and its media follow: session-media-by-place, derived, never stored.
    "session.addPlaceToSession": async (server, { payload }) => {
      if (!(await server.load("campaign.listPlaces")).some((p) => p.id === payload.place)) return ["scene-links-same-campaign"];
      const scenes = await server.load("session.listScenes");
      const scene = scenes.find((sc) => sc.id === payload.scene) || scenes[0];
      if (!scene) return ["scene-links-same-campaign"];
      if (!scene.places.includes(payload.place)) scene.places.push(payload.place);
      const library = await server.load("media.listMedia");
      for (const m of library.battlemaps.concat(library.illustrations)) if (m.place === payload.place && !scene.media.includes(m.id)) scene.media.push(m.id);
      return void 0;
    }
  };

  // mock/src/reads.js
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e", played: "Jou\xE9e" };
  var READS2 = {
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

  // mock/src/MediaLibrary.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var onPage = {
    ...shell,
    context: {
      set(name, value) {
        setContext(name, value);
        if (name !== "battlemap") return;
        void window.__mockServer.load("media.listMedia").then((library) => {
          const map = library.battlemaps.find((m) => m.id === value);
          if (map) document.dispatchEvent(new CustomEvent("fdm:grid-edit", { detail: { id: value, map } }));
        });
      }
    }
  };
  compose(props, (resolved) => mount(host, resolved, onPage));
})();
