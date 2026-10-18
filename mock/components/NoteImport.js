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
    constructor(status2, body) {
      super(`the server answered ${status2}`);
      this.status = status2;
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

  // apps/ai/note-import/src/ids.ts
  var LIST_IMPORTED_NOTES = "campaign.listImportedNotes";
  var EXTRACT_FROM_NOTES = "ai.extractFromNotes";
  var CREATE_ENTITIES_FROM_NOTE = "campaign.createEntitiesFromNote";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/ai/note-import/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var count = (value) => typeof value === "number" && Number.isFinite(value) ? value : 0;
  function readImport(value) {
    if (!isRecord(value) || text(value.id) === "") return null;
    return { id: text(value.id), date: text(value.date), title: text(value.title), chars: count(value.chars), proposed: count(value.proposed), created: count(value.created) };
  }
  function readProposal(value) {
    if (!isRecord(value) || text(value.id) === "") return null;
    const existing = typeof value.existing === "string" && value.existing !== "" ? value.existing : null;
    return { id: text(value.id), type: text(value.type), name: text(value.name), excerpt: text(value.excerpt), existing, checked: existing === null && value.checked !== false };
  }
  var list = (value, read) => Array.isArray(value) ? value.map(read).filter((item) => item !== null) : [];
  function parseNotes(data) {
    if (Array.isArray(data)) return { imports: list(data, readImport), proposals: [] };
    const answer = isRecord(data) ? data : {};
    return { imports: list(answer.imports, readImport), proposals: list(answer.proposals, readProposal) };
  }
  var pending = (notes, settled2) => notes.proposals.filter((proposal) => !settled2.has(proposal.id));
  var createLabel = (checked) => `Cr\xE9er la s\xE9lection (${checked})`;
  function commandBody(dataCapability, payload) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload, caller: { by: "dm", from: "NoteImport", roles: [] } };
  }
  var VIOLATIONS = {
    "text-only-payload": "seul du texte est accept\xE9",
    "no-creation-before-validation": "rien ne se cr\xE9e avant la validation"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    const violations = Array.isArray(result.violations) ? result.violations.filter((v) => typeof v === "string") : [];
    return { status: text(result.status), violations };
  }
  var isFinal = (status2) => FINAL.has(status2);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/ai/note-import/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var MIC = '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
  var PAUSE = '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';
  function button(p) {
    const writes = p.writes === void 0 ? "" : ` data-writes="${p.writes}"`;
    return `<button type="button" class="${p.variant}" id="${p.id}"${writes}${p.disabled === true ? " disabled" : ""}>${esc(p.label)}</button>`;
  }
  function skeleton() {
    return `<section class="panel mf-notes" id="note-import" aria-labelledby="notes-title"><div class="panel__head"><h2 id="notes-title">Importer des notes</h2><span class="small muted">La note brute est toujours conserv\xE9e</span></div><p class="muted small">Tapez, collez ou dictez vos notes en texte libre. L\u2019IA en extrait les personnages, lieux et \xE9v\xE9nements ; vous validez la liste en lot avant toute cr\xE9ation.</p><div id="notes-input"><div class="kit-textarea is-wide"><label class="kit-textarea__label" for="notes-text">Notes en texte libre</label><textarea class="kit-textarea__field" id="notes-text" rows="7"></textarea><div class="kit-textarea__footer"><p class="kit-textarea__hint">Collez vos notes existantes, ou dictez-les avec le micro.</p></div></div></div><div class="row" id="notes-controls"><div class="kit-micbutton kit-micbutton--compact" id="notes-mic-box" data-state="idle"><button type="button" class="kit-micbutton__btn" id="notes-mic" aria-pressed="false" aria-label="Dicter"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button><div class="kit-micbutton__text"><span class="kit-micbutton__label">Dicter</span><span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span> <span class="kit-micbutton__timer">00:00</span></span></div></div><div class="kit-button kit-button--bare">${button({ id: "notes-extract", label: "Extraire les \xE9l\xE9ments", variant: "primary", writes: EXTRACT_FROM_NOTES })}</div></div><div class="mf-notes__status" id="notes-status" aria-live="polite"></div><div id="notes-proposals" class="stack" data-reads="${LIST_IMPORTED_NOTES}" hidden></div><section id="notes-history" class="stack" aria-labelledby="notes-history-title"><h3 id="notes-history-title">Notes d\xE9j\xE0 import\xE9es</h3><ul class="list" id="notes-history-list" data-reads="${LIST_IMPORTED_NOTES}"></ul></section></section>`;
  }
  var MIC_TEXTS = { idle: "Pr\xEAt \xE0 dicter", recording: "Enregistrement", paused: "En pause" };
  function proposals(list2) {
    const option = (p) => `<label><input type="checkbox" name="proposal" value="${esc(p.id)}"${p.checked ? " checked" : ""}><span><span class="mf-notes__type">${esc(p.type)}</span>${esc(p.name)}${p.existing === null ? "" : ' <span class="mf-notes__dup">\xB7 existe d\xE9j\xE0, sera compl\xE9t\xE9</span>'}<small>\xAB ${esc(p.excerpt)} \xBB</small></span></label>`;
    const checked = list2.filter((p) => p.checked).length;
    return `<div class="panel__head"><h3 id="notes-proposals-title">${list2.length} proposition${list2.length > 1 ? "s" : ""} extraite${list2.length > 1 ? "s" : ""}</h3><span class="small muted">Les doublons probables sont d\xE9coch\xE9s</span></div><div class="kit-checkbox"><fieldset id="notes-proposals-list">${list2.map(option).join("")}</fieldset></div><div class="kit-button kit-button--bare">` + button({ id: "notes-create", label: createLabel(checked), variant: "primary", writes: CREATE_ENTITIES_FROM_NOTE, disabled: checked === 0 }) + button({ id: "notes-cancel", label: "Tout rejeter", variant: "ghost" }) + "</div>";
  }
  var day = (date) => {
    const parsed = /* @__PURE__ */ new Date(`${date}T12:00:00`);
    return Number.isNaN(parsed.getTime()) ? date : parsed.toLocaleDateString("fr-FR", { day: "numeric", month: "short" });
  };
  var history = (imports) => imports.map((i) => `<li class="item" id="notes-import-${esc(i.id)}"><span>${esc(i.title)}</span><span class="small muted">${esc(day(i.date))} \xB7 ${i.chars} caract\xE8res \xB7 ${i.created}/${i.proposed} cr\xE9\xE9s</span></li>`).join("");
  function status(outcome) {
    switch (outcome.kind) {
      case "none":
        return "";
      case "busy":
        return `<p class="item muted">${esc(outcome.text)}</p>`;
      case "ok":
        return `<p class="item is-ok" id="notes-done">${esc(outcome.text)}</p>`;
      case "error":
        return `<p class="item is-error" id="notes-error">${esc(outcome.text)}</p>`;
    }
  }

  // apps/ai/note-import/src/style.ts
  var STYLE = `
.mf-notes { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-notes *, .mf-notes *::before, .mf-notes *::after { box-sizing: inherit; }
.mf-notes h2, .mf-notes h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-notes h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-notes h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-notes p { margin: 0; }
.mf-notes.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-notes .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-notes .muted { color: var(--color-text-muted); }
.mf-notes .small { font-size: var(--font-size-small); }
.mf-notes .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-notes .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-notes .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-notes .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-notes .item.is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-notes .item.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-notes .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-notes .kit-textarea.is-wide { max-width: none; }
.mf-notes .kit-checkbox fieldset { padding: var(--space-2) var(--space-3); }
.mf-notes .kit-checkbox label span small { display: block; color: var(--color-text-muted); }
.mf-notes__type { font-family: var(--font-family-heading); text-transform: uppercase; font-size: var(--font-size-small); color: var(--color-secondary); margin-right: var(--space-1); }
.mf-notes__dup { color: var(--color-accent); font-size: var(--font-size-small); }
`;

  // apps/ai/note-import/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="NoteImport"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "NoteImport";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var campaignOf = (props2) => typeof props2.campaign === "string" ? props2.campaign : "";
  function mount(host2, props2, shell2, timing = WAITING, dictation) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`NoteImport: #${id} is not on the screen`);
      return found;
    };
    let campaign = campaignOf(props2);
    let notes = { imports: [], proposals: [] };
    let done = /* @__PURE__ */ new Set();
    let shown = [];
    let busy = false;
    const say = (outcome) => {
      part("notes-status").innerHTML = status(outcome);
    };
    const checkedIds = () => Array.from(host2.querySelectorAll('input[name="proposal"]:checked')).map((input) => input.value);
    const count2 = () => {
      const create2 = host2.querySelector("#notes-create");
      if (!create2) return;
      const checked = checkedIds().length;
      create2.textContent = createLabel(checked);
      create2.disabled = busy || checked === 0;
    };
    const paintProposals = () => {
      const box = part("notes-proposals");
      box.hidden = shown.length === 0;
      box.innerHTML = shown.length === 0 ? "" : proposals(shown);
      const cancel = host2.querySelector("#notes-cancel");
      if (cancel) cancel.disabled = busy;
    };
    const paint = () => {
      part("notes-history-list").innerHTML = history(notes.imports);
      shown = pending(notes, done);
      paintProposals();
    };
    const lock = (locked) => {
      busy = locked;
      for (const control of host2.querySelectorAll("#notes-text, #notes-extract, #notes-mic, #notes-cancel")) control.disabled = locked;
      count2();
    };
    const read = async () => parseNotes((await shell2.capability(LIST_IMPORTED_NOTES, void 0, { campaign })).data);
    const refresh = async () => {
      try {
        notes = await read();
        paint();
        return true;
      } catch {
        return false;
      }
    };
    async function send(dataCapability, payload) {
      const answer = await shell2.command(commandBody(dataCapability, payload));
      const id = commandId(answer);
      let state2 = settled(answer);
      for (let asked = 0; !isFinal(state2.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state2 = settled(await shell2.commandResult(id));
      }
      return state2;
    }
    async function act(wrong, run) {
      if (busy) return;
      if (campaign === "") return say({ kind: "error", text: "Aucune campagne choisie." });
      lock(true);
      try {
        await run();
      } catch {
        say({ kind: "error", text: wrong });
      } finally {
        lock(false);
      }
    }
    const extract = () => {
      const field = part("notes-text");
      if (field.value.trim() === "") {
        field.focus();
        return Promise.resolve();
      }
      const text2 = field.value;
      return act("L\u2019extraction a \xE9chou\xE9.", async () => {
        say({ kind: "busy", text: "Extraction en cours\u2026" });
        const state2 = await send(EXTRACT_FROM_NOTES, { campaign, text: text2 });
        if (state2.status === "rejected") return say({ kind: "error", text: `Extraction refus\xE9e : ${reasons(state2.violations)}.` });
        if (state2.status !== "applied") return say({ kind: "error", text: `L\u2019extraction n\u2019a pas abouti (\xE9tat : ${state2.status || "inconnu"}).` });
        done = /* @__PURE__ */ new Set();
        if (!await refresh()) return say({ kind: "error", text: "La note a \xE9t\xE9 analys\xE9e, mais ses propositions n\u2019ont pas pu \xEAtre lues." });
        say(shown.length === 0 ? { kind: "ok", text: "Aucun \xE9l\xE9ment trouv\xE9 dans cette note \xB7 note brute conserv\xE9e." } : { kind: "none" });
      });
    };
    const create = () => {
      const accepted = checkedIds();
      if (accepted.length === 0) return Promise.resolve();
      return act("La cr\xE9ation a \xE9chou\xE9.", async () => {
        say({ kind: "busy", text: "Cr\xE9ation en cours\u2026" });
        const state2 = await send(CREATE_ENTITIES_FROM_NOTE, { campaign, accepted });
        if (state2.status === "rejected") return say({ kind: "error", text: `Cr\xE9ation refus\xE9e : ${reasons(state2.violations)}.` });
        if (state2.status !== "applied") return say({ kind: "error", text: `La cr\xE9ation n\u2019a pas abouti (\xE9tat : ${state2.status || "inconnu"}).` });
        for (const proposal of shown) done.add(proposal.id);
        const read2 = await refresh();
        if (!read2) {
          shown = [];
          paintProposals();
        }
        const n = accepted.length;
        say({ kind: "ok", text: `${n} \xE9l\xE9ment${n > 1 ? "s" : ""} cr\xE9\xE9${n > 1 ? "s" : ""} \xB7 note brute conserv\xE9e.${read2 ? "" : " La liste des imports n\u2019a pas pu \xEAtre relue."}` });
      });
    };
    const reject = () => {
      if (busy) return;
      for (const proposal of shown) done.add(proposal.id);
      shown = [];
      paintProposals();
      say({ kind: "none" });
    };
    const mic = part("notes-mic-box");
    const micState = mic.querySelector(".kit-micbutton__state");
    const micTimer = mic.querySelector(".kit-micbutton__timer");
    let state = "idle";
    let seconds = 0;
    let tick = null;
    let pulse = null;
    const clock = () => `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
    const stopTimers = () => {
      if (tick !== null) clearInterval(tick);
      if (pulse !== null) clearInterval(pulse);
      tick = pulse = null;
      mic.style.setProperty("--kit-level", "0");
    };
    const setMic = (next) => {
      state = next;
      stopTimers();
      if (next === "recording") {
        tick = setInterval(() => {
          if (!mic.isConnected) return stopTimers();
          seconds += 1;
          if (micTimer) micTimer.textContent = clock();
        }, 1e3);
        pulse = setInterval(() => mic.style.setProperty("--kit-level", (Math.random() * 0.9 + 0.1).toFixed(2)), 140);
      }
      mic.setAttribute("data-state", next);
      if (micState) micState.textContent = MIC_TEXTS[next];
      if (micTimer) micTimer.textContent = clock();
      const button2 = part("notes-mic");
      button2.setAttribute("aria-pressed", String(next === "recording"));
      button2.setAttribute("aria-label", `Dicter \u2014 ${MIC_TEXTS[next]}`);
    };
    const dictate = () => {
      if (state !== "recording") return setMic("recording");
      setMic("paused");
      const heard = dictation?.transcript() ?? "";
      if (heard === "") return;
      const field = part("notes-text");
      field.value = field.value === "" ? heard : `${field.value}
${heard}`;
    };
    host2.addEventListener("change", (event) => {
      if (event.target?.name === "proposal") count2();
    });
    host2.addEventListener("click", (event) => {
      const button2 = event.target?.closest("button");
      if (!button2) return;
      if (button2.dataset.writes === EXTRACT_FROM_NOTES) void extract();
      else if (button2.dataset.writes === CREATE_ENTITIES_FROM_NOTE) void create();
      else if (button2.id === "notes-cancel") reject();
      else if (button2.id === "notes-mic") dictate();
    });
    setMic("idle");
    const load = () => {
      if (campaign === "") return;
      read().then(
        (loaded) => {
          notes = loaded;
          paint();
        },
        () => say({ kind: "error", text: "Impossible de lire les notes import\xE9es." })
      );
    };
    load();
    return {
      update(next) {
        const chosen = campaignOf(next);
        if (chosen === campaign) return;
        campaign = chosen;
        done = /* @__PURE__ */ new Set();
        notes = { imports: [], proposals: [] };
        paint();
        say({ kind: "none" });
        load();
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
      const { proposals: proposals2 } = await server.load("NoteImport");
      server.noteText = text2;
      server.noteProposals = proposals2.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
      return void 0;
    },
    // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
    // one element per accepted proposal, the raw note kept as an import listed
    // newest first, the proposals no longer awaiting.
    "campaign.createEntitiesFromNote": async (server, { payload }) => {
      const proposals2 = server.noteProposals || [];
      const accepted = Array.isArray(payload.accepted) ? payload.accepted : [];
      const created = proposals2.filter((p) => accepted.includes(p.id));
      const fixture = await server.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture.imports;
      const note = server.noteText || "";
      imports.unshift({ id: `imp-${server.uuid()}`, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), title: note.split("\n")[0].replace(/[.\s]+$/, "") || "Notes import\xE9es", chars: note.length, proposed: proposals2.length, created: created.length });
      server.noteProposals = [];
      return void 0;
    }
  };

  // mock/src/reads.js
  var READS = {
    // campaign.listImportedNotes (crates/campaign/capabilities/list-imported-notes):
    // the notes already imported, newest first, and the proposals of the latest
    // analysed note still awaiting validation. The imports are the fixture's
    // list; the proposals are what `ai.extractFromNotes` recorded (commands.js).
    "campaign.listImportedNotes": async (server) => {
      const fixture = await server.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture && fixture.imports || [];
      return { imports: [...imports].sort((x, y) => String(y.date).localeCompare(String(x.date))), proposals: server.noteProposals || [] };
    }
  };

  // mock/src/server.js
  var LATENCY_MS = 120;
  var delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  var uuid = () => crypto.randomUUID ? crypto.randomUUID() : "xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx".replace(/x/g, () => Math.floor(Math.random() * 16).toString(16));
  var json = (body, status2 = 200) => new Response(JSON.stringify(body), { status: status2, headers: { "content-type": "application/json" } });
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

  // mock/src/NoteImport.js
  var HEARD = "Dict\xE9 : M\xE8re Ombeline garde un herbier de plantes qui ne poussent qu\u2019au pied du mont Pourpre.";
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  mockServer().load("NoteImport").then((fixture) => {
    compose({ campaign: fixture.campaign, ...props }, (resolved) => {
      const mounted = mount(host, resolved, shell, void 0, { transcript: () => HEARD });
      host.querySelector("#notes-text").value = fixture.draft;
      return mounted;
    });
  });
})();
