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
  function createShell(fetcher = (input2, init) => fetch(input2, init), page = browserPage()) {
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

  // apps/soundbox/sound-library/src/ids.ts
  var LIST_SOUNDS = "soundbox.listSounds";
  var GET_LIVE_SESSION = "session.getLiveSession";
  var ADD_SOUND = "soundbox.addSound";
  var UPDATE_SOUND = "soundbox.updateSound";
  var REMOVE_SOUND = "soundbox.removeSound";
  var DATA_CAPABILITY_VERSION = 1;
  var CAMPAIGN_CONTEXT = "campaign";

  // apps/soundbox/sound-library/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var num = (value) => typeof value === "number" && Number.isFinite(value) ? value : null;
  var DEFAULT_VOLUME = 75;
  function parseSounds(data) {
    const list = isRecord(data) && Array.isArray(data.sounds) ? data.sounds : [];
    const sounds = [];
    for (const entry of list) {
      if (!isRecord(entry) || text(entry.id) === "") continue;
      sounds.push({
        id: text(entry.id),
        name: text(entry.name),
        tags: texts(entry.tags),
        volume: num(entry.volume) ?? 0,
        file: text(entry.file),
        duration: num(entry.duration),
        playing: entry.playing === true
      });
    }
    return sounds;
  }
  function campaignOf(data) {
    return isRecord(data) && isRecord(data.session) ? text(data.session.campaign) : "";
  }
  var norm = (value) => value.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase().trim();
  function filterSounds(sounds, search, tag) {
    const wanted = norm(search);
    const chosen = tag === null ? null : norm(tag);
    return sounds.filter(
      (sound) => (wanted === "" || norm(`${sound.name} ${sound.tags.join(" ")}`).includes(wanted)) && (chosen === null || sound.tags.some((t) => norm(t) === chosen))
    );
  }
  function tagsOf(sounds) {
    const seen = /* @__PURE__ */ new Map();
    for (const sound of sounds) for (const tag of sound.tags) if (!seen.has(norm(tag))) seen.set(norm(tag), tag);
    return [...seen.values()];
  }
  var splitTags = (value) => value.split(",").map((tag) => tag.trim().replace(/^#/, "")).filter((tag) => tag !== "");
  function nameFromFile(fileName) {
    const name = fileName.replace(/\.[^.]+$/, "").replace(/[-_]+/g, " ").trim();
    return name === "" ? "" : name.charAt(0).toUpperCase() + name.slice(1);
  }
  var FILE_TYPES = ["ogg", "mp3", "wav"];
  var isSoundFile = (fileName) => FILE_TYPES.some((type) => fileName.toLowerCase().endsWith(`.${type}`));
  var duration = (seconds) => seconds === null ? "\u2014" : `${seconds.toFixed(1).replace(".", ",")} s`;
  function summary(sounds) {
    const count = sounds.length;
    const tags = tagsOf(sounds).length;
    return `${count} ${count > 1 ? "sons" : "son"} \xB7 ${tags} ${tags > 1 ? "tags" : "tag"}`;
  }
  function addProblem(form) {
    if (form.file === "") return "D\xE9posez d\u2019abord le fichier du son.";
    if (form.name.trim() === "") return "Le nom du son est obligatoire.";
    return "";
  }
  function editProblem(form) {
    if (form.name.trim() === "") return "Le nom du son est obligatoire.";
    if (!(form.volume >= 0 && form.volume <= 100)) return "Le volume reste entre 0 et 100 %.";
    return "";
  }
  function addPayload(form, campaign) {
    return { ...campaign === "" ? {} : { campaign }, name: form.name.trim(), tags: splitTags(form.tags), volume: DEFAULT_VOLUME, file: form.file };
  }
  function updatePayload(form) {
    return { name: form.name.trim(), tags: splitTags(form.tags), volume: form.volume };
  }
  function commandBody(dataCapability, payload, targetId) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload,
      ...targetId === void 0 ? {} : { targetId },
      caller: { by: "dm", from: "SoundLibrary", roles: [] }
    };
  }
  var VIOLATIONS = {
    "sound-volume-range": "le volume reste entre 0 et 100 %",
    "tags-normalized": "les tags sont en minuscules, sans doublon",
    "not-found": "ce son n\u2019existe plus"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result2 = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    return { status: text(result2.status), violations: texts(result2.violations) };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/soundbox/sound-library/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var attr = (name, value) => value === "" ? "" : ` ${name}="${esc(value)}"`;
  function input(p) {
    return `<div class="kit-input is-wide"><label>${esc(p.label)}<input type="${p.type ?? "text"}" id="${p.id}"${attr("value", p.value ?? "")}${attr("placeholder", p.placeholder ?? "")}>` + (p.hint ? `<small>${esc(p.hint)}</small>` : "") + "</label></div>";
  }
  function buttons(items) {
    return '<div class="kit-button kit-button--bare">' + items.map((b) => `<button type="button" class="${b.variant}" id="${b.id}"${attr("data-writes", b.writes ?? "")}>${esc(b.label)}</button>`).join("") + "</div>";
  }
  function slider(p) {
    const fill = `${p.value}%`;
    return `<div class="kit-slider kit-slider--bare"><div class="kit-slider__field" data-name="volume"><div class="kit-slider__head"><label class="kit-slider__label" for="${p.id}">${esc(p.label)}</label><span class="kit-slider__value"><output class="kit-slider__out" for="${p.id}">${p.value}</output><span class="kit-slider__unit">%</span></span></div><input class="kit-slider__input" id="${p.id}" type="range" name="volume" min="0" max="100" value="${p.value}" style="--fill: ${fill}"><div class="kit-slider__bounds"><span>0 %</span><span>100 %</span></div></div></div>`;
  }
  var UPLOAD_ICON = '<svg class="kit-filedrop__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>';
  function fileDrop() {
    const types = FILE_TYPES.map((t) => t.toUpperCase());
    return '<div class="kit-filedrop is-wide" id="sounds-drop-field"><label class="kit-filedrop__zone" tabindex="0" id="sounds-drop">' + UPLOAD_ICON + `<span class="kit-filedrop__title">D\xE9posez un son ou <b>parcourez</b></span><span class="kit-filedrop__hint">Effet ponctuel, 30 s maximum</span><span class="kit-filedrop__types">${types.map((t) => `<span class="kit-filedrop__tag">${t}</span>`).join("")}</span><input class="kit-filedrop__input" type="file" id="sounds-file" accept="${FILE_TYPES.map((t) => `.${t}`).join(",")}"></label><div class="kit-filedrop__error">Type de fichier non accept\xE9.</div><div class="kit-filedrop__file"><div class="kit-filedrop__thumb"></div><div class="kit-filedrop__info"><div class="kit-filedrop__name"></div><div class="kit-filedrop__row"><span class="kit-filedrop__size"></span><span class="kit-filedrop__pct">Pr\xEAt</span></div><div class="kit-filedrop__bar"><div class="kit-filedrop__fill"></div></div></div></div></div>`;
  }
  function skeleton() {
    const reads = `data-reads="${LIST_SOUNDS}"`;
    return `<div class="mf-sounds layout-2" id="sound-library"><section class="panel" id="sounds-list-panel" aria-labelledby="sounds-title"><div class="panel__head"><h2 id="sounds-title">Sons de la campagne</h2><span class="small muted" id="sounds-campaign" data-reads="${GET_LIVE_SESSION}"></span><span class="small muted" id="sounds-count" ${reads}></span></div><div id="sounds-search">${input({ id: "sounds-search-input", label: "Rechercher", type: "search", placeholder: "Nom ou tag : porte, combat, ext\xE9rieur\u2026" })}</div><div class="row" id="sounds-filter" aria-live="polite"></div><div id="sounds-table" ${reads} aria-live="polite"></div><div id="sounds-status" aria-live="polite"></div></section><aside class="stack" id="sounds-side"><form class="panel" id="sounds-upload" aria-labelledby="sounds-upload-title"><h2 id="sounds-upload-title">T\xE9l\xE9verser un son</h2><p class="small muted">Effets ponctuels uniquement. Le nom sert aussi \xE0 la d\xE9tection au micro : dites \xAB la porte grince \xBB et le son vous est propos\xE9.</p><div id="sounds-upload-fields">` + fileDrop() + input({ id: "sounds-new-name", label: "Nom", placeholder: "Porte qui grince" }) + input({ id: "sounds-new-tags", label: "Tags", placeholder: "porte, bois, int\xE9rieur", hint: "S\xE9par\xE9s par des virgules" }) + buttons([{ id: "sounds-new-submit", label: "Ajouter \xE0 la soundbox", variant: "primary", writes: ADD_SOUND }]) + '</div><div id="sounds-upload-result" aria-live="polite"></div></form><form class="panel" id="sounds-edit" aria-labelledby="sounds-edit-title" hidden><h2 id="sounds-edit-title">Modifier le son</h2><div id="sounds-edit-fields"></div><div id="sounds-edit-result" aria-live="polite"></div></form></aside></div>';
  }
  var tagButton = (tag, chosen) => `<button type="button" data-tag="${esc(tag)}" aria-pressed="${chosen !== null && norm(chosen) === norm(tag)}">#${esc(tag)}</button>`;
  function table(sounds, chosen) {
    const rows = sounds.map(
      (s) => `<tr id="sound-${esc(s.id)}"${s.playing ? ' class="is-playing"' : ""}><td>${esc(s.name)}</td><td><span class="mf-sounds__tags">${s.tags.map((t) => tagButton(t, chosen)).join("")}</span></td><td>${esc(s.file || "\u2014")}</td><td class="num">${duration(s.duration)}</td><td class="num">${s.volume} %</td><td><span class="row"><button type="button" class="chip" data-edit="${esc(s.id)}">Modifier</button></span></td></tr>`
    ).join("");
    return `<table class="kit-table" id="sounds-table-grid"><thead><tr><th>Nom</th><th>Tags</th><th>Fichier</th><th class="num">Dur\xE9e</th><th class="num">Volume</th><th></th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  var empty = (library) => `<p class="muted" id="sounds-empty">${library ? "Aucun son dans la biblioth\xE8que : d\xE9posez le premier." : "Aucun son ne correspond \xE0 la recherche."}</p>`;
  var unreadable = () => '<p class="is-error" id="sounds-error">Impossible de lire les sons de la campagne.</p>';
  function filter(chosen) {
    if (chosen === null) return "";
    return `<div class="kit-badge"><span class="primary" id="sounds-filter-badge">Tag : #${esc(chosen)}</span></div><button type="button" class="chip" id="sounds-filter-clear">Retirer le filtre</button>`;
  }
  function editFields(form) {
    return input({ id: "sounds-edit-name", label: "Nom", value: form.name }) + input({ id: "sounds-edit-tags", label: "Tags", value: form.tags, hint: "S\xE9par\xE9s par des virgules" }) + slider({ id: "sounds-edit-volume", label: "Volume propre", value: form.volume }) + buttons([
      { id: "sounds-edit-save", label: "Enregistrer", variant: "primary", writes: UPDATE_SOUND },
      { id: "sounds-edit-delete", label: "Supprimer", variant: "danger", writes: REMOVE_SOUND },
      { id: "sounds-edit-cancel", label: "Fermer", variant: "ghost" }
    ]);
  }
  function result(outcome) {
    if (outcome.kind === "none") return "";
    if (outcome.kind === "busy") return `<p class="muted">${esc(outcome.text)}</p>`;
    return `<p class="${outcome.kind === "ok" ? "is-ok" : "is-error"}">${esc(outcome.text)}</p>`;
  }

  // apps/soundbox/sound-library/src/style.ts
  var STYLE = `
.mf-sounds { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-sounds *, .mf-sounds *::before, .mf-sounds *::after { box-sizing: inherit; }
.mf-sounds.layout-2 { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
@media (max-width: 960px) { .mf-sounds.layout-2 { grid-template-columns: minmax(0, 1fr); } }
.mf-sounds h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sounds p { margin: 0; }
.mf-sounds .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-sounds .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-sounds .muted { color: var(--color-text-muted); }
.mf-sounds .small { font-size: var(--font-size-small); }
.mf-sounds .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-sounds .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-sounds .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-sounds .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-sounds .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-sounds .kit-slider.kit-slider--bare { padding: 0; background: transparent; border: 0; box-shadow: none; max-width: none; }
.mf-sounds .kit-input, .mf-sounds .kit-filedrop { max-width: none; }
.mf-sounds #sounds-upload-fields, .mf-sounds #sounds-edit-fields { display: grid; gap: var(--space-3); }
.mf-sounds__tags { display: flex; flex-wrap: wrap; gap: var(--space-1); }
.mf-sounds__tags button { padding: 0 var(--space-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: transparent; font: inherit; font-size: var(--font-size-small); color: var(--color-text-muted); cursor: pointer; }
.mf-sounds__tags button:hover, .mf-sounds__tags button:focus-visible { border-color: var(--color-primary); color: var(--color-accent); }
.mf-sounds__tags button[aria-pressed="true"] { border-color: var(--color-primary); color: var(--color-primary); }
.mf-sounds tr.is-playing td:first-child::before { content: "\u25B6 "; color: var(--color-success); }
.mf-sounds .is-ok, .mf-sounds .is-error { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-sounds .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-sounds .is-error { border-color: var(--color-danger); color: var(--color-danger); }
`;

  // apps/soundbox/sound-library/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="SoundLibrary"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "SoundLibrary";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var size = (bytes) => bytes < 1024 ? `${bytes} o` : bytes < 1048576 ? `${(bytes / 1024).toFixed(1).replace(".", ",")} Ko` : `${(bytes / 1048576).toFixed(1).replace(".", ",")} Mo`;
  function mount(host2, props2, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    let campaign = typeof props2.campaign === "string" ? props2.campaign : "";
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`SoundLibrary: #${id} is not in the card`);
      return found;
    };
    const field = (id) => part(id).value;
    let sounds = [];
    let loaded = false;
    let unread = false;
    let search = "";
    let tag = null;
    let add = { file: "", name: "", tags: "" };
    let suggested = "";
    let editing = null;
    const say = (id, outcome) => {
      part(id).innerHTML = result(outcome);
    };
    const paintList = () => {
      part("sounds-count").textContent = loaded ? summary(sounds) : "";
      part("sounds-filter").innerHTML = filter(tag);
      const shown2 = filterSounds(sounds, search, tag);
      part("sounds-table").innerHTML = unread ? unreadable() : !loaded ? '<p class="muted">Chargement des sons\u2026</p>' : shown2.length === 0 ? empty(sounds.length === 0) : table(shown2, tag);
    };
    const lock = (locked) => {
      for (const control of host2.querySelectorAll("#sounds-upload input, #sounds-upload button, #sounds-edit input, #sounds-edit button")) {
        control.disabled = locked;
      }
    };
    const locate = async () => {
      if (campaign !== "") return;
      try {
        const named = shell2.context.get(CAMPAIGN_CONTEXT);
        if (typeof named === "string" && named !== "") {
          campaign = named;
          return;
        }
        campaign = campaignOf((await shell2.capability(GET_LIVE_SESSION, void 0)).data);
        if (campaign === "") return;
        part("sounds-campaign").textContent = `Campagne : ${campaign}`;
        shell2.context.set(CAMPAIGN_CONTEXT, campaign);
      } catch {
      }
    };
    let located = null;
    const campaignKnown = () => {
      located ??= locate().then(() => {
        if (campaign === "") located = null;
      });
      return located;
    };
    let latest = 0;
    const refresh = async () => {
      await campaignKnown();
      const mine = latest += 1;
      let fresh;
      try {
        fresh = parseSounds((await shell2.capability(LIST_SOUNDS, void 0, campaign === "" ? {} : { campaign })).data);
      } catch {
        if (mine === latest) unread = !loaded;
        return false;
      }
      if (mine !== latest) return true;
      sounds = fresh;
      loaded = true;
      unread = false;
      if (editing !== null && !sounds.some((s) => s.id === editing?.id)) {
        editing = null;
        part("sounds-edit").hidden = true;
      }
      paintList();
      return true;
    };
    async function send(dataCapability, payload, targetId) {
      const answer = await shell2.command(commandBody(dataCapability, payload, targetId));
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    let busy = false;
    async function act(area, wrong, run) {
      if (busy) return;
      busy = true;
      lock(true);
      try {
        say(area, await run());
      } catch {
        say(area, { kind: "error", text: wrong });
        await refresh();
      } finally {
        busy = false;
        lock(false);
      }
    }
    const failure = (refused2, unfinished, state) => {
      if (state.status === "applied") return null;
      if (state.status === "rejected") return { kind: "error", text: `${refused2} : ${reasons(state.violations)}.` };
      return { kind: "error", text: `${unfinished} n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).` };
    };
    const clearDrop = () => {
      part("sounds-drop-field").classList.remove("has-file", "has-error", "is-done");
      part("sounds-file").value = "";
      add = { file: "", name: "", tags: "" };
      suggested = "";
    };
    const takeFile = (file) => {
      const drop = part("sounds-drop-field");
      if (!isSoundFile(file.name)) {
        drop.classList.remove("has-file", "is-done");
        drop.classList.add("has-error");
        add = { ...add, file: "" };
        return;
      }
      drop.classList.remove("has-error");
      drop.classList.add("has-file", "is-done");
      const named = (cls) => {
        const found = drop.querySelector(`.${cls}`);
        if (!found) throw new Error(`SoundLibrary: .${cls} is not in the drop zone`);
        return found;
      };
      named("kit-filedrop__name").textContent = file.name;
      named("kit-filedrop__size").textContent = size(file.size);
      named("kit-filedrop__thumb").textContent = (file.name.split(".").pop() ?? "").toUpperCase();
      named("kit-filedrop__fill").style.width = "100%";
      add = { ...add, file: file.name };
      const name = part("sounds-new-name");
      if (name.value === "" || name.value === suggested) {
        suggested = nameFromFile(file.name);
        name.value = suggested;
      }
    };
    const addSound = () => {
      add = { file: add.file, name: field("sounds-new-name"), tags: field("sounds-new-tags") };
      const why = addProblem(add);
      if (why) {
        say("sounds-upload-result", { kind: "error", text: why });
        return Promise.resolve();
      }
      const form = add;
      return act("sounds-upload-result", "L\u2019ajout a \xE9chou\xE9.", async () => {
        say("sounds-upload-result", { kind: "busy", text: "Ajout en cours\u2026" });
        await campaignKnown();
        const state = await send(ADD_SOUND, addPayload(form, campaign));
        const wrong = failure("Ajout refus\xE9", "L\u2019ajout", state);
        if (wrong) return wrong;
        clearDrop();
        part("sounds-new-name").value = "";
        part("sounds-new-tags").value = "";
        const read = await refresh();
        return { kind: "ok", text: read ? `\xAB ${form.name.trim()} \xBB est dans la soundbox.` : "Le son est ajout\xE9, mais la liste n\u2019a pas pu \xEAtre relue." };
      });
    };
    const openEdit = (id) => {
      const sound = sounds.find((s) => s.id === id);
      if (!sound) return;
      editing = { id: sound.id, name: sound.name, tags: sound.tags.join(", "), volume: sound.volume };
      part("sounds-edit").hidden = false;
      part("sounds-edit-fields").innerHTML = editFields(editing);
      say("sounds-edit-result", { kind: "none" });
      if (busy) lock(true);
      const form = part("sounds-edit");
      if (typeof form.scrollIntoView === "function") form.scrollIntoView({ behavior: "smooth", block: "nearest" });
    };
    const readEdit = (current) => ({
      id: current.id,
      name: field("sounds-edit-name"),
      tags: field("sounds-edit-tags"),
      volume: Number(field("sounds-edit-volume"))
    });
    const saveSound = () => {
      if (editing === null) return Promise.resolve();
      const form = readEdit(editing);
      const why = editProblem(form);
      if (why) {
        say("sounds-edit-result", { kind: "error", text: why });
        return Promise.resolve();
      }
      return act("sounds-edit-result", "L\u2019enregistrement a \xE9chou\xE9.", async () => {
        say("sounds-edit-result", { kind: "busy", text: "Enregistrement\u2026" });
        const state = await send(UPDATE_SOUND, updatePayload(form), form.id);
        const wrong = failure("Enregistrement refus\xE9", "L\u2019enregistrement", state);
        if (wrong) return wrong;
        editing = form;
        const read = await refresh();
        return { kind: "ok", text: read ? "Son enregistr\xE9." : "Son enregistr\xE9, mais la liste n\u2019a pas pu \xEAtre relue." };
      });
    };
    const removeSound = () => {
      if (editing === null) return Promise.resolve();
      const form = editing;
      const name = sounds.find((s) => s.id === form.id)?.name ?? form.name;
      return act("sounds-status", "La suppression a \xE9chou\xE9.", async () => {
        say("sounds-status", { kind: "busy", text: "Suppression\u2026" });
        const state = await send(REMOVE_SOUND, {}, form.id);
        const wrong = failure("Suppression refus\xE9e", "La suppression", state);
        if (wrong) return wrong;
        if (editing?.id === form.id) {
          editing = null;
          part("sounds-edit").hidden = true;
        }
        const read = await refresh();
        return { kind: "ok", text: read ? `\xAB ${name} \xBB est supprim\xE9.` : `\xAB ${name} \xBB est supprim\xE9, mais la liste n\u2019a pas pu \xEAtre relue.` };
      });
    };
    host2.addEventListener("submit", (event) => event.preventDefault());
    host2.addEventListener("input", (event) => {
      const target = event.target;
      if (target?.id === "sounds-search-input") {
        search = target.value ?? "";
        paintList();
      } else if (target?.id === "sounds-edit-volume") {
        const slider2 = event.target;
        const out = slider2.closest(".kit-slider__field")?.querySelector(".kit-slider__out");
        if (out) out.textContent = slider2.value;
        slider2.style.setProperty("--fill", `${slider2.value}%`);
      }
    });
    host2.addEventListener("change", (event) => {
      const target = event.target;
      if (target?.id !== "sounds-file") return;
      const file = target.files?.[0];
      if (file) takeFile(file);
    });
    host2.addEventListener("dragover", (event) => {
      if (event.target?.closest("#sounds-drop")) {
        event.preventDefault();
        part("sounds-drop").classList.add("is-over");
      }
    });
    host2.addEventListener("dragleave", (event) => {
      if (event.target?.closest("#sounds-drop")) part("sounds-drop").classList.remove("is-over");
    });
    host2.addEventListener("drop", (event) => {
      if (!event.target?.closest("#sounds-drop")) return;
      event.preventDefault();
      part("sounds-drop").classList.remove("is-over");
      const file = event.dataTransfer?.files[0];
      if (file) takeFile(file);
    });
    host2.addEventListener("keydown", (event) => {
      if (event.target?.id === "sounds-drop" && (event.key === "Enter" || event.key === " ")) {
        event.preventDefault();
        part("sounds-file").click();
      }
    });
    host2.addEventListener("click", (event) => {
      const clicked = event.target;
      const button = clicked?.closest("button");
      if (!button) return;
      if (button.dataset.tag !== void 0) {
        tag = tag !== null && norm(tag) === norm(button.dataset.tag) ? null : button.dataset.tag;
        paintList();
      } else if (button.dataset.edit !== void 0) openEdit(button.dataset.edit);
      else if (button.id === "sounds-filter-clear") {
        tag = null;
        paintList();
      } else if (button.id === "sounds-new-submit") void addSound();
      else if (button.id === "sounds-edit-save") void saveSound();
      else if (button.id === "sounds-edit-delete") void removeSound();
      else if (button.id === "sounds-edit-cancel") {
        editing = null;
        part("sounds-edit").hidden = true;
      }
    });
    paintList();
    void refresh().then((read) => {
      if (!read) paintList();
    });
    let first = true;
    shell2.onDataVersion?.(() => {
      if (first) first = false;
      else void refresh();
    });
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

  // mock/src/SoundLibrary.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input2, init) => server.fetch(new URL(input2, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
