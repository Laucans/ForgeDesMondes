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

  // apps/ai/oral-brief/src/ids.ts
  var GET_LAST_BRIEF = "session.getLastBrief";
  var ANALYZE_BRIEF = "ai.analyzeBrief";
  var APPROVER = "role:approver";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/ai/oral-brief/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var count = (value) => typeof value === "number" && Number.isFinite(value) ? value : 0;
  var list = (value, read) => Array.isArray(value) ? value.map(read).filter((item) => item !== null) : [];
  function readScene(value) {
    if (!isRecord(value) || text(value.title) === "") return null;
    return { order: count(value.order), title: text(value.title), links: list(value.links, (link) => typeof link === "string" && link !== "" ? link : null) };
  }
  function readRecognized(value) {
    if (!isRecord(value) || text(value.id) === "") return null;
    return { id: text(value.id), type: text(value.type), name: text(value.name), heard: text(value.heard) || null, updated: value.updated === true, change: text(value.change) };
  }
  function readMissing(value) {
    if (!isRecord(value) || text(value.id) === "") return null;
    return { id: text(value.id), type: text(value.type), name: text(value.name) };
  }
  function parseBrief(data) {
    const answer = isRecord(data) ? data : {};
    const result2 = isRecord(answer.result) ? answer.result : {};
    return {
      lastRun: text(answer.lastRun) || null,
      duration: text(answer.duration) || null,
      transcript: list(answer.transcript, (line) => typeof line === "string" && line !== "" ? line : null),
      // The scenes are shown in their order, whatever order the answer lists them in.
      scenes: list(result2.scenes, readScene).sort((a, b) => a.order - b.order),
      recognized: list(result2.recognized, readRecognized),
      missing: list(result2.missing, readMissing)
    };
  }
  function commandBody(dataCapability, payload) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload, caller: { by: "dm", from: "OralBrief", roles: [APPROVER] } };
  }
  var VIOLATIONS = {
    "brief-no-auto-creation": "le brief ne cr\xE9e aucun \xE9l\xE9ment",
    "text-only-no-audio": "seul du texte est accept\xE9",
    "scene-order-contiguous": "les sc\xE8nes doivent rester num\xE9rot\xE9es sans trou",
    "scene-links-same-campaign": "un \xE9l\xE9ment li\xE9 doit \xEAtre de la campagne de la s\xE9ance"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result2 = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    const violations = Array.isArray(result2.violations) ? result2.violations.filter((v) => typeof v === "string") : [];
    return { status: text(result2.status), violations };
  }
  var isFinal = (status2) => FINAL.has(status2);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }
  function lastRunLabel(lastRun, duration) {
    if (lastRun === null) return "";
    const parsed = new Date(/(Z|[+-]\d\d:?\d\d)$/.test(lastRun) ? lastRun : `${lastRun}Z`);
    const when = Number.isNaN(parsed.getTime()) ? lastRun : parsed.toLocaleString("fr-FR", { day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" });
    return `Dernier brief : ${when}${duration === null ? "" : ` \xB7 ${duration}`}`;
  }

  // apps/ai/oral-brief/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var MIC = '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
  var PAUSE = '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';
  var MIC_TEXTS = { idle: "Pr\xEAt \xE0 \xE9couter", recording: "J\u2019\xE9coute le brief", paused: "En pause \u2014 reprenez ou analysez" };
  function button(p) {
    const writes = p.writes === void 0 ? "" : ` data-writes="${p.writes}"`;
    const diff = p.diff === void 0 ? "" : ` data-diff="${esc(p.diff)}"`;
    return `<button type="button" class="${p.variant}" id="${esc(p.id)}"${writes}${diff}${p.disabled === true ? " disabled" : ""}>${esc(p.label)}</button>`;
  }
  var badge = (label, tone) => `<span class="kit-badge"><span${tone === "" ? "" : ` class="${tone}"`}>${esc(label)}</span></span>`;
  function skeleton() {
    return `<section class="panel mf-brief" id="oral-brief" aria-labelledby="brief-title"><div class="panel__head"><h2 id="brief-title">Brief oral</h2><span class="small muted" id="brief-last" data-reads="${GET_LAST_BRIEF}"></span></div><p class="small muted">Racontez la s\xE9ance au micro, ou tapez-la. L\u2019IA en tire les sc\xE8nes, reconna\xEEt les PNJ, lieux et \xE9v\xE9nements existants et place les manquants dans la liste \xAB \xE0 cr\xE9er \xBB.</p><div id="brief-input"><div class="kit-textarea is-wide"><label class="kit-textarea__label" for="brief-text">Brief de la s\xE9ance</label><textarea class="kit-textarea__field" id="brief-text" rows="5"></textarea><div class="kit-textarea__footer"><p class="kit-textarea__hint">Seul le texte part vers l\u2019IA, jamais le son.</p></div></div></div><div class="row" id="brief-controls"><div class="kit-micbutton" id="brief-mic-box" data-state="idle"><button type="button" class="kit-micbutton__btn" id="brief-mic" aria-pressed="false" aria-label="Brief oral"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button><div class="kit-micbutton__text"><span class="kit-micbutton__label">Brief oral</span><span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span> <span class="kit-micbutton__timer">00:00</span></span></div></div><div class="kit-button kit-button--bare">${button({ id: "brief-analyse", label: "Terminer et analyser", variant: "primary", writes: ANALYZE_BRIEF, disabled: true })}</div></div><div class="mf-brief__status" id="brief-status" aria-live="polite"></div><div class="mf-brief__result" id="brief-result" data-reads="${GET_LAST_BRIEF}"></div></section>`;
  }
  var transcript = (lines, duration) => `<article class="kit-card" id="brief-transcript-card"><div class="body"><h3>Transcription${duration === null ? "" : ` \xB7 ${esc(duration)}`}</h3><ol class="mf-brief__lines" id="brief-transcript">${lines.map((line) => `<li>${esc(line)}</li>`).join("")}</ol></div></article>`;
  var scenes = (list2) => `<section class="mf-brief__col" id="brief-scenes"><h3>${list2.length} sc\xE8ne${list2.length > 1 ? "s" : ""} cr\xE9\xE9e${list2.length > 1 ? "s" : ""}</h3><ol>` + list2.map((s) => `<li><b>${esc(s.title)}</b><br><small>${esc(s.links.join(" \xB7 "))}</small></li>`).join("") + "</ol></section>";
  var known = (x) => `<li id="brief-known-${esc(x.id)}"><a href="fiche-entite.html?id=${esc(x.id)}">${esc(x.name)}</a> <small>${esc(x.type)}${x.heard === null ? "" : ` \xB7 entendu : \xAB ${esc(x.heard)} \xBB`}</small> ${badge("Existe", "success")}` + (x.updated ? `<br><span class="mf-brief__upd">${badge("Mis \xE0 jour", "accent")}<span class="kit-button kit-button--bare">${button({ id: `brief-diff-${x.id}`, label: "Voir le diff", variant: "ghost", diff: x.id })}</span></span>${x.change === "" ? "" : `<br><small>${esc(x.change)}</small>`}` : "") + "</li>";
  var recognized = (list2) => `<section class="mf-brief__col" id="brief-recognized"><h3>${list2.length} entit\xE9${list2.length > 1 ? "s" : ""} reconnue${list2.length > 1 ? "s" : ""}</h3><ul>${list2.map(known).join("")}</ul></section>`;
  var absent = (x) => `<li id="brief-missing-${esc(x.id)}"><a href="#to-create-${esc(x.id)}">${esc(x.name)}</a> <small>${esc(x.type)}</small> ${badge("\xE0 cr\xE9er", "")}</li>`;
  var missing = (list2) => `<section class="mf-brief__col" id="brief-missing"><h3>${list2.length} manquante${list2.length > 1 ? "s" : ""} \u2192 \xAB \xE0 cr\xE9er \xBB</h3><ul>${list2.map(absent).join("")}</ul></section>`;
  function result(brief) {
    if (brief.lastRun === null) return '<p class="small muted" id="brief-none">Aucun brief analys\xE9 pour cette s\xE9ance.</p>';
    return `${transcript(brief.transcript, brief.duration)}<div class="mf-brief__cols">${scenes(brief.scenes)}${recognized(brief.recognized)}${missing(brief.missing)}</div>`;
  }
  function status(outcome) {
    switch (outcome.kind) {
      case "none":
        return "";
      case "busy":
        return `<p class="item muted">${esc(outcome.text)}</p>`;
      case "ok":
        return `<p class="item is-ok" id="brief-done">${esc(outcome.text)}</p>`;
      case "error":
        return `<p class="item is-error" id="brief-error">${esc(outcome.text)}</p>`;
    }
  }

  // apps/ai/oral-brief/src/style.ts
  var STYLE = `
.mf-brief { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-brief *, .mf-brief *::before, .mf-brief *::after { box-sizing: inherit; }
.mf-brief h2, .mf-brief h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-brief h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-brief h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-brief p { margin: 0; }
.mf-brief.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-brief .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-brief .muted { color: var(--color-text-muted); }
.mf-brief .small { font-size: var(--font-size-small); }
.mf-brief .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-brief .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-brief .item.is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-brief .item.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-brief .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-brief .kit-textarea.is-wide { max-width: none; }
.mf-brief .kit-card { max-width: none; }
.mf-brief .kit-badge { display: inline-flex; }
.mf-brief__lines { margin: 0; padding-left: var(--space-4); display: grid; gap: var(--space-1); max-height: calc(var(--space-5) * 5); overflow-y: auto; }
.mf-brief__result { display: grid; gap: var(--space-3); }
.mf-brief__cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(calc(var(--space-5) * 6), 1fr)); gap: var(--space-3); }
.mf-brief__col { display: grid; gap: var(--space-2); align-content: start; padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-brief__col h3 { font-size: var(--font-size-small); color: var(--color-accent); }
.mf-brief__col ol, .mf-brief__col ul { margin: 0; padding-left: var(--space-4); display: grid; gap: var(--space-1); }
.mf-brief__col small { color: var(--color-text-muted); }
.mf-brief__upd { display: inline-flex; gap: var(--space-2); align-items: center; }
`;

  // apps/ai/oral-brief/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  var NO_BRIEF = { lastRun: null, duration: null, transcript: [], scenes: [], recognized: [], missing: [] };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="OralBrief"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "OralBrief";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var sessionOf = (props2) => typeof props2.session === "string" ? props2.session : "";
  function mount(host2, props2, shell2, timing = WAITING, dictation) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`OralBrief: #${id} is not on the screen`);
      return found;
    };
    let session = sessionOf(props2);
    let busy = false;
    let take = "";
    const say = (outcome) => {
      part("brief-status").innerHTML = status(outcome);
    };
    const paint = (brief) => {
      part("brief-last").textContent = lastRunLabel(brief.lastRun, brief.duration);
      part("brief-result").innerHTML = result(brief);
    };
    const field = () => part("brief-text");
    const mic = part("brief-mic-box");
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
    const count2 = () => {
      const analyse2 = host2.querySelector("#brief-analyse");
      if (!analyse2) return;
      analyse2.disabled = busy || field().value.trim() === "" && state !== "recording" && take === "";
    };
    const setMic = (next) => {
      state = next;
      stopTimers();
      if (next === "recording") {
        dictation?.start?.();
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
      const button2 = part("brief-mic");
      button2.setAttribute("aria-pressed", String(next === "recording"));
      button2.setAttribute("aria-label", `Brief oral \u2014 ${MIC_TEXTS[next]}`);
      count2();
    };
    const hear = () => {
      if (state !== "recording") return;
      setMic("paused");
      const recorded = dictation?.audio() ?? "";
      if (recorded !== "") take = recorded;
      const heard = dictation?.transcript() ?? "";
      count2();
      if (heard === "") return;
      const area = field();
      area.value = area.value === "" ? heard : `${area.value}
${heard}`;
      count2();
    };
    const dictate = () => state === "recording" ? hear() : setMic("recording");
    const lock = (locked) => {
      busy = locked;
      for (const control of host2.querySelectorAll("#brief-text, #brief-mic")) control.disabled = locked;
      count2();
    };
    const read = async () => parseBrief((await shell2.capability(GET_LAST_BRIEF, void 0, { session })).data);
    async function send(dataCapability, payload) {
      const answer = await shell2.command(commandBody(dataCapability, payload));
      const id = commandId(answer);
      let outcome = settled(answer);
      for (let asked = 0; !isFinal(outcome.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        outcome = settled(await shell2.commandResult(id));
      }
      return outcome;
    }
    const analyse = async () => {
      if (busy) return;
      hear();
      const text2 = field().value.trim();
      if (text2 === "" && take === "") {
        field().focus();
        return;
      }
      if (session === "") return say({ kind: "error", text: "Aucune s\xE9ance choisie." });
      const target = session;
      lock(true);
      try {
        say({ kind: "busy", text: "Analyse du brief en cours\u2026" });
        const audio = text2 === "" ? await take : "";
        if (text2 === "" && audio === "") return say({ kind: "error", text: "Aucun son n\u2019a \xE9t\xE9 enregistr\xE9." });
        const outcome = await send(ANALYZE_BRIEF, text2 === "" ? { session: target, dictation: audio } : { session: target, text: text2 });
        if (outcome.status === "rejected") return say({ kind: "error", text: `Analyse refus\xE9e : ${reasons(outcome.violations)}.` });
        if (!isFinal(outcome.status) && outcome.status !== "") return say({ kind: "error", text: `L\u2019analyse est toujours en attente (\xE9tat : ${outcome.status}) : elle s\u2019appliquera \xE0 son tour, ne la relancez pas.` });
        if (outcome.status !== "applied") return say({ kind: "error", text: `L\u2019analyse n\u2019a pas abouti (\xE9tat : ${outcome.status || "inconnu"}).` });
        if (target !== session) return;
        field().value = "";
        seconds = 0;
        take = "";
        setMic("idle");
        try {
          const brief = await read();
          if (target !== session) return;
          paint(brief);
          say({ kind: "ok", text: `Brief analys\xE9 \xB7 ${brief.scenes.length} sc\xE8ne${brief.scenes.length > 1 ? "s" : ""}, ${brief.missing.length} \xE0 cr\xE9er.` });
        } catch {
          say({ kind: "error", text: "Le brief a \xE9t\xE9 analys\xE9, mais son r\xE9sultat n\u2019a pas pu \xEAtre relu." });
        }
      } catch {
        say({ kind: "error", text: "L\u2019analyse a \xE9chou\xE9." });
      } finally {
        lock(false);
      }
    };
    host2.addEventListener("input", (event) => {
      if (event.target?.id === "brief-text") count2();
    });
    host2.addEventListener("click", (event) => {
      const button2 = event.target?.closest("button");
      if (!button2) return;
      if (button2.dataset.writes === ANALYZE_BRIEF) void analyse();
      else if (button2.id === "brief-mic") dictate();
      else if (button2.dataset.diff !== void 0) {
        const Event = host2.ownerDocument.defaultView?.CustomEvent ?? CustomEvent;
        host2.dispatchEvent(new Event("open-versions", { bubbles: true, detail: { id: button2.dataset.diff } }));
      }
    });
    setMic("idle");
    paint(NO_BRIEF);
    const load = () => {
      if (session === "") return;
      const target = session;
      read().then(
        (brief) => {
          if (target === session) paint(brief);
        },
        () => {
          if (target === session) say({ kind: "error", text: "Impossible de lire le dernier brief." });
        }
      );
    };
    load();
    return {
      update(next) {
        const chosen = sessionOf(next);
        if (chosen === session) return;
        session = chosen;
        if (state === "recording") void Promise.resolve(dictation?.audio?.()).catch(() => void 0);
        seconds = 0;
        take = "";
        setMic("idle");
        field().value = "";
        paint(NO_BRIEF);
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

  // mock/src/server.js
  var LATENCY_MS = 120;
  var delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  var uuid = () => crypto.randomUUID ? crypto.randomUUID() : "xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx".replace(/x/g, () => Math.floor(Math.random() * 16).toString(16));
  var json = (body, status2 = 200) => new Response(JSON.stringify(body), { status: status2, headers: { "content-type": "application/json" } });
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
      const known2 = (await s.aggregates())[name];
      return known2 ? known2.manifest : void 0;
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
      const known2 = (await s.manifests())[dataCapability];
      const aggregate = known2 && known2.manifest && known2.manifest.target && known2.manifest.target.aggregate || target.aggregate || null;
      const document2 = await s.aggregate(aggregate) || { fields: {} };
      const declared = document2.fields || {};
      const derived = Object.keys(payload).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      const keys = known2 ? undeclaredKeys(known2.manifest, payload) : [];
      const warnUndeclared = () => {
        if (!keys.length) return;
        const warning = undeclaredWarning(keys, dataCapability);
        entry.warnings.push(warning);
        s.undeclared.push({ commandId: commandId2, dataCapability, keys, manifest: known2.path, warning });
        console.error(`mock server: ${warning} (${known2.path}) \u2014 the real server accepts it with this warning and the journeys fail on it: send only what the manifest's payload declares`);
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
      const needed = reviewNeeded(document2, fieldsOf(known2 && known2.manifest, payload), body.caller);
      const needsReview = verdict.review.length > 0 || needed.length > 0;
      const merging = new Set(Object.keys(declared).filter((f) => declared[f].merge));
      const ahead = declaredAhead(entry, merging);
      if (needsReview) {
        entry.state = "awaiting_review";
        entry.reviewId = `r_${s.next()}`;
        entry.warnings.push(`review ${entry.reviewId} required`);
        const first = fieldsOf(known2 && known2.manifest, payload)[0];
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

  // mock/src/OralBrief.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  mockServer().load("OralBrief").then((fixture) => {
    let told = false;
    const dictation = { transcript: () => told ? "" : (told = true, fixture.transcript.join("\n")), audio: () => "" };
    host.addEventListener("open-versions", (e) => window.Mock?.emit("open-versions", e.detail));
    compose({ session: fixture.session, ...props }, (resolved) => mount(host, resolved, shell, void 0, dictation));
  });
})();
