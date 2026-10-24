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
  function createShell(fetcher = (input, init) => fetch(input, init)) {
    return {
      capability: async (id, sha256, variables = {}) => await call(fetcher, "POST", `/capabilities/${encodeURIComponent(id)}`, { sha256, variables }),
      command: (body2) => call(fetcher, "POST", "/commands", body2),
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
      const shown2 = history.versions.find((v) => v.v === modal?.shown);
      if (!modal || !shown2) return;
      const ticked = [...modal.el.querySelectorAll("#versions-diff .kit-diffview__row")].filter((row) => row.querySelector(".kit-diffview__check")?.checked).map((row) => Number(row.dataset.index));
      if (ticked.length === 0) return say("Cochez les changements \xE0 annuler.", "error");
      await restore({ version: shown2.v - 1, changes: ticked }, `${ticked.length} changement(s) de la v${shown2.v} annul\xE9(s)`);
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

  // mock/src/reads.js
  async function closedBySession(server2, variables) {
    const bar = await server2.load("LiveSessionBar");
    return bar && variables.session === bar.session.id ? bar : null;
  }
  var bridged = /* @__PURE__ */ new WeakSet();
  async function instanceOf(server2, id) {
    const data = await server2.load("table.getBattlemapInstance");
    if (!data) return void 0;
    if (id && data.id !== id) return server2.instances ? server2.instances.get(id) : void 0;
    const store = window.Mock && window.Mock.store;
    if (store && !bridged.has(data)) {
      bridged.add(data);
      data.state = store.get(`map:${data.id}`, data.state);
      store.on(`map:${data.id}`, (state) => {
        if (state && state !== data.state) {
          data.state = state;
          server2.bump();
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
    const current2 = Math.max(0, Math.min(hpMax, hp));
    return current2 === 0 ? "mort" : current2 === hpMax ? "indemne" : 2 * current2 > hpMax ? "bless\xE9" : "en sang";
  }
  async function dressed(server2, tokens) {
    const characters = await server2.load("campaign.listCharacters") || [];
    return tokens.map((t) => {
      const c = characters.find((x) => x.id === t.char);
      return c ? { ...t, initials: c.initials, color: c.color, portrait: c.portrait } : t;
    });
  }
  var diffusionBridged = false;
  async function diffusionRow(server2) {
    const fixture = await server2.load("live.getDiffusion");
    const store = window.Mock && window.Mock.store;
    if (store && !diffusionBridged) {
      diffusionBridged = true;
      store.on("diffusion", () => server2.bump());
    }
    return store ? store.get("diffusion", fixture) : structuredClone(fixture);
  }
  async function ratioOfIllustration(server2, id) {
    const media = await server2.load("media.listMedia");
    const found = media && media.illustrations.find((i) => i.id === id);
    return found && found.ratio > 0 ? found.ratio : 1;
  }
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e" };
  var READS = {
    // session.getLiveSession (crates/session/capabilities/get-live-session): the running session as
    // `{ session, microphone }` — its header and the PCs ticked present (those of the session in
    // `session.listSessions`), the microphone listening until the session is closed — and, beside
    // them, the scenes of its screen (fixtures/LiveSessionBar.json) the bar offers and marks the
    // current one of.
    "session.getLiveSession": async (server2) => {
      const live = await server2.load("session.getLiveSession");
      const bar = await server2.load("LiveSessionBar");
      if (!live) return live;
      const listed = (await server2.load("session.listSessions")).find((s) => s.id === live.id);
      const status = live.status || "live";
      return {
        session: { ...live, status, statusLabel: STATUS_LABELS[status] ?? null, present: listed?.present ?? [] },
        microphone: { on: status !== "closed" },
        ...bar ? { scenes: bar.scenes, currentScene: bar.currentScene } : {}
      };
    },
    "session.getSessionJournal": async (server2, variables) => {
      const bar = await closedBySession(server2, variables);
      if (!bar) return server2.load("session.getSessionJournal");
      return { journal: Array.from({ length: bar.closing.journalLines }, (_, i) => ({ t: bar.session.startedAt, text: `Ligne ${i + 1} du journal transcrit` })) };
    },
    "session.getPreviousSessionSummary": async (server2, variables) => {
      const bar = await closedBySession(server2, variables);
      if (!bar) return server2.load("session.getPreviousSessionSummary");
      const { id, order, title, date } = bar.session;
      return {
        session: { id, order, title, date },
        summary: bar.closing.summary,
        summaryMeta: `G\xE9n\xE9r\xE9 \xE0 la cl\xF4ture par ${bar.closing.summaryModel}`,
        mentions: bar.closing.mentions.map((name) => ({ id: name, type: "pnj", name })),
        threads: bar.closing.threads
      };
    },
    // live.getDiffusion (crates/live/capabilities/get-diffusion): the one row both
    // player views read, so view 'dm' and view 'player' answer the same values —
    // position, size, zoom and pan. The window's x and w are fractions of the stage's
    // width, y of its height; h, derived from the image's ratio, is in the width's unit
    // and never stored. Nothing else leaves the row: no token, no HP figure, no note.
    "live.getDiffusion": async (server2) => {
      const row = await diffusionRow(server2);
      const win = row.window;
      const ratio = win ? await ratioOfIllustration(server2, win.id || win.media) : 1;
      return {
        base: { kind: row.base.kind, id: row.base.id || "" },
        window: win ? { media: win.id || win.media, x: win.x, y: win.y, w: win.w, h: win.w / ratio } : null,
        view: row.view ? { zoom: row.view.z ?? row.view.zoom, panX: row.view.px ?? row.view.panX, panY: row.view.py ?? row.view.panY } : null
      };
    },
    // table.getBattlemapInstance (crates/table/capabilities/get-battlemap-instance):
    // the DM sees everything; the players no hidden token, no hit point figure, no
    // note — each visible NPC gives its HealthBand@1 band instead.
    "table.getBattlemapInstance": async (server2, { instance, view } = {}) => {
      const data = await instanceOf(server2, instance);
      if (!data) return data;
      const dressedTokens = (await dressed(server2, data.state.tokens)).map((t) => t.kind === "pnj" ? { ...t, dead: t.hp === 0 } : t);
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
    "session.getSession": async (server2, { id }) => {
      const session = (await server2.load("session.listSessions")).find((s) => s.id === id);
      if (!session) return null;
      const status = session.status === "played" ? "closed" : session.status;
      return { id: session.id, order: session.order, title: session.title, date: session.date, status, statusLabel: STATUS_LABELS[status] ?? null, present: session.present };
    },
    // campaign.listImportedNotes (crates/campaign/capabilities/list-imported-notes):
    // the notes already imported, newest first, and the proposals of the latest
    // analysed note still awaiting validation. The imports are the fixture's
    // list; the proposals are what `ai.extractFromNotes` recorded (commands.js).
    "campaign.listImportedNotes": async (server2) => {
      const fixture = await server2.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture && fixture.imports || [];
      return { imports: [...imports].sort((x, y) => String(y.date).localeCompare(String(x.date))), proposals: server2.noteProposals || [] };
    },
    // soundbox.listSounds (crates/soundbox/capabilities/list-sounds): the
    // campaign's sounds with their effective gain — the own volume times the
    // master volume, both percents (EffectiveVolume@1) — and whether one plays
    // now (the fixture's `playing`, which soundbox.playSound and soundbox.stopAll move). The optional `search` keeps the
    // names that hold it and `tag` the sounds that carry it, whatever the case
    // (SoundTag@1); the fixture's `duration` is kept, the store has none yet.
    "soundbox.listSounds": async (server2, { search, tag }) => {
      const { masterVolume, sounds } = await server2.load("soundbox.listSounds");
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
    "media.getBattlemap": async (server2, { id }) => {
      const battlemap = (await server2.load("media")).battlemaps.find((b) => b.id === id);
      if (!battlemap) return null;
      const { opacity, ...grid } = battlemap.grid;
      return { ...battlemap, grid: { ...grid, opacity: opacity / 100, feetPerHex: 5 } };
    },
    // session.listScenes (crates/session/capabilities/list-scenes): a session's scenes by rank. The fixture
    // is the scenes of session 4, the one in preparation; another session has none yet.
    "session.listScenes": async (server2, { session }) => session === "s4" ? await server2.load("session.listScenes") : [],
    // One element's history, newest first: the version list of `entity` in the fixture; an element the fixture
    // does not know but `entities` does has never been saved (an empty history), any other has no answer (null).
    "campaign.listEntityVersions": async (server2, { entity }) => {
      const history = (await server2.load("campaign.listEntityVersions"))[entity];
      if (history) return history;
      const entities = await server2.load("entities");
      const known = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === entity);
      return known ? { name: known.name, versions: [] } : null;
    },
    // campaign.getEntity (crates/campaign/capabilities/get-entity): one element's sheet, as the
    // Capability answers it — the lists `campaign.saveEntity` writes, so a save is read back. A PC or
    // an NPC has its stats with their modifier computed from the score (AbilityModifier@1), its token
    // (`tokenImage`: a dedicated image) and its D&D Beyond link; a place or an event has its kind
    // (`lieu`, `evenement`), aliases, description and notes alone. An unknown element is `null`.
    "campaign.getEntity": async (server2, { id }) => {
      const lists = [["campaign.listCharacters", null], ["campaign.listPlaces", "lieu"], ["campaign.listEvents", "evenement"]];
      for (const [name, kind] of lists) {
        const found = (await server2.load(name) || []).find((e) => e.id === id);
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
  var elementOf = async (server2, id) => {
    for (const name of new Set(Object.values(LISTS))) {
      const element = (await server2.load(name) || []).find((e) => e.id === id);
      if (element) return { element };
    }
    return {};
  };
  var changedBy = async (server2, element, patch, origin, source, changes) => {
    Object.assign(element, patch);
    const history = await server2.load("campaign.listEntityVersions") || {};
    const entry = history[element.id] || (history[element.id] = { name: element.name, versions: [] });
    const v = Math.max(0, ...entry.versions.map((version2) => version2.v)) + 1;
    entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin, source, restoredFrom: null, seen: false, changes });
  };
  var onScreen = (fraction, kept) => typeof fraction === "number" && Number.isFinite(fraction) ? Math.max(0, Math.min(1, fraction)) : kept;
  async function onWindow(server2, change2) {
    const row = await diffusionRow(server2);
    if (!row.window) return ["window-fields-together"];
    const violations = await change2(row.window);
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
  async function onToken(server2, target, token, change2) {
    const data = await instanceOf(server2, target && target.id);
    const found = data && data.state.tokens.find((t) => t.id === token);
    if (!found) return ["instance-isolated"];
    const violations = change2(found, data.state);
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
  async function onCombat(server2, id, change2) {
    const data = await instanceOf(server2, id);
    if (!data) return ["instance-isolated"];
    if (!data.state.combat) return ["one-active-combat"];
    const violations = await change2(data.state.combat, data.state);
    if (violations) return violations;
    shareInstance(data);
    return void 0;
  }
  var soundTags = (tags) => [...new Set((Array.isArray(tags) ? tags : []).map((t) => [...String(t).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "")].map((c) => c.toLowerCase()).join("")).filter(Boolean))];
  var COMMANDS = {
    // table.moveToken (crates/table/data-capabilities/move-token): (q, r) is the
    // one position both views read.
    "table.moveToken": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      t.q = payload.q;
      t.r = payload.r;
    }),
    // table.renameToken: only that token's name; an empty name is refused.
    "table.renameToken": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      if (!String(payload.name || "").trim()) return ["name-required"];
      t.name = String(payload.name).trim();
      return void 0;
    }),
    // table.setTokenVisibility: hidden tokens keep their place; the players do not read them.
    "table.setTokenVisibility": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      t.visible = payload.visible === true;
    }),
    // table.setTokenCondition (crates/table/data-capabilities/set-token-condition): the badge
    // `payload.state` put on that token when `payload.on`, after the others and once, or taken off,
    // the others keeping their order — its conditions alone. A state that is no name without the
    // blanks around it, or holds U+0000, or an `on` that is no boolean, is `state-persisted-reset`.
    "table.setTokenCondition": (server2, { payload, targetId }) => onToken(server2, { id: targetId || payload.instance }, payload.token, (t) => {
      const state = typeof payload.state === "string" ? payload.state.trim() : "";
      if (!state || state.includes("\0") || typeof payload.on !== "boolean") return ["state-persisted-reset"];
      const held = t.states || [];
      t.states = payload.on ? held.includes(state) ? held : [...held, state] : held.filter((s) => s !== state);
      return void 0;
    }),
    // table.removeToken: the token leaves the map.
    "table.removeToken": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t, state) => {
      state.tokens = state.tokens.filter((x) => x !== t);
    }),
    // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
    // hp-change-one-token — the targeted token only; pj-hp-not-tracked — a PC has
    // no hit points; the current ones stay within 0 and the maximum, and an NPC
    // is dead iff they are 0 (dead-derived).
    "table.changeHitPoints": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      if (t.kind !== "pnj" || typeof t.hp !== "number") return ["pj-hp-not-tracked"];
      t.hp = Math.max(0, Math.min(t.hpMax, t.hp + Math.trunc(Number(payload.delta) || 0)));
      t.dead = t.hp === 0;
      return void 0;
    }),
    // table.startCombat: the NPC roll a d20 plus the DEX modifier of their sheet,
    // the DM enters the PCs' initiative; the order is the initiative, then the DEX,
    // then at random (initiative-tiebreak). A hidden token is in the order and keeps its rank.
    "table.startCombat": async (server2, { payload, targetId }) => {
      const data = await instanceOf(server2, targetId);
      if (!data) return ["instance-isolated"];
      if (data.state.combat) return ["one-active-combat"];
      const characters = await server2.load("campaign.listCharacters") || [];
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
    "table.nextTurn": (server2, { targetId }) => onCombat(server2, targetId, (combat, state) => {
      advance(combat, state);
      return void 0;
    }),
    // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative): the open combat
    // reordered as `payload.order` gives it, every participant once, named by its token — the ranks
    // 1 to n in that order; the turn and the round stay. An order that is no list, omits a
    // participant, repeats one or names one not in the combat is `initiative-tiebreak`.
    "table.reorderInitiative": (server2, { payload, targetId }) => onCombat(server2, targetId || payload.instance, (combat) => {
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
    "table.addParticipant": (server2, { payload, targetId }) => onCombat(server2, targetId || payload.instance, async (combat, state) => {
      const token = state.tokens.find((t) => t.id === payload.token);
      if (!token) return ["not-found"];
      if (combat.order.some((o) => o.id === token.id)) return ["initiative-tiebreak"];
      const pc = token.kind === "pj";
      if (pc ? !Number.isInteger(payload.initiative) : payload.initiative != null) return ["npc-initiative-bounds"];
      const dex = dexModifier(await server2.load("campaign.listCharacters") || [], token.char);
      const init = pc ? payload.initiative : 1 + Math.floor(Math.random() * 20) + dex;
      const at = combat.order.findIndex((o) => o.init < init || o.init === init && o.dex < dex);
      combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token.id, init, dex });
      return void 0;
    }),
    // table.removeParticipant (crates/table/data-capabilities/remove-participant): the participant
    // `payload.participant` — named by its token, as the carousel shows it — taken out of the open combat;
    // its token stays on the map and the order closes up. The participant that holds the turn is refused
    // by `next-turn`: the Battlemap sends table.nextTurn first. One not in the combat is `not-found`.
    "table.removeParticipant": (server2, { payload, targetId }) => onCombat(server2, targetId, (combat) => {
      if (!combat.order.some((o) => o.id === payload.participant)) return ["not-found"];
      if (combat.current === payload.participant) return ["next-turn"];
      combat.order = combat.order.filter((o) => o.id !== payload.participant);
      return void 0;
    }),
    // table.endCombat: the combat ends; the tokens stay as they are.
    "table.endCombat": (server2, { targetId }) => onCombat(server2, targetId, (combat, state) => {
      state.combat = null;
      return void 0;
    }),
    // table.resetBattlemap: state-persisted-reset — the instance's tokens, combat and initiative are removed: an empty map.
    "table.resetBattlemap": async (server2, { targetId }) => {
      const data = await instanceOf(server2, targetId);
      if (!data) return ["instance-isolated"];
      data.state = { tokens: [], combat: null };
      data.pjInitiative = {};
      shareInstance(data);
      return void 0;
    },
    // table.placeTokens: one character dropped, once or « × n »; the new tokens
    // are numbered after those already taken for it (multi-drop-numbering), a
    // monster takes its hit points from its sheet, a PC has one token.
    "table.placeTokens": async (server2, { payload, targetId }) => {
      const data = await instanceOf(server2, targetId);
      const character = (await server2.load("campaign.listCharacters") || []).find((c) => c.id === payload.character);
      if (!data || !character) return ["instance-isolated"];
      const { tokens } = data.state;
      const mine = tokens.filter((t) => t.char === character.id);
      const pnj = character.kind === "pnj";
      if (!pnj && (mine.length || !(data.roster || []).includes(character.id))) return ["token-roster"];
      const count = pnj ? Math.max(1, Math.min(12, Number(payload.count) || 1)) : 1;
      const base = character.tokenName || String(character.name).split(" ")[0];
      let num = Math.max(0, ...mine.map((t) => t.num || 0));
      const numbered = pnj && (count > 1 || mine.length > 0);
      for (const hex of freeHexes({ q: payload.q, r: payload.r }, tokens, count)) {
        num += 1;
        tokens.push({
          id: `t-${character.id}${pnj ? `-${num}` : ""}`,
          char: character.id,
          kind: character.kind,
          name: numbered ? `${base} ${num}` : base,
          num: pnj ? num : null,
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
    "live.moveWindow": (server2, { payload }) => onWindow(server2, (w) => {
      w.x = onScreen(payload.x, w.x);
      w.y = onScreen(payload.y, w.y);
    }),
    // live.closeWindow (crates/live/data-capabilities/close-window): the window's
    // media, position and width are emptied together (window-fields-together);
    // the base and the framing stay. A window already closed is closed again.
    "live.closeWindow": async (server2) => {
      const row = await diffusionRow(server2);
      row.window = null;
      if (window.Mock) window.Mock.store.set("diffusion", row);
      return void 0;
    },
    // live.resizeWindow (crates/live/data-capabilities/resize-window): only the width
    // is stored, the largest that fits the box asked for at the image's ratio — w
    // alone with no h — from 0.1 to the screen's width, to 4 decimals
    // (window-proportional); the position stays. A w that is no number, which the
    // server refuses, keeps the width.
    "live.resizeWindow": (server2, { payload }) => onWindow(server2, async (w) => {
      if (typeof payload.w !== "number" || !Number.isFinite(payload.w)) return void 0;
      const ratio = await ratioOfIllustration(server2, w.id || w.media);
      const fit = typeof payload.h === "number" && Number.isFinite(payload.h) ? Math.min(payload.w, payload.h * ratio) : payload.w;
      w.w = Math.round(Math.max(0.1, Math.min(1, fit)) * 1e4) / 1e4;
      return void 0;
    }),
    // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
    // a battlemap instance of the session, or an illustration of the campaign,
    // becomes the player view's base, framed as the DM framed it (1 and 0, 0
    // when no framing is sent); the floating window stays as it is. Any other
    // base would leave it incoherent (`base-coherent`).
    "live.showOnPlayerView": async (server2, { payload }) => {
      let base;
      if (payload.kind === "battlemap") {
        const data = await instanceOf(server2, payload.instance);
        if (!data) return ["base-coherent"];
        base = { kind: "battlemap", id: data.id };
      } else if (payload.kind === "illustration") {
        const listed = [await server2.load("media"), await server2.load("media.listMedia")];
        if (!listed.some((media) => media && media.illustrations.some((i) => i.id === payload.media))) return ["base-coherent"];
        base = { kind: "illustration", id: payload.media };
      } else return ["base-coherent"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.base = base;
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
    "live.showWindowOnPlayerView": async (server2, { payload }) => {
      const id = payload.media ?? payload.id;
      if (!id) return payload.kind && payload.kind !== "illustration" ? ["window-proportional"] : void 0;
      const listed = [await server2.load("media"), await server2.load("media.listMedia")];
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
    "soundbox.addSound": async (server2, { payload }) => {
      const name = typeof payload.name === "string" ? payload.name.trim() : "";
      const file = typeof payload.file === "string" ? payload.file.trim() : "";
      if (!name || !file) return ["unprocessable"];
      const tags = payload.tags ?? [];
      if (!Array.isArray(tags) || !tags.every((t) => typeof t === "string")) return ["tags-normalized"];
      const given = payload.volume ?? 100;
      const volume = typeof given === "number" || typeof given === "string" && given.trim() !== "" ? Number(given) : NaN;
      if (!(volume >= 0 && volume <= 100)) return ["sound-volume-range"];
      const library = await server2.load("soundbox.listSounds");
      library.sounds.unshift({ id: server2.uuid(), name, tags: soundTags(tags), duration: 0, volume: Math.round(volume), file });
      return void 0;
    },
    // session.createSession (crates/session/data-capabilities/create-session):
    // the next session of the campaign, at the position after the last, in
    // preparation, with the PCs ticked present. `present-only-pj` refuses a
    // character that is not a PC of the fixtures; `oneshot-single-session` a
    // second session of a oneshot.
    "session.createSession": async (server2, { payload }) => {
      const sessions = await server2.load("session.listSessions");
      const characters = await server2.load("campaign.listCharacters");
      const campaign = (await server2.load("campaign.listCampaigns") || []).find((c) => c.id === payload.campaign);
      const present = Array.isArray(payload.present) ? payload.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
      if (campaign && campaign.type === "oneshot" && (campaign.sessions ?? 0) > 0) return ["oneshot-single-session"];
      if (campaign) campaign.sessions = (campaign.sessions ?? 0) + 1;
      sessions.push({
        id: server2.uuid(),
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
    "session.addPresentCharacter": async (server2, { payload, targetId }) => {
      const session = (await server2.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
      if (!session) return ["not-found"];
      const characters = await server2.load("campaign.listCharacters");
      if (!characters.some((c) => c.kind === "pj" && c.id === payload.character)) return ["present-only-pj"];
      if (!session.present.includes(payload.character)) session.present.push(payload.character);
      return void 0;
    },
    // session.reorderScenes (crates/session/data-capabilities/reorder-scenes):
    // the session's scenes in the order the DM gives, every rank rewritten at
    // once, 1 to n. An order that omits a scene of the fixtures' session,
    // repeats one or names one it does not hold is `scene-order-contiguous`,
    // and no rank changes.
    "session.reorderScenes": async (server2, { payload }) => {
      const scenes = await server2.load("session.listScenes");
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
    "session.removePresentCharacter": async (server2, { payload, targetId }) => {
      const session = (await server2.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
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
    "media.uploadMedia": async (server2, { payload }) => {
      const { kind, place = null } = payload;
      if (kind !== "battlemap" && kind !== "illustration") return ["media-kind"];
      if (place && !(await server2.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["media-place-is-lieu"];
      const base = { id: server2.uuid(), title: String(payload.title || "").trim() || "Sans titre", file: payload.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      for (const name of ["media", "media.listMedia"]) {
        const media = await server2.load(name);
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
    "media.saveGridSettings": async (server2, { payload, targetId }) => {
      const battlemap = (await server2.load("media")).battlemaps.find((b) => b.id === targetId);
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
    "media.setMediaPlace": async (server2, { payload, targetId }) => {
      const id = targetId || payload.media;
      const listed = [await server2.load("media"), await server2.load("media.listMedia")];
      const found = listed.flatMap((media) => media.battlemaps.concat(media.illustrations)).filter((m) => m.id === id);
      if (!found.length) return ["not-found"];
      const place = payload.place ?? null;
      if (place !== null && !(await server2.load("campaign.listPlaces")).some((p) => p.id === place)) return ["media-place-is-lieu"];
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
    "session.addScene": async (server2, { payload }) => {
      const title = String(payload.title || "").trim();
      if (!title) return ["scene-title-required"];
      const known = new Set((await server2.load("campaign.listPlaces")).map((p) => p.id));
      const places = [...new Set(Array.isArray(payload.places) ? payload.places : [])];
      if (places.some((id) => !known.has(id))) return ["scene-links-same-campaign"];
      const library = await server2.load("media.listMedia");
      const media = library.battlemaps.concat(library.illustrations).filter((m) => places.includes(m.place)).map((m) => m.id);
      const scenes = await server2.load("session.listScenes");
      scenes.push({
        id: server2.uuid(),
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
    "session.addPlaceToSession": async (server2, { payload }) => {
      const place = payload.place;
      const scenes = await server2.load("session.listScenes") || [];
      const first2 = scenes.reduce((low, sc) => !low || sc.order < low.order ? sc : low, null);
      const scene = payload.scene ? scenes.find((sc) => sc.id === payload.scene) : first2;
      if (!scene) return ["not-found"];
      if (!(await server2.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["scene-links-same-campaign"];
      if (!scene.places.includes(place)) scene.places.push(place);
      const library = await server2.load("media.listMedia") || { battlemaps: [], illustrations: [] };
      for (const medium of library.battlemaps.concat(library.illustrations)) {
        if (medium.place === place && !scene.media.includes(medium.id)) scene.media.push(medium.id);
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
    "table.createBattlemapInstance": async (server2, { payload }) => {
      const session = await server2.load("session.getSession");
      const scene = (await server2.load("session.listScenes")).find((sc) => sc.id === payload.scene);
      const map = (await server2.load("media.listMedia")).battlemaps.find((m) => m.id === payload.battlemap);
      if (!session || session.id !== payload.session || !scene || !map) return ["not-found"];
      if (scene.battlemap) return ["one-instance-per-scene"];
      const id = `bmi-${map.id}-${scene.id}`;
      server2.instances = server2.instances || /* @__PURE__ */ new Map();
      server2.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
      scene.battlemap = id;
      return void 0;
    },
    "campaign.restoreEntityVersion": async (server2, { payload, targetId }) => {
      const history = (await server2.load("campaign.listEntityVersions"))[targetId];
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
    "campaign.exportCampaign": async (server2, { payload }) => {
      const campaign = await server2.load("campaign.getCampaign");
      if (!campaign || campaign.id !== payload.id) return ["not-found"];
      const now = /* @__PURE__ */ new Date();
      const pad = (n) => String(n).padStart(2, "0");
      campaign.lastExport = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
      return void 0;
    },
    // soundbox.updateSound (crates/soundbox/data-capabilities/update-sound): the
    // name, tags and volume of one sound, tags normalised and volume ranged as
    // for an addition; a sound that is not there is `not-found`.
    "soundbox.updateSound": async (server2, { payload, targetId }) => {
      const sound = (await server2.load("soundbox.listSounds")).sounds.find((s) => s.id === targetId);
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
    "soundbox.removeSound": async (server2, { targetId }) => {
      const library = await server2.load("soundbox.listSounds");
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
    "soundbox.setMasterVolume": async (server2, { payload, targetId }) => {
      if (!String(targetId || payload.campaign || "").trim()) return ["not-found"];
      const { volume } = payload;
      if (typeof volume !== "number" || !(volume >= 0 && volume <= 100)) return ["master-volume-range"];
      for (const name of ["soundbox.listSounds", "sounds"]) {
        const fixture = await server2.load(name);
        if (fixture) fixture.masterVolume = Math.round(volume);
      }
      return void 0;
    },
    // soundbox.stopAll (crates/soundbox/data-capabilities/stop-all): every
    // running playback gets its stop (`stop-all`); with none running it still
    // succeeds and changes nothing.
    "soundbox.stopAll": async (server2) => {
      for (const sound of (await server2.load("soundbox.listSounds")).sounds) sound.playing = false;
      return void 0;
    },
    // soundbox.setSoundVolume (crates/soundbox/data-capabilities/set-sound-volume):
    // one sound's own volume, in percent, from 0 to 100; out of range it is
    // refused by `sound-volume-range`, never clamped. Nothing else of the sound
    // changes; its effective gain follows (EffectiveVolume@1).
    "soundbox.setSoundVolume": async (server2, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const fixtures = (await Promise.all(["soundbox.listSounds", "sounds"].map((name) => server2.load(name)))).filter(Boolean);
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
    "campaign.createCampaign": async (server2, { payload }) => {
      const name = String(payload.name ?? "").trim();
      if (!name) return ["campaign-name-required"];
      if (!["campagne", "oneshot"].includes(payload.type)) return ["campaign-type-unknown"];
      const text2 = (field) => typeof payload[field] === "string" ? payload[field] : null;
      (await server2.load("campaign.listCampaigns")).unshift({
        id: server2.uuid(),
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
    "ai.saveSettings": async (server2, { payload }) => {
      const settings = await server2.load("ai.getSettings");
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
    "ai.testConnection": async (server2, { payload }) => {
      const settings = await server2.load("ai.getSettings");
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
    "ai.extractFromNotes": async (server2, { payload }) => {
      const text2 = String(payload.text || "");
      const { proposals } = await server2.load("NoteImport");
      server2.noteText = text2;
      server2.noteProposals = proposals.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
      return void 0;
    },
    // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
    // one element per accepted proposal, the raw note kept as an import listed
    // newest first, the proposals no longer awaiting.
    "campaign.createEntitiesFromNote": async (server2, { payload }) => {
      const proposals = server2.noteProposals || [];
      const accepted = Array.isArray(payload.accepted) ? payload.accepted : [];
      const created = proposals.filter((p) => accepted.includes(p.id));
      const fixture = await server2.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture.imports;
      const note = server2.noteText || "";
      imports.unshift({ id: `imp-${server2.uuid()}`, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), title: note.split("\n")[0].replace(/[.\s]+$/, "") || "Notes import\xE9es", chars: note.length, proposed: proposals.length, created: created.length });
      server2.noteProposals = [];
      return void 0;
    },
    // session.closeSession (crates/session/data-capabilities/close-session): the
    // live session closed in one action (`close-session`) — its microphone off,
    // its close dated, its journal kept as it was — and a summary of its journal
    // written by the active provider's summary model, the journal's text alone
    // sent, as a Journal request (`text-only-no-audio`). The summary is the one
    // the next preparation shows. A session that is not live is refused by
    // `status-lifecycle`; one that is not the live session is `not-found`.
    "session.closeSession": async (server2, { payload, targetId }) => {
      const live = await server2.load("session.getLiveSession");
      if (!live || live.id !== (targetId || payload.id)) return ["not-found"];
      if ((live.status || "live") !== "live") return ["status-lifecycle"];
      const settings = await server2.load("ai.getSettings");
      const binary = settings.mode === "binary" && settings.binary;
      const model = (binary || settings.providers?.[settings.provider] || {}).summaryModel || "";
      const provider = binary ? `${binary.binary}-binary` : settings.provider === "opencode" ? "opencode-server" : "cloud";
      const { journal = [] } = await server2.load("session.getSessionJournal") || {};
      const lines = journal.length ? journal.map((line) => `${line.t} ${line.text}`) : ["(aucune ligne transcrite)"];
      const text2 = [`Journal de la s\xE9ance \xAB ${live.title} \xBB`, ...lines].join("\n");
      const requests = (await server2.load("ai.listProviderRequests"))?.requests;
      if (requests) requests.push({ sequence: requests.length + 1, kind: "journal", provider, text: text2, modalities: ["text"] });
      const summary = `R\xE9sum\xE9 de la s\xE9ance ${live.order} g\xE9n\xE9r\xE9 par ${model} \xE0 partir de ${journal.length} lignes du journal.`;
      Object.assign(live, { status: "closed", closedAt: (/* @__PURE__ */ new Date()).toISOString(), microphone: { on: false }, summary, summaryModel: model });
      const previous = await server2.load("session.getPreviousSessionSummary");
      if (previous) Object.assign(previous, { session: { id: live.id, order: live.order, title: live.title, date: live.date }, summary, summaryMeta: `G\xE9n\xE9r\xE9 \xE0 la cl\xF4ture par ${model}` });
      return void 0;
    },
    // campaign.saveEntity (crates/dataguard/data-capabilities/save-entity): the
    // DM's explicit save of an element, its sheet and its aliases, which adds a
    // version (EntityVersion@1, source `dm`) whose changes are the fields that
    // moved. An alias naming another element, by its name or an alias
    // (AliasMatch@1), is refused by `alias-unique-in-campaign`; a player on
    // anything but a PC by `character-is-pj-or-pnj`. Only the editor's fields
    // are kept: a DEX modifier given is never written.
    "campaign.saveEntity": async (server2, { payload }) => {
      const given = payload.entity || payload;
      const lists = await Promise.all(Object.values(LISTS).filter((v2, i, all2) => all2.indexOf(v2) === i).map(async (name) => [name, await server2.load(name) || []]));
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
      const id = before.id || given.id || server2.uuid();
      after.id = id;
      const list = await server2.load(LISTS[kind]) || [];
      if (found && found.name !== LISTS[kind]) {
        const old = lists.find(([name]) => name === found.name)[1];
        old.splice(old.indexOf(before), 1);
      }
      const at = list.indexOf(before);
      if (at >= 0) list[at] = after;
      else list.push(after);
      const history = await server2.load("campaign.listEntityVersions") || {};
      const entry = history[id] || (history[id] = { name: after.name, versions: [] });
      entry.name = after.name;
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => f !== "id").sort().filter((f) => JSON.stringify(before[f] ?? null) !== JSON.stringify(after[f] ?? null)).map((field) => ({ field, old: before[field] ?? null, new: after[field] ?? null }));
      const v = Math.max(0, ...entry.versions.map((version2) => version2.v)) + 1;
      entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin: "dm", source: "Fiche enregistr\xE9e par le DM", seen: true, changes });
      return void 0;
    },
    // ai.proposeCharacter (crates/ai/data-capabilities/propose-character): the
    // initial prompt told or typed gives the character's background and behaviour and, for an NPC,
    // a statblock — the proposals of fixtures/EntityEditor.json, the fake llm's answers. It applies
    // at once and is a version like any other (source ia), unseen until the DM looks. A prompt with
    // no text, or an element the lists do not hold, is refused.
    "ai.proposeCharacter": async (server2, { payload, targetId }) => {
      const { element } = await elementOf(server2, targetId);
      if (!element || !String(payload.prompt ?? "").trim()) return ["not-found"];
      const { proposals } = await server2.load("EntityEditor");
      const proposal = proposals[element.id] || proposals.default;
      const patch = { prompt: payload.prompt, background: proposal.background, behavior: proposal.behavior };
      if (element.kind === "pnj") Object.assign(patch, { stats: { ...proposal.stats }, ca: proposal.ca, pvMax: proposal.pvMax, features: proposal.features.map((f) => ({ ...f })) });
      await changedBy(server2, element, patch, "ia", "Prompt initial", element.kind === "pnj" ? proposal.changes : proposal.changes.slice(0, 1));
      return void 0;
    },
    // dndbeyond.resyncCharacter (crates/dndbeyond/data-capabilities/resync-character): the stats,
    // hit points and spells of a PC linked to D&D Beyond are read again — the patch of
    // fixtures/EntityEditor.json, and the spells its « Sorts et capacités » line adds, as the
    // character-sheets fake plays the level-up — whose changes the history keeps; the aliases, the
    // notes and the token are left as they are (`dndbeyond-resync-scope`). A sheet with no link is
    // refused.
    "dndbeyond.resyncCharacter": async (server2, { targetId }) => {
      const { element } = await elementOf(server2, targetId);
      if (!element || !element.ddb || !element.ddb.url) return ["not-found"];
      const { ddbResync } = await server2.load("EntityEditor");
      const resync = ddbResync[element.id] || ddbResync.default;
      const { stats, pvMax } = resync.patch;
      const spells = resync.changes.find((c) => c.field === "Sorts et capacit\xE9s");
      const names = (line) => line.split(",").map((name) => name.trim()).filter(Boolean);
      const known = (element.features || []).map((f) => f.name);
      const gained = spells ? names(spells.new).filter((name) => !names(spells.old).includes(name) && !known.includes(name)) : [];
      const features = gained.length ? { features: [...element.features || [], ...gained.map((name) => ({ name, text: "Ajout\xE9 \xE0 la fiche D&D Beyond lors de la mont\xE9e de niveau." }))] } : {};
      await changedBy(server2, element, { ...stats ? { stats: { ...element.stats, ...stats } } : {}, ...pvMax ? { pvMax } : {}, ...features, ddb: { ...element.ddb, syncedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19) } }, "dndbeyond", "Resynchronisation D&D Beyond", resync.changes.filter((c) => c.field === "PV max" || c.field === "Sorts et capacit\xE9s" && gained.length || stats && c.field in stats));
      return void 0;
    },
    // dndbeyond.importCharacter (crates/dndbeyond/data-capabilities/import-character): a D&D
    // Beyond URL (or a bare id) imports the PC of the campaign linked to that character — the
    // character-sheets fake knows the ones the fixtures link, as they were last synchronised —
    // its statblock and sync date alone, its name, aliases, notes and token left as the DM made
    // them (`dndbeyond-resync-scope`), as a version of source dndbeyond. A PC targeted is filled
    // only if it is that one; a URL of another host, or of a character the fake does not know,
    // is refused and nothing is written.
    "dndbeyond.importCharacter": async (server2, { payload, targetId }) => {
      const url = String(payload.url ?? "").trim();
      const [, host2, path] = /^https?:\/\/([^/]+)\/(.*)$/.exec(url) || [];
      const match = host2 && ["www.dndbeyond.com", "dndbeyond.com"].includes(host2.toLowerCase()) ? /^(?:profile\/[^/?#]+\/)?characters\/(\d+)(?:[/?#]|$)/.exec(path) : null;
      const digits = match ? match[1] : /^\d+$/.test(url) ? url : null;
      const ddbId = digits && BigInt(digits) > 0n ? String(BigInt(digits)) : null;
      if (!ddbId) return ["not-found"];
      const characters = await server2.load("campaign.listCharacters") || [];
      const linked = characters.find((e) => e.kind === "pj" && e.ddb && String(e.ddb.url ?? "").replace(/\/+$/, "").endsWith(`/characters/${ddbId}`));
      if (!linked) return ["not-found"];
      if (targetId && targetId !== linked.id) return ["already-imported"];
      await changedBy(server2, linked, { ddb: { ...linked.ddb, syncedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19) } }, "dndbeyond", "Import D&D Beyond", []);
      return void 0;
    },
    // soundbox.playSound (crates/soundbox/data-capabilities/play-sound): a
    // playback of the sound starts, whatever else plays (`simultaneous-playback`),
    // from the soundbox (the default) or a suggestion's pastille; any other
    // source, the player view's, is refused (`dm-side-only`). The sound plays
    // as soundbox.listSounds says it: `playing`.
    "soundbox.playSound": async (server2, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const sound = (await server2.load("soundbox.listSounds")).sounds.find((candidate) => candidate.id === id);
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
  var json = (body2, status = 200) => new Response(JSON.stringify(body2), { status, headers: { "content-type": "application/json" } });
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
    const command = async (body2) => {
      const [dataCapability, version2] = String(body2.dataCapability || "").split("@");
      const payload = body2.payload || {};
      const target = body2.target || {};
      const targetId = target.id || body2.targetId;
      const by = body2.caller && body2.caller.by || body2.by || "anonymous";
      const handler = COMMANDS[dataCapability];
      if (body2.idempotencyKey) {
        const seen = [...s.queue.values()].find((e) => e.idempotencyKey === body2.idempotencyKey && e.dataCapability === dataCapability);
        if (seen) return answer(seen, true);
      }
      const aggregate = target.aggregate || null;
      const declared = (aggregate && await s.load(`aggregates/${aggregate}`) || { fields: {} }).fields || {};
      const derived = Object.keys(payload).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version2) || 1, aggregate, partition, targetId, by, payload, basedOn: body2.basedOn, idempotencyKey: body2.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
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
      return json({ result: resultOf(entry) });
    };
    s.fetch = async (input, init) => {
      await delay(LATENCY_MS);
      const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url);
      const method = init && init.method || "GET";
      const body2 = init && typeof init.body === "string" ? JSON.parse(init.body) : {};
      if (method === "POST" && url.pathname.startsWith("/capabilities/")) {
        const data = await read(decodeURIComponent(url.pathname.slice("/capabilities/".length)), body2.variables || {});
        return data === void 0 ? notFound() : json({ asOf: s.dataVersion, data });
      }
      if (method === "POST" && url.pathname === "/commands") return command(body2);
      const lookup = /^\/commands\/([^/]+)$/.exec(url.pathname);
      if (lookup && method === "GET") {
        const entry = s.commands.get(decodeURIComponent(lookup[1]));
        return entry ? json({ result: resultOf(entry) }) : notFound();
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
