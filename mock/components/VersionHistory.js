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
    constructor(status, body2) {
      super(`the server answered ${status}`);
      this.status = status;
      this.body = body2;
    }
  };
  async function call(fetcher, method, path, body2) {
    const init = { method };
    if (body2 !== void 0) {
      init.headers = { "content-type": "application/json" };
      init.body = JSON.stringify(body2);
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
  var asStrings = (value) => Array.isArray(value) && value.every((item2) => typeof item2 === "string") ? value : void 0;
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
      if (meta !== void 0) return asCaller(JSON.parse(meta)) ?? FAIL_CLOSED;
      return await mockupCaller(page);
    } catch {
      return FAIL_CLOSED;
    }
  }
  function withCaller(body2, caller) {
    if (!body2 || typeof body2 !== "object" || Array.isArray(body2)) return body2;
    const sent = body2.caller;
    return { ...body2, caller: { ...sent && typeof sent === "object" ? sent : {}, by: caller.by, roles: caller.roles, permissions: caller.permissions } };
  }
  function createShell(fetcher = (input, init) => fetch(input, init), page = browserPage()) {
    let caller;
    const context2 = new Context();
    const named = page.href ? new URL(page.href, "http://shell").searchParams.get("campaign") : null;
    if (named) context2.hold("campaign", named);
    else {
      const fallback = page.meta("shell-default-campaign");
      if (fallback) context2.set("campaign", fallback);
    }
    return {
      capability: async (id, sha256, variables = {}) => await call(fetcher, "POST", `/capabilities/${encodeURIComponent(id)}`, { sha256, variables }),
      command: async (body2) => call(fetcher, "POST", "/commands", withCaller(body2, await (caller ??= pageCaller(page)))),
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

  // apps/campaign/version-history/src/ids.ts
  var LIST_ENTITY_VERSIONS = "campaign.listEntityVersions";
  var RESTORE_ENTITY_VERSION = "campaign.restoreEntityVersion";
  var DATA_CAPABILITY_VERSION = 1;
  var ENTITY_AGGREGATE = "Entity";

  // apps/campaign/version-history/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var shown = (value) => {
    if (value === null || value === void 0) return "";
    return typeof value === "string" ? value : JSON.stringify(value);
  };
  function change(value) {
    return isRecord(value) && typeof value.field === "string" ? [{ field: value.field, old: shown(value.old), new: shown(value.new) }] : [];
  }
  function version(value) {
    if (!isRecord(value) || typeof value.v !== "number") return [];
    return [
      {
        v: value.v,
        date: text(value.date),
        origin: text(value.origin),
        source: text(value.source),
        restoredFrom: typeof value.restoredFrom === "number" ? value.restoredFrom : null,
        seen: value.seen !== false,
        changes: Array.isArray(value.changes) ? value.changes.flatMap(change) : []
      }
    ];
  }
  function parseHistory(data) {
    const answer = isRecord(data) ? data : {};
    const versions = Array.isArray(answer.versions) ? answer.versions.flatMap(version) : [];
    return { name: text(answer.name), versions: versions.sort((a, b) => b.v - a.v) };
  }
  var hasPrevious = (history, v) => history.versions.some((other) => other.v === v.v - 1);
  var topVersion = (history) => history.versions[0]?.v ?? 0;
  function seenUpTo(history) {
    const unseen2 = history.versions.filter((v) => !v.seen).map((v) => v.v);
    return unseen2.length === 0 ? topVersion(history) : Math.min(...unseen2) - 1;
  }
  var unseen = (history, upTo) => history.versions.filter((v) => v.v > upTo && v.origin !== "manuelle");
  var SOURCES = {
    dm: "Fiche modifi\xE9e \xE0 la main",
    ia: "G\xE9n\xE9ration par l\u2019IA",
    dndbeyond: "Synchronisation D&D Beyond",
    brief: "Brief oral"
  };
  function sourceLabel(v) {
    if (v.source === "restauration") return v.restoredFrom === null ? "Retour \xE0 une version ant\xE9rieure" : `Retour \xE0 la v${v.restoredFrom}`;
    return SOURCES[v.source] ?? v.source;
  }
  function originBadge(origin) {
    switch (origin) {
      case "ia":
        return { label: "IA", tone: "accent" };
      case "manuelle":
        return { label: "Manuelle", tone: "primary" };
      case "dndbeyond":
        return { label: "D&D Beyond", tone: "success" };
      case "import":
        return { label: "Import de notes", tone: null };
      default:
        return { label: origin, tone: null };
    }
  }
  function restoreBody(dataCapability, entity, restore, idempotencyKey) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      target: { aggregate: ENTITY_AGGREGATE, id: entity },
      payload: restore.changes === void 0 ? { version: restore.version } : { version: restore.version, changes: restore.changes },
      idempotencyKey,
      caller: { by: "dm", from: "VersionHistory", roles: [] }
    };
  }
  var VIOLATIONS = {
    "alias-unique-in-campaign": "un alias de cette version appartient d\xE9j\xE0 \xE0 un autre \xE9l\xE9ment",
    "not-found": "cette version n\u2019existe pas pour cette fiche"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    const violations = Array.isArray(result.violations) ? result.violations.filter((v) => typeof v === "string") : [];
    return { status: text(result.status), violations };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/campaign/version-history/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var reads = `data-reads="${LIST_ENTITY_VERSIONS}"`;
  var when = (at) => {
    const date = new Date(at);
    return Number.isNaN(date.getTime()) ? at : date.toLocaleString("fr-FR", { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
  };
  function skeleton() {
    return `<div class="mf-versions" id="version-history" ${reads}><div class="panel__head" id="versions-head" hidden><h2 id="versions-title"></h2></div><div id="versions-trigger" ${reads}></div><p class="small muted" id="versions-note" hidden></p></div>`;
  }
  function trigger(unseen2) {
    const button = unseen2 > 0 ? `<button type="button" class="primary" id="versions-updated" title="L\u2019IA a modifi\xE9 la fiche : voir le diff et l\u2019historique">Mis \xE0 jour${unseen2 > 1 ? ` (${unseen2})` : ""}</button>` : '<button type="button" class="ghost" id="versions-open">Historique des versions</button>';
    return `<div class="kit-button kit-button--bare kit-button--small">${button}</div>`;
  }
  function diff(v, pick) {
    const rows = v.changes.map(
      (c, i) => `<li class="kit-diffview__row" data-index="${i}"><div class="kit-diffview__field">${esc(c.field)}</div><div class="kit-diffview__values"><del class="kit-diffview__old">${esc(c.old || "\u2014")}</del><ins class="kit-diffview__new">${esc(c.new || "\u2014")}</ins></div>` + (pick ? '<label class="kit-diffview__action"><input type="checkbox" class="kit-diffview__check"><span>Annuler ce changement</span></label>' : "") + "</li>"
    ).join("");
    return `<div class="kit-diffview"><header class="kit-diffview__head"><h2 class="kit-diffview__title">${esc(`Version ${v.v} \u2014 ${sourceLabel(v)}`)}</h2><span class="kit-diffview__meta">${esc(`${when(v.date)} \xB7 ${v.changes.length} champ(s)`)}</span></header><ul class="kit-diffview__list">${rows}</ul></div>`;
  }
  function item(v, isNewest, isUnseen) {
    const badge = originBadge(v.origin);
    const badges = [`<span${badge.tone === null ? "" : ` class="${badge.tone}"`}>${esc(badge.label)}</span>`];
    if (isUnseen) badges.push('<span class="accent">Non vue</span>');
    const restore = isNewest ? "" : `<button type="button" class="secondary" data-action="restore:${v.v}" data-writes="${RESTORE_ENTITY_VERSION}">Revenir \xE0 cette version</button>`;
    return `<li class="mf-versions__item${isUnseen ? " is-unseen" : ""}" id="version-${v.v}"><div class="panel__head"><b>v${v.v}</b><div class="kit-badge">${badges.join("")}</div></div><span>${esc(sourceLabel(v))}</span><span class="small muted">${esc(`${when(v.date)} \xB7 ${v.changes.map((c) => c.field).join(", ")}`)}</span><div class="kit-button kit-button--bare kit-button--small"><button type="button" class="ghost" data-action="show:${v.v}">Voir le diff</button>${restore}</div></li>`;
  }
  function body(history, target, unseen2) {
    return `<div id="versions-diff" ${reads}>${target ? diff(target, hasPrevious(history, target)) : "<p>Aucune version.</p>"}</div><h3>Historique</h3><ul class="mf-versions__list" id="versions-list" ${reads}>${history.versions.map((v, i) => item(v, i === 0, unseen2.has(v.v))).join("")}</ul><p class="mf-versions__status" id="versions-status" aria-live="polite" hidden></p>`;
  }
  function dialog(name, inner) {
    return `<div class="mf-modal mf-modal--wide" id="versions-dialog"><div class="kit-dialog"><div class="kit-dialog__veil"><div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="${esc(`Versions de \xAB ${name} \xBB`)}"><header class="kit-dialog__header"><h2 class="kit-dialog__title">${esc(`Versions de \xAB ${name} \xBB`)}</h2><button class="kit-dialog__close" type="button" aria-label="Fermer">\xD7</button></header><div class="kit-dialog__body" id="versions-body">${inner}</div><footer class="kit-dialog__footer"><button type="button" class="kit-dialog__btn kit-dialog__btn--ghost" id="versions-close">Fermer</button><button type="button" class="kit-dialog__btn kit-dialog__btn--primary" id="versions-revert" data-action="revert" data-writes="${RESTORE_ENTITY_VERSION}">Annuler les changements coch\xE9s</button></footer></div></div></div></div>`;
  }

  // apps/campaign/version-history/src/style.ts
  var STYLE = `
.mf-versions, .mf-modal { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-versions *, .mf-versions *::before, .mf-versions *::after, .mf-modal *, .mf-modal *::before, .mf-modal *::after { box-sizing: inherit; }
.mf-versions [hidden], .mf-modal [hidden] { display: none !important; }
.mf-versions h2, .mf-modal h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-versions h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-modal h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-versions p, .mf-modal p { margin: 0; }
.mf-versions .panel__head, .mf-modal .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-versions .muted, .mf-modal .muted { color: var(--color-text-muted); }
.mf-versions .small, .mf-modal .small { font-size: var(--font-size-small); }
.mf-versions { display: inline-flex; }
.mf-versions.is-panel { display: grid; gap: var(--space-3); }
.mf-versions__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-versions__item { display: grid; gap: var(--space-1); padding: var(--space-2) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-versions__item.is-unseen { border-color: var(--color-accent); }
.mf-versions__item .panel__head b { font-family: var(--font-family-heading); color: var(--color-accent); }
.mf-versions__status { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-versions__status.is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-versions__status.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-modal { position: fixed; inset: 0; z-index: 100; }
.mf-modal .kit-dialog, .mf-modal .kit-dialog__veil { height: 100%; }
.mf-modal .kit-dialog__window { max-height: 88vh; }
.mf-modal--wide .kit-dialog__window { max-width: calc(var(--space-5) * 20); }
.mf-modal .kit-dialog__body > * + * { margin-top: var(--space-3); }
`;

  // apps/campaign/version-history/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  var EMPTY = { name: "", versions: [] };
  var clicks = 0;
  var keyOf = (entity, restore) => `VersionHistory:${entity}:${restore.version}:${(restore.changes ?? []).join(",")}:${Date.now().toString(36)}-${clicks += 1}`;
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="VersionHistory"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "VersionHistory";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, props2, shell2, timing = WAITING) {
    const doc = host2.ownerDocument;
    ensureStyle(doc);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`VersionHistory: #${id} is not in the panel`);
      return found;
    };
    let current2 = props2;
    let history = EMPTY;
    let unreadable = false;
    const looked = /* @__PURE__ */ new Map();
    let generation = 0;
    let busy = false;
    let modal = null;
    let loading = Promise.resolve(true);
    const entity = () => current2.entity ? current2.entity : void 0;
    const unseenNow = () => {
      const id = entity();
      return id === void 0 ? [] : unseen(history, looked.get(id) ?? topVersion(history));
    };
    const lookAtAll = () => {
      const id = entity();
      if (id !== void 0) looked.set(id, topVersion(history));
    };
    const paintTrigger = () => {
      part("versions-trigger").innerHTML = trigger(unseenNow().length);
      const title = current2.title ?? "";
      const head = part("versions-head");
      const note = part("versions-note");
      const top = history.versions[0];
      part("version-history").classList.toggle("is-panel", title !== "");
      part("versions-title").textContent = title;
      head.hidden = title === "";
      if (title === "") part("version-history").insertBefore(part("versions-trigger"), note);
      else head.append(part("versions-trigger"));
      note.classList.toggle("is-error", unreadable);
      note.hidden = !unreadable && (title === "" || top === void 0);
      note.textContent = unreadable ? "Impossible de lire l\u2019historique des versions." : `${history.name || "La fiche"} a \xE9t\xE9 mise \xE0 jour : ${top ? sourceLabel(top) : ""}. Ouvrez l\u2019historique pour revenir \xE0 une version ant\xE9rieure.`;
    };
    const showStatus = () => {
      const status = modal?.el.querySelector("#versions-status");
      if (!status || !modal?.status) return;
      status.textContent = modal.status.text;
      status.className = `mf-versions__status${modal.status.kind === "busy" ? "" : ` is-${modal.status.kind}`}`;
      status.hidden = false;
    };
    const say = (text2, kind) => {
      if (!modal) return;
      modal.status = { text: text2, kind };
      showStatus();
    };
    const lock = (locked) => {
      for (const button of modal?.el.querySelectorAll("#versions-body button, #versions-revert") ?? []) button.disabled = locked;
      if (!locked) paintRevert();
    };
    function paintRevert() {
      const button = modal?.el.querySelector("#versions-revert");
      const target = history.versions.find((v) => v.v === modal?.shown);
      if (button) button.disabled = busy || !target || !hasPrevious(history, target);
    }
    function paintModal(target) {
      if (!modal) return;
      modal.shown = target?.v;
      const inner = modal.el.querySelector("#versions-body");
      if (inner) inner.innerHTML = body(history, target, new Set(unseenNow().map((v) => v.v)));
      showStatus();
      paintRevert();
      lookAtAll();
      paintTrigger();
    }
    async function fetchHistory() {
      const id = entity();
      const mine = generation += 1;
      if (id === void 0) {
        history = EMPTY;
        unreadable = false;
        paintTrigger();
        return true;
      }
      try {
        const read = parseHistory((await shell2.capability(LIST_ENTITY_VERSIONS, void 0, { entity: id })).data);
        if (mine !== generation) return true;
        history = read;
        unreadable = false;
        if (!looked.has(id)) looked.set(id, seenUpTo(read));
        paintTrigger();
        if (modal && !busy) paintModal(read.versions.find((v) => v.v === modal?.shown) ?? read.versions[0]);
        return true;
      } catch {
        if (mine === generation) {
          unreadable = true;
          paintTrigger();
        }
        return false;
      }
    }
    const load = () => loading = fetchHistory();
    function close() {
      modal?.close();
    }
    function open(focus) {
      close();
      const first3 = focus ?? unseenNow()[0] ?? history.versions[0];
      const wrapper = doc.createElement("div");
      wrapper.innerHTML = dialog(history.name || "Nouvelle fiche", body(history, first3, new Set(unseenNow().map((v) => v.v))));
      const el = wrapper.firstElementChild;
      doc.body.append(el);
      const onKey = (event) => {
        if (event.key === "Escape") close();
      };
      const mine = {
        el,
        shown: first3?.v,
        status: null,
        close() {
          el.remove();
          doc.removeEventListener("keydown", onKey);
          if (modal === mine) modal = null;
        }
      };
      modal = mine;
      doc.addEventListener("keydown", onKey);
      el.addEventListener("click", onDialogClick);
      el.addEventListener("change", (event) => {
        const box = event.target;
        if (box?.classList.contains("kit-diffview__check")) box.closest(".kit-diffview__row")?.classList.toggle("kit-diffview__row--reverted", box.checked);
      });
      paintRevert();
      lookAtAll();
      paintTrigger();
    }
    function onDialogClick(event) {
      const target = event.target;
      if (!target || !modal) return;
      if (target.classList.contains("kit-dialog__veil") || target.closest(".kit-dialog__close, #versions-close")) return close();
      const button = target.closest("button[data-action]");
      if (!button || busy) return;
      const [act, number] = (button.dataset.action ?? "").split(":");
      if (button.dataset.writes !== RESTORE_ENTITY_VERSION && act !== "show") return;
      if (act === "show") {
        const version2 = history.versions.find((v) => v.v === Number(number));
        if (modal && version2) {
          modal.status = null;
          paintModal(version2);
        }
      } else if (act === "restore") void restore({ version: Number(number) }, `La v${number} est restaur\xE9e`);
      else if (act === "revert") void revert();
    }
    async function revert() {
      const shown3 = history.versions.find((v) => v.v === modal?.shown);
      if (!modal || !shown3) return;
      const ticked = [...modal.el.querySelectorAll("#versions-diff .kit-diffview__row")].filter((row) => row.querySelector(".kit-diffview__check")?.checked).map((row) => Number(row.dataset.index));
      if (ticked.length === 0) return say("Cochez les changements \xE0 annuler.", "error");
      await restore({ version: shown3.v - 1, changes: ticked }, `${ticked.length} changement(s) de la v${shown3.v} annul\xE9(s)`);
    }
    async function send(restore2, id) {
      const answer = await shell2.command(restoreBody(RESTORE_ENTITY_VERSION, id, restore2, keyOf(id, restore2)));
      const commandRef = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && commandRef !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(commandRef));
      }
      return state;
    }
    async function restore(what, done) {
      const id = entity();
      if (busy || id === void 0) return;
      busy = true;
      lock(true);
      say("Restauration en cours\u2026", "busy");
      try {
        const state = await send(what, id);
        if (state.status === "rejected") {
          const why = reasons(state.violations);
          return say(why === "" ? "Restauration refus\xE9e." : `Restauration refus\xE9e : ${why}.`, "error");
        }
        if (state.status !== "applied") return say(`La restauration n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`, "error");
        const read = await load();
        if (read && id === entity()) {
          paintModal(history.versions[0]);
          say(`${done} : la v${topVersion(history)} est en t\xEAte de l\u2019historique.`, "ok");
        } else say(`${done}, mais l\u2019historique n\u2019a pas pu \xEAtre relu.`, "ok");
      } catch {
        say("La restauration a \xE9chou\xE9.", "error");
      } finally {
        busy = false;
        lock(false);
      }
    }
    host2.addEventListener("click", (event) => {
      if (!event.target?.closest("#versions-updated, #versions-open")) return;
      void loading.then(() => open());
    });
    let first2 = true;
    const poll = {};
    poll.stop = shell2.onDataVersion?.(() => {
      if (!host2.isConnected) {
        poll.stop?.();
        return close();
      }
      if (first2) first2 = false;
      else if (!busy) void load();
    });
    paintTrigger();
    void load();
    return {
      update(next) {
        if (next.entity !== current2.entity) {
          close();
          history = EMPTY;
          current2 = next;
          paintTrigger();
        } else current2 = next;
        void load();
      },
      open: async () => {
        await loading;
        open();
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
    const mounted2 = mount2(resolveProps(raw));
    if (follows.length) onContext((name) => {
      if (follows.includes(name) && mounted2 && mounted2.update) mounted2.update(resolveProps(raw));
    });
    return mounted2;
  }

  // mock/src/server.js
  var LATENCY_MS = 120;
  var delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  var uuid = () => crypto.randomUUID ? crypto.randomUUID() : "xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx".replace(/x/g, () => Math.floor(Math.random() * 16).toString(16));
  var json2 = (body2, status = 200) => new Response(JSON.stringify(body2), { status, headers: { "content-type": "application/json" } });
  var notFound = () => json2({ error: "not-found" }, 404);
  var refused = (why) => json2({ error: why }, 409);
  var same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  var FINAL2 = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  var LEGACY = { reads: ["../src/reads.js", "READS"], commands: ["../src/commands.js", "COMMANDS"] };
  var HandlerError = class extends Error {
  };
  var shown2 = (path) => `mock/${path.replace(/^\.\.\//, "")}`;
  async function moduleAt(path) {
    const url = new URL(path, location.href).href;
    const served = await fetch(url).then((r) => r.ok, () => false);
    if (!served) return null;
    try {
      return await import(url);
    } catch (e) {
      throw new HandlerError(`${shown2(path)} does not load: ${e && e.message ? e.message : e}`);
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
      if (own && kept) throw new HandlerError(`${id} is defined twice: mock/handlers/${id}.js and ${shown2(LEGACY[kind][0])} \u2014 keep one`);
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
    const command = async (body2) => {
      const [dataCapability, version2] = String(body2.dataCapability || "").split("@");
      const payload = body2.payload || {};
      const target = body2.target || {};
      const targetId = target.id || body2.targetId;
      const by = body2.caller && body2.caller.by || body2.by || "anonymous";
      const handler = await s.handlerOf("commands", dataCapability);
      if (body2.idempotencyKey) {
        const seen = [...s.queue.values()].find((e) => e.idempotencyKey === body2.idempotencyKey && e.dataCapability === dataCapability);
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
      const entry = { commandId: commandId2, dataCapability, version: Number(version2) || 1, aggregate, partition, targetId, by, payload, basedOn: body2.basedOn, idempotencyKey: body2.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
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
      const needed = reviewNeeded(document2, fieldsOf(known && known.manifest, payload), body2.caller);
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
      } else if (ahead && (body2.mode || "confirm_on_stale") === "confirm_on_stale") {
        entry.state = "awaiting_confirmation";
        entry.aheadOf = ahead.command;
        entry.warnings.push("a value is declared ahead: confirm or cancel");
        s.tell("ValueDeclaredAhead", by, commandId2, { field: `${partition}.${ahead.field}`, yourValue: payload[ahead.field], declaredAhead: { value: ahead.value, by: ahead.by, command: ahead.command }, actions: ["confirm_overwrite", "cancel", "edit"] });
      }
      s.tell("CommandQueued", by, commandId2);
      await drive();
      return answer(entry);
    };
    const gesture = async (id, verb, body2) => {
      const entry = s.queue.get(id);
      if (!entry) return notFound();
      const by = body2 && body2.by || entry.by;
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
      const body2 = init && typeof init.body === "string" ? JSON.parse(init.body) : {};
      if (method === "POST" && url.pathname.startsWith("/capabilities/")) {
        const data = await read(decodeURIComponent(url.pathname.slice("/capabilities/".length)), body2.variables || {});
        return data === void 0 ? notFound() : json2({ asOf: s.dataVersion, data });
      }
      if (method === "POST" && url.pathname === "/commands") return command(body2);
      const lookup = /^\/commands\/([^/]+)$/.exec(url.pathname);
      if (lookup && method === "GET") {
        const entry = s.commands.get(decodeURIComponent(lookup[1]));
        return entry ? json2({ result: resultOf(entry) }) : notFound();
      }
      const move = /^\/commands\/([^/]+)\/(confirm|cancel|requeue|approve|reject)$/.exec(url.pathname);
      if (move && method === "POST") return gesture(decodeURIComponent(move[1]), move[2], body2);
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

  // mock/src/VersionHistory.js
  var { host, props } = mountPoint();
  var server = mockServer();
  var shell = shellOnce((s) => createShell((input, init) => s.fetch(new URL(input, location.href), init)));
  var param = (name) => new URLSearchParams(location.search).get(name);
  var first = param("id") || props.entity || (param("new") ? "nouvelle-fiche" : "morlox");
  var current = { ...props, entity: first };
  var mounted = compose(current, (resolved) => mount(host, { ...resolved, entity: first }, shell));
  Mock.on("entity-updated", async (u) => {
    if (u.versioned) {
      server.bump();
      mounted.update({ ...current });
      return;
    }
    const all = await server.load("campaign.listEntityVersions");
    const entities = await server.load("entities");
    const known = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === u.id);
    const history = all[u.id] = all[u.id] || { name: known ? known.name : u.id, versions: [] };
    history.versions.unshift({ v: (history.versions[0] ? history.versions[0].v : 0) + 1, date: (/* @__PURE__ */ new Date()).toISOString(), origin: u.origin, source: u.source, restoredFrom: null, seen: false, changes: u.changes });
    server.bump();
    mounted.update(current);
  });
  Mock.on("entity-opened", (u) => {
    current = { ...current, entity: u.id || "nouvelle-fiche" };
    mounted.update(current);
  });
  Mock.on("open-versions", (u) => {
    current = { ...current, entity: u.id };
    mounted.update(current);
    mounted.open();
  });
})();
