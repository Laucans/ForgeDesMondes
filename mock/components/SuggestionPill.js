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
      if (this.held.has(name)) {
        if (this.values.get(name) !== value) console.warn(`shell: "${name}" is held by the address, a write of it was ignored`);
        return;
      }
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
  var SETTLING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 100, polls: 50 };
  var ADVANCING = /* @__PURE__ */ new Set(["queued", "confirmed"]);
  var record = (value) => value && typeof value === "object" && !Array.isArray(value) ? value : {};
  var stateOf = (answer) => {
    const result = record(record(answer).result);
    const id = record(answer).commandId ?? result.commandId;
    return { id: typeof id === "string" ? id : "", status: typeof result.status === "string" ? result.status : "" };
  };
  var FAIL_CLOSED = { by: "player", roles: [], permissions: [] };
  var browserPage = () => ({
    href: typeof location === "undefined" ? "" : location.href,
    meta: (name) => typeof document === "undefined" ? void 0 : document.querySelector(`meta[name="${name}"]`)?.getAttribute("content") ?? void 0,
    fetch: (input, init) => fetch(input, init)
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
      try {
        const screen = await product(page, `apps/screens/${mocked.screen}.json`);
        const pilot = screen.pilot;
        surface = pilot && typeof pilot.query === "string" && here.searchParams.get(pilot.query) === "1" ? pilot.surface : screen.surface;
      } catch (error) {
        if (surface === void 0) throw error;
      }
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
    const from = sent && typeof sent === "object" && !Array.isArray(sent) ? sent.from : void 0;
    return { ...body, caller: { ...from === void 0 ? {} : { from }, by: caller.by, roles: caller.roles, permissions: caller.permissions } };
  }
  function createShell(fetcher = (input, init) => fetch(input, init), page = browserPage(), settling = SETTLING) {
    const writing = /* @__PURE__ */ new Set();
    const follow = async (sent) => {
      try {
        const first = stateOf(await sent);
        const { id } = first;
        let { status } = first;
        for (let asked = 0; ADVANCING.has(status) && id !== "" && asked < settling.polls; asked += 1) {
          await settling.sleep(settling.everyMs);
          ({ status } = stateOf(await call(fetcher, "GET", `/commands/${encodeURIComponent(id)}`)));
        }
      } catch {
      }
    };
    const track = (sent) => {
      const settled2 = follow(sent).finally(() => writing.delete(settled2));
      writing.add(settled2);
    };
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
      capability: async (id, sha256, variables = {}) => {
        await Promise.all([...writing]);
        return await call(fetcher, "POST", `/capabilities/${encodeURIComponent(id)}`, { sha256, variables });
      },
      command: (body) => {
        const sent = (async () => call(fetcher, "POST", "/commands", withCaller(body, await callerOf())))();
        track(sent);
        return sent;
      },
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

  // apps/live/suggestion-pill/src/ids.ts
  var LIST_SUGGESTIONS = "live.listSuggestions";
  var SET_MICROPHONE = "live.setMicrophone";
  var PLAY_SOUND = "soundbox.playSound";
  var DATA_CAPABILITY_VERSION = 1;
  var MICRO_UI = "SuggestionPill";
  var BUS_OPEN = "fdm:open";
  var SOUND_SOURCE = "suggestion";

  // apps/live/suggestion-pill/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var PILE = 5;
  function parseProps(raw) {
    return { campaign: text(raw.campaign) };
  }
  function parseSuggestion(value) {
    if (!isRecord(value)) return null;
    const id = text(value.id);
    if (id === "") return null;
    const kind = value.kind === "sound" ? "sound" : "entity";
    return { id, kind, type: kind === "sound" ? "" : text(value.type), name: text(value.name), via: text(value.via), detectedAt: text(value.detectedAt) };
  }
  var list = (value) => Array.isArray(value) ? value.map(parseSuggestion).filter((s) => s !== null) : [];
  function parseSuggestions(data) {
    const answer = isRecord(data) ? data : {};
    const all = Array.isArray(data) ? list(data) : list(answer.history).length > 0 ? list(answer.history) : list(answer.suggestions);
    const times = all.map((s) => Date.parse(s.detectedAt));
    const history = times.every((t) => !Number.isNaN(t)) ? all.map((s, i) => ({ s, t: times[i] ?? 0 })).sort((a, b) => b.t - a.t).map(({ s }) => s) : all;
    return { stack: history.slice(0, PILE), history };
  }
  var keyOf = (s) => `${s.id}@${s.detectedAt}`;
  var arrivals = (history, seen) => history.filter((s) => !seen.has(keyOf(s))).reverse();
  var isAnswer = (data) => isRecord(data) || Array.isArray(data);
  function without(all, held) {
    if (held.length === 0) return all;
    const keys = new Set(held.map(keyOf));
    const history = all.history.filter((s) => !keys.has(keyOf(s)));
    return { stack: history.slice(0, PILE), history };
  }
  function timeOf(detectedAt) {
    const at = Date.parse(detectedAt);
    return Number.isNaN(at) ? "" : new Date(at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
  }
  function hostType(type) {
    const folded = type.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
    if (folded === "lieu" || folded === "place") return "place";
    if (folded === "evenement" || folded === "event") return "event";
    return "character";
  }
  function commandBody(dataCapability, payload, targetId) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload,
      ...targetId === void 0 ? {} : { target: { id: targetId } },
      caller: { by: "dm", from: MICRO_UI, roles: [] }
    };
  }
  var VIOLATIONS = {
    "status-lifecycle": "la s\xE9ance est termin\xE9e",
    "dm-side-only": "r\xE9serv\xE9 \xE0 la vue du MJ",
    "not-found": "introuvable",
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

  // apps/live/suggestion-pill/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var MIC = '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
  var PAUSE = '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';
  var MIC_TEXTS = { idle: "\xC9teint \xB7 transcription locale", recording: "\xC9coute en local", paused: "En pause" };
  var MIC_LABEL = "Micro de s\xE9ance";
  var ICON = { lieu: "L", place: "L", pnj: "P", npc: "P", pj: "J", "\xE9v\xE9nement": "\xC9", evenement: "\xC9", event: "\xC9" };
  function skeleton() {
    return `<aside class="mf-pill" id="suggestions" aria-labelledby="suggestions-title"><div class="mf-pill__card" id="suggestions-card"><div class="mf-pill__head"><h2 id="suggestions-title">Suggestions</h2><button type="button" class="icon-btn" id="suggestions-history-toggle" aria-expanded="false" aria-controls="suggestions-history" aria-label="Historique des 5 derni\xE8res entit\xE9s" title="Historique des 5 derni\xE8res entit\xE9s">\u27F2</button></div><p class="mf-pill__heard small" id="suggestions-heard" aria-live="polite"></p><p class="mf-pill__status small" id="suggestions-status" role="status" hidden></p><ol class="mf-pill__stack" id="suggestions-stack" data-reads="${LIST_SUGGESTIONS}"></ol><section class="mf-pill__history" id="suggestions-history" aria-labelledby="suggestions-history-title" hidden><h3 id="suggestions-history-title">Historique des 5 derni\xE8res entit\xE9s</h3><ol class="mf-pill__stack" id="suggestions-history-list" data-reads="${LIST_SUGGESTIONS}"></ol></section></div><div class="mf-pill__mic" id="suggestions-mic" data-writes="${SET_MICROPHONE}"><div class="kit-micbutton kit-micbutton--compact" id="suggestions-mic-box" data-state="idle"><button type="button" class="kit-micbutton__btn" id="suggestions-mic-button" aria-pressed="false" aria-label="${MIC_LABEL}"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button><div class="kit-micbutton__text"><span class="kit-micbutton__label">${MIC_LABEL}</span><span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span></span></div></div></div></aside>`;
  }
  function row(s, index, prefix, armed) {
    const sound = s.kind === "sound";
    const at = timeOf(s.detectedAt);
    const icon = sound ? "\u266A" : ICON[s.type.toLowerCase()] ?? "\u2022";
    const hint = sound ? armed ? "Son \xB7 cliquer encore pour jouer" : "Son \xB7 cliquer deux fois pour jouer" : esc(s.type);
    const attrs = sound ? ` data-writes="${PLAY_SOUND}" aria-pressed="${armed}"` : "";
    return `<li><button type="button" id="${prefix}-${index}" class="${armed ? "is-armed" : ""}" data-kind="${s.kind}"${attrs} data-type="${esc(s.type)}" data-id="${esc(s.id)}"><span class="mf-pill__icon${sound ? " is-sound" : ""}" aria-hidden="true">${icon}</span><span><b>${esc(s.name)}</b><small>${hint} \xB7 \xAB ${esc(s.via)} \xBB${at === "" ? "" : ` \xB7 ${at}`}</small></span></button></li>`;
  }
  function rows(list2, prefix, empty, armed) {
    return list2.length === 0 ? `<li class="mf-pill__empty">${esc(empty)}</li>` : list2.map((s, i) => row(s, i, prefix, s.kind === "sound" && s.id === armed)).join("");
  }

  // apps/live/suggestion-pill/src/style.ts
  var STYLE = `
.mf-pill { position: fixed; right: var(--space-3); bottom: var(--space-3); left: auto; top: auto; z-index: 1000; display: grid; gap: var(--space-2); justify-items: end; width: min(calc(100vw - var(--space-4)), calc(var(--space-5) * 8)); max-height: calc(100vh - var(--space-4)); margin: 0; box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-pill *, .mf-pill *::before, .mf-pill *::after { box-sizing: inherit; }
.mf-pill h2, .mf-pill h3, .mf-pill p { margin: 0; }
.mf-pill h2, .mf-pill h3 { font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-pill .small { font-size: var(--font-size-small); }
.mf-pill .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-pill .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-pill .kit-micbutton--compact .kit-micbutton__btn { width: calc(var(--space-5) + var(--space-2)); height: calc(var(--space-5) + var(--space-2)); }
.mf-pill .kit-micbutton--compact .kit-micbutton__label { font-size: var(--font-size-base); }
.mf-pill__card { width: 100%; display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-secondary); border-radius: var(--radius-large); box-shadow: var(--shadow-high); overflow-y: auto; min-height: 0; }
.mf-pill__head { display: flex; align-items: center; justify-content: space-between; }
.mf-pill__head h2 { font-size: var(--font-size-base); color: var(--color-accent); }
.mf-pill__heard { color: var(--color-text-muted); font-style: italic; min-height: 1.5em; }
.mf-pill__status { color: var(--color-danger); }
.mf-pill__stack { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); }
.mf-pill__stack button { width: 100%; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-2); align-items: center; text-align: left; padding: var(--space-2) var(--space-3); background: var(--color-surface-2); color: var(--color-text); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); font: inherit; cursor: pointer; }
.mf-pill__stack button:hover { border-color: var(--color-primary); }
.mf-pill__stack button.is-armed { border-color: var(--color-success); }
.mf-pill__stack li:first-child button { border-color: var(--color-accent); }
.mf-pill__stack b { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-pill__stack small { display: block; color: var(--color-text-muted); }
.mf-pill__icon { display: grid; place-items: center; width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); border-radius: var(--radius-pill); background: var(--color-secondary); color: var(--color-text); font-family: var(--font-family-heading); }
.mf-pill__icon.is-sound { background: var(--color-success); color: var(--color-primary-contrast); }
.mf-pill__empty { color: var(--color-text-muted); font-size: var(--font-size-small); }
.mf-pill__history { border-top: var(--border-width) solid var(--color-border); padding-top: var(--space-2); display: grid; gap: var(--space-1); }
.mf-pill__history h3 { font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-pill__mic { padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); box-shadow: var(--shadow-high); }
`;

  // apps/live/suggestion-pill/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 1e3, beatMs: 600 };
  var EMPTY_STACK = "Activez le micro : les entit\xE9s et les sons nomm\xE9s appara\xEEtront ici.";
  var EMPTY_HISTORY = "Aucune suggestion pour l\u2019instant.";
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="SuggestionPill"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "SuggestionPill";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, raw, shell2, timing = WAITING) {
    const doc = host2.ownerDocument;
    ensureStyle(doc);
    host2.innerHTML = skeleton();
    let props2 = parseProps(raw);
    let suggestions = parseSuggestions(void 0);
    let seen = /* @__PURE__ */ new Set();
    let seeded = false;
    let pending = [];
    let revealing = false;
    const forgetArrivals = () => {
      seen = /* @__PURE__ */ new Set();
      seeded = false;
      pending = [];
      revealing = false;
    };
    let mic = "idle";
    let busy = false;
    let armed = null;
    let generation = 0;
    let readNo = 0;
    let stopWatching = () => void 0;
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`SuggestionPill: #${id} is not on the screen`);
      return found;
    };
    const scope = () => props2.campaign === "" ? {} : { campaign: props2.campaign };
    let speaker = "";
    function say(text2, from) {
      if (text2 === "" && speaker !== from) return;
      const box = part("suggestions-status");
      box.textContent = text2;
      box.hidden = text2 === "";
      speaker = text2 === "" ? "" : from;
    }
    const shown2 = () => without(suggestions, pending);
    function paintHeard() {
      const newest = shown2().stack[0];
      part("suggestions-heard").textContent = mic === "recording" ? newest ? `\xAB ${newest.via} \xBB` : "J\u2019\xE9coute\u2026" : mic === "paused" ? "Micro en pause \u2014 rien n\u2019est \xE9cout\xE9." : "";
    }
    function paintLists() {
      const active = doc.activeElement;
      const on = active !== null && host2.contains(active) ? active.closest("button[data-kind]") : null;
      const focused = on === null ? null : { list: on.closest("ol")?.id ?? "", kind: on.getAttribute("data-kind") ?? "", id: on.getAttribute("data-id") ?? "" };
      const view = shown2();
      part("suggestions-stack").innerHTML = rows(view.stack, "suggestion", EMPTY_STACK, armed);
      part("suggestions-history-list").innerHTML = rows(view.history, "suggestion-history", EMPTY_HISTORY, armed);
      if (focused !== null) {
        const buttons = [...host2.querySelectorAll(`#${focused.list} button[data-kind]`)];
        buttons.find((r) => r.dataset.kind === focused.kind && r.dataset.id === focused.id)?.focus();
      }
      paintHeard();
    }
    function paintMic() {
      const box = part("suggestions-mic-box");
      box.setAttribute("data-state", mic);
      const words = box.querySelector(".kit-micbutton__state");
      if (words) words.textContent = MIC_TEXTS[mic];
      const button = part("suggestions-mic-button");
      button.setAttribute("aria-pressed", String(mic === "recording"));
      button.setAttribute("aria-label", `${MIC_LABEL} \u2014 ${MIC_TEXTS[mic]}`);
      button.disabled = busy;
      paintHeard();
    }
    async function reveal() {
      if (revealing) return;
      revealing = true;
      const mine = generation;
      try {
        while (pending.length > 0 && mine === generation && host2.isConnected) {
          pending = pending.slice(1);
          paintLists();
          await timing.sleep(timing.beatMs);
        }
      } finally {
        if (mine === generation) revealing = false;
      }
    }
    async function refresh() {
      const mine = generation;
      const no = readNo += 1;
      try {
        const read = await shell2.capability(LIST_SUGGESTIONS, void 0, scope());
        if (mine !== generation || no < readNo) return;
        suggestions = parseSuggestions(read.data);
        const listed = new Set(suggestions.history.map(keyOf));
        pending = pending.filter((s) => listed.has(keyOf(s)));
        if (seeded) pending = [...pending, ...arrivals(suggestions.history, seen)];
        seeded = seeded || isAnswer(read.data);
        for (const key of listed) seen.add(key);
        say("", "read");
        if (armed !== null && !suggestions.history.some((s) => s.kind === "sound" && s.id === armed)) armed = null;
        paintLists();
        void reveal();
      } catch {
        if (mine !== generation || no < readNo) return;
        say("Impossible de lire les suggestions.", "read");
      }
    }
    function watch() {
      stopWatching();
      stopWatching = shell2.onDataVersion(() => {
        if (host2.isConnected) void refresh();
        else stopWatching();
      }, timing.watchMs);
    }
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
    const why = (state) => state.violations.length > 0 ? reasons(state.violations) : isFinal(state.status) ? state.status : `\xE9tat : ${state.status || "inconnu"}`;
    async function toggleMic() {
      if (busy) return;
      const before = mic;
      const on = before !== "recording";
      busy = true;
      mic = on ? "recording" : "paused";
      paintMic();
      try {
        const state = await send(SET_MICROPHONE, { on });
        if (isFinal(state.status) && state.status !== "applied") {
          mic = before;
          say(`Le micro n\u2019a pas chang\xE9 : ${why(state)}.`, "mic");
        } else {
          say(state.status === "applied" ? "" : `Le micro : le serveur ne l\u2019a pas encore pris en compte (${why(state)}).`, "mic");
          if (on) watch();
          else stopWatching();
        }
      } catch {
        mic = before;
        say("La commande du micro n\u2019a pas pu \xEAtre envoy\xE9e.", "mic");
      } finally {
        busy = false;
        paintMic();
      }
      await refresh();
    }
    async function play(id, name) {
      try {
        const state = await send(PLAY_SOUND, { source: SOUND_SOURCE }, id);
        say(state.status === "applied" ? "" : `\xAB ${name} \xBB n\u2019a pas pu \xEAtre jou\xE9 : ${why(state)}.`, "sound");
      } catch {
        say(`\xAB ${name} \xBB n\u2019a pas pu \xEAtre jou\xE9.`, "sound");
      }
    }
    host2.addEventListener("click", (event) => {
      const target = event.target;
      if (target?.closest("#suggestions-history-toggle")) {
        const history = part("suggestions-history");
        history.hidden = !history.hidden;
        part("suggestions-history-toggle").setAttribute("aria-expanded", String(!history.hidden));
        return;
      }
      if (target?.closest("#suggestions-mic-button")) {
        void toggleMic();
        return;
      }
      const row2 = target?.closest("button[data-kind]");
      if (!row2) return;
      const id = row2.dataset.id ?? "";
      if (row2.dataset.kind === "sound") {
        if (armed === id) {
          armed = null;
          paintLists();
          void play(id, row2.querySelector("b")?.textContent ?? "");
        } else {
          armed = id;
          paintLists();
        }
        return;
      }
      armed = null;
      paintLists();
      const Event = doc.defaultView?.CustomEvent ?? CustomEvent;
      doc.dispatchEvent(new Event(BUS_OPEN, { detail: { type: hostType(row2.dataset.type ?? ""), id } }));
    });
    paintMic();
    paintLists();
    void refresh();
    return {
      update(next) {
        const before = props2;
        props2 = parseProps(next);
        if (props2.campaign === before.campaign) return;
        generation += 1;
        suggestions = parseSuggestions(void 0);
        forgetArrivals();
        armed = null;
        stopWatching();
        mic = "idle";
        say("", "mic");
        paintMic();
        paintLists();
        void refresh();
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

  // mock/src/SuggestionPill.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
