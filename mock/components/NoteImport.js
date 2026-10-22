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
  var normalized = (term) => String(term ?? "").trim().toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "").normalize("NFC");
  var LISTS = { pj: "campaign.listCharacters", pnj: "campaign.listCharacters", lieu: "campaign.listPlaces", evenement: "campaign.listEvents" };
  var SAVED = ["kind", "name", "aliases", "description", "notes", "player", "prompt", "background", "behavior", "stats", "ca", "pvMax", "spells", "features", "actions", "portrait", "token", "crop", "color"];
  var COMMANDS = {
    // session.addPresentCharacter / session.removePresentCharacter (the Session
    // aggregate): a PC of the session's campaign ticked present or unticked.
    // `present-only-pj`: a character that is not a PC of the campaign is refused.
    "session.addPresentCharacter": async (server, { payload }) => {
      const session = await server.load("session.getSession");
      const character = (await server.load("campaign.listCharacters")).find((c) => c.id === payload.character);
      if (!character || character.kind !== "pj") return ["present-only-pj"];
      if (!session.present.includes(character.id)) session.present.push(character.id);
      return void 0;
    },
    "session.removePresentCharacter": async (server, { payload }) => {
      const session = await server.load("session.getSession");
      session.present = session.present.filter((id) => id !== payload.character);
      return void 0;
    },
    // session.addScene: a scene at the end, its rank the next one.
    "session.addScene": async (server, { payload }) => {
      const scenes = await server.load("session.listScenes");
      const next = Math.max(0, ...scenes.map((sc) => sc.order)) + 1;
      scenes.push({ id: `sc-${server.uuid().slice(0, 8)}`, order: next, title: String(payload.title || "Nouvelle sc\xE8ne"), places: [], characters: [], events: [], media: [], toCreate: [], battlemap: null, notes: "" });
      return void 0;
    },
    // session.reorderScenes: every rank rewritten at once. `scene-order-contiguous`:
    // the ranks must be 1 to n, each scene once.
    "session.reorderScenes": async (server, { payload }) => {
      const scenes = await server.load("session.listScenes");
      const order = Array.isArray(payload.order) ? payload.order : [];
      const ranks = order.map((o) => o.rank).sort((a, b) => a - b);
      const known = new Set(scenes.map((sc) => sc.id));
      if (order.length !== scenes.length || ranks.some((rank, i) => rank !== i + 1) || order.some((o) => !known.has(o.scene)) || new Set(order.map((o) => o.scene)).size !== scenes.length) return ["scene-order-contiguous"];
      for (const o of order) scenes.find((sc) => sc.id === o.scene).order = o.rank;
      scenes.sort((a, b) => a.order - b.order);
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
      const media = await server.load("media");
      const base = { id: server.uuid(), title: String(payload.title || "").trim() || "Sans titre", file: payload.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      if (kind === "battlemap") media.battlemaps.push({ ...base, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 } });
      else media.illustrations.push(base);
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
      const history2 = (await server.load("campaign.listEntityVersions"))[targetId];
      const target = history2 && history2.versions.find((v) => v.v === payload.version);
      if (!target) return ["not-found"];
      const sheet = (upTo) => history2.versions.slice().sort((a, b) => a.v - b.v).filter((v) => v.v <= upTo).reduce((fields, v) => {
        for (const c of v.changes) fields[c.field] = c.new;
        return fields;
      }, {});
      const before = sheet(history2.versions[0].v);
      let after = sheet(target.v);
      if (Array.isArray(payload.changes)) {
        const undone = (history2.versions.find((v) => v.v === payload.version + 1) || { changes: [] }).changes;
        after = { ...before };
        for (const i of payload.changes) if (undone[i]) after[undone[i].field] = undone[i].old;
      }
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].sort().filter((field) => (before[field] ?? "") !== (after[field] ?? "")).map((field) => ({ field, old: before[field] ?? "", new: after[field] ?? "" }));
      history2.versions.unshift({ v: history2.versions[0].v + 1, date: (/* @__PURE__ */ new Date()).toISOString(), origin: "manuelle", source: "restauration", restoredFrom: target.v, changes });
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
      const history2 = await server.load("campaign.listEntityVersions") || {};
      const entry = history2[id] || (history2[id] = { name: after.name, versions: [] });
      entry.name = after.name;
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => f !== "id").sort().filter((f) => JSON.stringify(before[f] ?? null) !== JSON.stringify(after[f] ?? null)).map((field) => ({ field, old: before[field] ?? null, new: after[field] ?? null }));
      const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
      entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin: "dm", source: "Fiche enregistr\xE9e par le DM", seen: true, changes });
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
    }
  };

  // mock/src/reads.js
  var READS = {
    // table.getBattlemapInstance: the fixture's instance, or one a command made.
    "table.getBattlemapInstance": async (server, { instance }) => {
      const fixture = await server.load("table.getBattlemapInstance");
      if (!instance || instance === fixture.id) return fixture;
      return server.instances && server.instances.get(instance);
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
    // One element's history, newest first: the version list of `entity` in the fixture; an element the fixture
    // does not know but `entities` does has never been saved (an empty history), any other has no answer (null).
    "campaign.listEntityVersions": async (server, { entity }) => {
      const history2 = (await server.load("campaign.listEntityVersions"))[entity];
      if (history2) return history2;
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
