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
      const meta2 = page.meta("shell-caller");
      if (meta2 !== void 0) return { caller: asCaller(JSON.parse(meta2)) ?? FAIL_CLOSED, settled: true };
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

  // apps/session/live-session-bar/src/ids.ts
  var GET_LIVE_SESSION = "session.getLiveSession";
  var GET_DIFFUSION = "live.getDiffusion";
  var GET_SESSION_JOURNAL = "session.getSessionJournal";
  var GET_PREVIOUS_SESSION_SUMMARY = "session.getPreviousSessionSummary";
  var SHOW_COVER = "live.showCover";
  var CLOSE_WINDOW = "live.closeWindow";
  var CLOSE_SESSION = "session.closeSession";
  var DATA_CAPABILITY_VERSION = 1;
  var SESSION_AGGREGATE = "Session";
  var SCENE_CONTEXT = "scene";

  // apps/session/live-session-bar/src/model.ts
  var isDict = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var list = (value) => Array.isArray(value) ? value : [];
  var texts = (value) => list(value).filter((t) => typeof t === "string");
  function parseScene(value) {
    if (!isDict(value)) return null;
    const id = text(value.id);
    return id === "" ? null : { id, order: typeof value.order === "number" ? value.order : 0, title: text(value.title) };
  }
  function parseLive(data) {
    if (!isDict(data)) return null;
    const header = isDict(data.session) ? data.session : data;
    const id = text(header.id);
    if (id === "") return null;
    const scenes2 = list(header.scenes ?? data.scenes).flatMap((s) => parseScene(s) ?? []);
    const current = header.currentScene ?? data.currentScene;
    const mic = isDict(data.microphone) ? data.microphone.on : void 0;
    return {
      id,
      order: typeof header.order === "number" ? header.order : 0,
      title: text(header.title),
      startedAt: text(header.startedAt),
      micOn: typeof mic === "boolean" ? mic : null,
      scenes: scenes2,
      currentScene: typeof current === "number" && current >= 0 && current < scenes2.length ? current : 0
    };
  }
  var KINDS = /* @__PURE__ */ new Set(["cover", "illustration", "battlemap"]);
  function parseDiffusion(data) {
    const answer = isDict(data) ? data : {};
    const base = isDict(answer.base) ? answer.base : {};
    const kind = KINDS.has(text(base.kind)) ? text(base.kind) : "cover";
    const window2 = isDict(answer.window) ? answer.window : null;
    return { base: { kind, title: text(base.title) }, window: window2 === null ? null : { title: text(window2.title) } };
  }
  function showing(diffusion) {
    const { kind, title } = diffusion.base;
    const base = kind === "illustration" ? title === "" ? "une illustration (sc\xE8ne)" : `\xAB ${title} \xBB (sc\xE8ne)` : kind === "battlemap" ? title === "" ? "une battlemap" : `la battlemap \xAB ${title} \xBB` : "la couverture de la campagne";
    const window2 = diffusion.window === null ? null : diffusion.window.title === "" ? "une fen\xEAtre" : `fen\xEAtre \xAB ${diffusion.window.title} \xBB`;
    return { base, window: window2 };
  }
  var MINUTES_PER_DAY = 24 * 60;
  function durationSince(startedAt, now) {
    const match = /^(\d{1,2}):(\d{2})$/.exec(startedAt.trim());
    if (match === null) return "";
    const start = Number(match[1]) * 60 + Number(match[2]);
    const minutes = ((now.getHours() * 60 + now.getMinutes() - start) % MINUTES_PER_DAY + MINUTES_PER_DAY) % MINUTES_PER_DAY;
    const hours = Math.floor(minutes / 60);
    return hours === 0 ? `${minutes} min` : `${hours} h ${String(minutes % 60).padStart(2, "0")} min`;
  }
  function modelOf(meta2) {
    if (isDict(meta2)) return text(meta2.model);
    const sentence = text(meta2);
    return /\bpar ([^,]+)/.exec(sentence)?.[1]?.trim() ?? sentence;
  }
  function parseRecap(journal, summary, duration) {
    const lines = isDict(journal) ? list(journal.journal) : [];
    const answer = isDict(summary) ? summary : {};
    return {
      journalLines: lines.length,
      duration,
      summaryModel: modelOf(answer.summaryMeta),
      summary: text(answer.summary),
      mentions: list(answer.mentions).flatMap((m) => isDict(m) && text(m.name) !== "" ? [text(m.name)] : []),
      threads: texts(answer.threads)
    };
  }
  var linesLabel = (count) => `${count.toLocaleString("fr-FR")} ligne${count > 1 ? "s" : ""}`;
  function commandBody(dataCapability, idempotencyKey, target) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload: {},
      ...target === void 0 ? {} : { target },
      idempotencyKey,
      caller: { by: "dm", from: "LiveSessionBar", roles: [] }
    };
  }
  var VIOLATIONS = {
    "status-lifecycle": "cette s\xE9ance est d\xE9j\xE0 close",
    "close-session": "la cl\xF4ture a \xE9t\xE9 refus\xE9e"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isDict(answer) && isDict(answer.result) ? answer.result : {};
    return { status: text(result.status), violations: texts(result.violations) };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isDict(answer) ? text(answer.commandId) || (isDict(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/session/live-session-bar/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var button = (id, label, variant, extra = "") => `<button type="button" class="${variant}" id="${id}"${extra}>${esc(label)}</button>`;
  function skeleton() {
    const live = `data-reads="${GET_LIVE_SESSION}"`;
    return `<section class="mf-live-bar" id="session-bar" aria-labelledby="session-bar-title" ${live}><div class="mf-live-bar__title"><h1 id="session-bar-title"></h1><span class="small muted" id="session-bar-meta" ${live}></span></div><div class="kit-badge" id="session-bar-badges" ${live}></div><nav class="mf-live-bar__scenes" id="session-bar-scenes" aria-label="Sc\xE8nes de la s\xE9ance" ${live}></nav><div class="mf-live-bar__broadcast" id="session-bar-broadcast" data-reads="${GET_DIFFUSION}"><span class="small" id="session-bar-showing" aria-live="polite"></span><div class="kit-button kit-button--bare kit-button--small" id="session-bar-broadcast-actions"></div></div><div class="kit-button kit-button--bare kit-button--small" id="session-bar-actions">` + button("session-bar-player", "Ouvrir la player view", "secondary", ' data-href="player-view.html" data-target="_blank"') + button("session-bar-pilot", "Player view pilot\xE9e", "ghost", ' data-href="player-view.html?pilote=1" data-target="_blank"') + button("session-bar-close", "Clore la s\xE9ance", "danger", ` data-writes="${CLOSE_SESSION}"`) + '</div><div class="mf-modal" id="session-close-modal" hidden></div></section>';
  }
  var heading = (live) => `S\xE9ance ${live.order} \u2014 ${live.title}`;
  var meta = (live) => live.startedAt === "" ? "En direct" : `En direct depuis ${live.startedAt}`;
  function badges(live, closed) {
    const mic = live.micOn === null ? "" : live.micOn && !closed ? '<span class="accent">Micro actif</span>' : "<span>Micro en pause</span>";
    return (closed ? '<span class="danger">Close</span>' : '<span class="success">En direct</span>') + mic;
  }
  var scenes = (live) => live.scenes.map((s, i) => `<button type="button" class="chip${i === live.currentScene ? " is-on" : ""}" data-scene="${i}" id="session-bar-scene-${i + 1}" title="${esc(s.title)}" aria-pressed="${i === live.currentScene}">Sc\xE8ne ${s.order} \xB7 ${esc(s.title)}</button>`).join("");
  function showingHtml(diffusion) {
    const what = showing(diffusion);
    return `Les joueurs voient <b>${esc(what.base)}</b>` + (what.window === null ? "" : ` + <b>${esc(what.window)}</b>`);
  }
  function broadcastActions(diffusion) {
    return (diffusion.base.kind === "cover" ? "" : button("session-bar-cover", "Revenir \xE0 la couverture", "ghost", ` data-writes="${SHOW_COVER}"`)) + (diffusion.window === null ? "" : button("session-bar-close-window", "Fermer la fen\xEAtre", "ghost", ` data-writes="${CLOSE_WINDOW}"`));
  }
  function dialog(title, body, footer) {
    return `<div class="kit-dialog"><div class="kit-dialog__veil"><div class="kit-dialog__window" role="dialog" aria-modal="true" aria-labelledby="session-close-title"><header class="kit-dialog__header"><h2 class="kit-dialog__title" id="session-close-title">${esc(title)}</h2><button class="kit-dialog__close" type="button" id="session-close-x" aria-label="Fermer">\xD7</button></header><div class="kit-dialog__body">${body}</div><footer class="kit-dialog__footer">${footer}</footer></div></div></div>`;
  }
  var dialogButton = (id, label, variant, extra = "") => `<button type="button" class="kit-dialog__btn kit-dialog__btn--${variant}" id="${id}"${extra}>${esc(label)}</button>`;
  function confirmDialog(live, problem = "", busy = false) {
    return dialog(
      `Clore la s\xE9ance ${live.order}`,
      `<p class="kit-dialog__text">Le micro sera mis en pause et les sons arr\xEAt\xE9s. Le journal transcrit est conserv\xE9 sur cette machine ; un r\xE9sum\xE9 est g\xE9n\xE9r\xE9 avec le mod\xE8le configur\xE9.</p><p class="small muted">Une s\xE9ance close ne se rouvre pas depuis la barre.</p><p class="mf-live-bar__status${problem === "" ? "" : " mf-live-bar__error"}" id="session-close-status" role="status" aria-live="polite">${esc(busy ? "Cl\xF4ture en cours\u2026" : problem)}</p>`,
      dialogButton("session-close-cancel", "Continuer la s\xE9ance", "ghost", busy ? " disabled" : "") + dialogButton("session-close-submit", "Clore la s\xE9ance", "primary", ` data-writes="${CLOSE_SESSION}"${busy ? " disabled" : ""}`)
    );
  }
  function recapDialog(live, recap, problem = "") {
    const row = (id, label, value, reads) => `<li class="kit-dialog__row" id="${id}" data-reads="${reads}"><span>${esc(label)}</span><strong>${esc(value)}</strong></li>`;
    const summary = `data-reads="${GET_PREVIOUS_SESSION_SUMMARY}"`;
    return dialog(
      `S\xE9ance ${live.order} close`,
      '<p class="kit-dialog__text">Le micro est en pause et les sons sont arr\xEAt\xE9s. Le journal transcrit est conserv\xE9 sur cette machine.</p>' + (problem === "" ? "" : `<p class="mf-live-bar__error" id="session-close-status" role="status">${esc(problem)}</p>`) + '<ul class="kit-dialog__list">' + row("session-close-lines", "Lignes de journal", linesLabel(recap.journalLines), GET_SESSION_JOURNAL) + (recap.duration === "" ? "" : row("session-close-duration", "Dur\xE9e", recap.duration, GET_LIVE_SESSION)) + (recap.summaryModel === "" ? "" : row("session-close-model", "Mod\xE8le du r\xE9sum\xE9", recap.summaryModel, GET_PREVIOUS_SESSION_SUMMARY)) + `</ul><div class="mf-live-bar__summary"><h3>R\xE9sum\xE9 de la s\xE9ance</h3><p id="session-close-summary" ${summary}>${esc(recap.summary === "" ? "Aucun r\xE9sum\xE9 n\u2019a pu \xEAtre g\xE9n\xE9r\xE9." : recap.summary)}</p>` + (recap.mentions.length === 0 ? "" : `<h3>Entit\xE9s mentionn\xE9es</h3><div class="row" id="session-close-mentions" ${summary}>${recap.mentions.map((m) => `<span class="chip">${esc(m)}</span>`).join("")}</div>`) + (recap.threads.length === 0 ? "" : `<h3>Fils ouverts</h3><ul class="list" ${summary}>${recap.threads.map((t, i) => `<li class="item" id="session-close-thread-${i + 1}">${esc(t)}</li>`).join("")}</ul>`) + '</div><p class="small muted">Ce r\xE9sum\xE9 appara\xEEtra en t\xEAte de la pr\xE9paration de la s\xE9ance suivante.</p>',
      dialogButton("session-close-done", "Retour \xE0 la campagne", "primary", ' data-href="campagne.html"')
    );
  }

  // apps/session/live-session-bar/src/style.ts
  var STYLE = `
.mf-live-bar { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-live-bar *, .mf-live-bar *::before, .mf-live-bar *::after { box-sizing: inherit; }
.mf-live-bar [hidden] { display: none !important; }
.mf-live-bar h1, .mf-live-bar h2, .mf-live-bar h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-live-bar h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-live-bar p { margin: 0; }
.mf-live-bar .muted { color: var(--color-text-muted); }
.mf-live-bar .small { font-size: var(--font-size-small); }
.mf-live-bar .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-live-bar .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-live-bar .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-live-bar .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-live-bar .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-live-bar .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); text-decoration: none; cursor: pointer; }
.mf-live-bar .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-live-bar .chip.is-on { background: var(--color-primary); border-color: var(--color-primary); color: var(--color-primary-contrast); font-weight: var(--font-weight-heading); }
.mf-live-bar { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-3) var(--space-4); padding: var(--space-2) var(--space-4); background: var(--color-surface-2); border-bottom: var(--border-width) solid var(--color-border); }
.mf-live-bar__title { display: grid; }
.mf-live-bar__title h1 { font-size: var(--font-size-large); color: var(--color-primary); }
.mf-live-bar__scenes { display: flex; flex-wrap: wrap; gap: var(--space-1); }
.mf-live-bar__broadcast { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); margin-left: auto; padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); }
.mf-live-bar__broadcast b { color: var(--color-accent); }
.mf-live-bar__summary { display: grid; gap: var(--space-2); }
.mf-live-bar__status { min-height: var(--space-4); }
.mf-live-bar__error { color: var(--color-danger); }
.mf-live-bar .mf-modal { position: fixed; inset: 0; z-index: 100; }
.mf-live-bar .mf-modal .kit-dialog, .mf-live-bar .mf-modal .kit-dialog__veil { height: 100%; }
.mf-live-bar .mf-modal .kit-dialog__window { max-width: calc(var(--space-5) * 20); max-height: 88vh; }
.mf-live-bar .kit-dialog__btn:disabled { opacity: 0.55; cursor: not-allowed; }
`;

  // apps/session/live-session-bar/src/index.ts
  var DEFAULT_TIMING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120, now: () => /* @__PURE__ */ new Date() };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="LiveSessionBar"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "LiveSessionBar";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, props2, shell2, timing = DEFAULT_TIMING) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`LiveSessionBar: #${id} is not in the bar`);
      return found;
    };
    const requested = typeof props2.session === "string" && props2.session !== "" ? { id: props2.session } : void 0;
    let live = null;
    let diffusion = parseDiffusion(void 0);
    let closed = false;
    let busy = false;
    const paintLive = () => {
      part("session-bar-title").textContent = live === null ? "Aucune s\xE9ance en cours" : heading(live);
      part("session-bar-meta").textContent = live === null ? "" : meta(live);
      part("session-bar-badges").innerHTML = live === null ? "" : badges(live, closed);
      part("session-bar-scenes").innerHTML = live === null ? "" : scenes(live);
      part("session-bar-close").disabled = live === null || closed;
    };
    const paintDiffusion = () => {
      part("session-bar-showing").innerHTML = showingHtml(diffusion);
      part("session-bar-broadcast-actions").innerHTML = broadcastActions(diffusion);
    };
    const readLive = async () => {
      live = parseLive((await shell2.capability(GET_LIVE_SESSION, void 0, requested)).data);
      paintLive();
      if (live !== null) shell2.context.set(SCENE_CONTEXT, live.currentScene);
    };
    const readDiffusion = async () => {
      diffusion = parseDiffusion((await shell2.capability(GET_DIFFUSION, void 0)).data);
      paintDiffusion();
    };
    async function send(dataCapability, target) {
      const answer = await shell2.command(commandBody(dataCapability, `${dataCapability}:${crypto.randomUUID()}`, target));
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    async function broadcast(dataCapability) {
      if (busy) return;
      busy = true;
      try {
        const state = await send(dataCapability);
        if (state.status === "rejected") part("session-bar-showing").textContent = `Refus\xE9 : ${reasons(state.violations)}.`;
        else if (state.status !== "applied") part("session-bar-showing").textContent = `La commande n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`;
        else await readDiffusion();
      } catch {
        part("session-bar-showing").textContent = "La commande a \xE9chou\xE9.";
      } finally {
        busy = false;
      }
    }
    const modal = () => part("session-close-modal");
    const show = (html) => {
      modal().innerHTML = html;
      modal().hidden = false;
      (modal().querySelector(".kit-dialog__footer button:not(:disabled)") ?? modal().querySelector("button"))?.focus();
    };
    const hide = () => {
      modal().hidden = true;
      modal().innerHTML = "";
    };
    async function recap(session) {
      const duration = durationSince(session.startedAt, timing.now());
      const [journal, summary] = await Promise.allSettled([
        shell2.capability(GET_SESSION_JOURNAL, void 0, { session: session.id }),
        shell2.capability(GET_PREVIOUS_SESSION_SUMMARY, void 0, { session: session.id })
      ]);
      const lost = journal.status === "rejected" || summary.status === "rejected";
      show(
        recapDialog(
          session,
          parseRecap(journal.status === "fulfilled" ? journal.value.data : void 0, summary.status === "fulfilled" ? summary.value.data : void 0, duration),
          lost ? "La s\xE9ance est close, mais son bilan n\u2019a pas pu \xEAtre lu en entier." : ""
        )
      );
    }
    async function closeSession() {
      const session = live;
      if (busy || session === null || closed) return;
      busy = true;
      show(confirmDialog(session, "", true));
      try {
        const state = await send(CLOSE_SESSION, { aggregate: SESSION_AGGREGATE, id: session.id });
        if (state.status === "rejected") return show(confirmDialog(session, `Cl\xF4ture refus\xE9e : ${reasons(state.violations)}.`));
        if (state.status !== "applied") return show(confirmDialog(session, `La cl\xF4ture n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`));
      } catch {
        return show(confirmDialog(session, "La cl\xF4ture a \xE9chou\xE9."));
      } finally {
        busy = false;
      }
      closed = true;
      paintLive();
      await recap(session);
    }
    host2.addEventListener("click", (event) => {
      const target = event.target;
      const button2 = target?.closest("button");
      if (!button2 || button2.disabled) return;
      if (button2.dataset.href !== void 0) {
        event.stopPropagation();
        if (button2.dataset.target === "_blank") host2.ownerDocument.defaultView?.open(button2.dataset.href, "_blank", "noopener");
        else host2.ownerDocument.defaultView?.location.assign(button2.dataset.href);
      } else if (button2.dataset.scene !== void 0 && live !== null) {
        live = { ...live, currentScene: Number(button2.dataset.scene) };
        paintLive();
        shell2.context.set(SCENE_CONTEXT, live.currentScene);
      } else if (button2.dataset.writes === SHOW_COVER || button2.dataset.writes === CLOSE_WINDOW) void broadcast(button2.dataset.writes);
      else if (button2.id === "session-bar-close" && live !== null) show(confirmDialog(live));
      else if (button2.id === "session-close-submit") void closeSession();
      else if (button2.id === "session-close-cancel" || button2.id === "session-close-x") hide();
    });
    host2.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && !modal().hidden && !busy) hide();
    });
    paintLive();
    paintDiffusion();
    part("session-bar-title").textContent = "Lecture de la s\xE9ance\u2026";
    void readLive().catch(() => {
      part("session-bar-title").textContent = "Impossible de lire la s\xE9ance.";
    });
    void readDiffusion().catch(() => {
      part("session-bar-showing").textContent = "Impossible de lire ce que voient les joueurs.";
    });
    shell2.onDataVersion(() => {
      if (!busy) void readDiffusion().catch(() => void 0);
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

  // mock/src/LiveSessionBar.js
  var { host, props } = mountPoint();
  var shared = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var shell = Object.assign(Object.create(shared), {
    command: async (body) => {
      const answer = await shared.command(body);
      if (String(body && body.dataCapability).startsWith("session.closeSession")) {
        const mic = document.getElementById("suggestions-mic-button");
        const control = mic && mic.closest("[data-kit]");
        if (control && control.mic && control.mic.state === "recording") control.mic.set("paused");
      }
      return answer;
    }
  });
  shared.context.on((name, value) => {
    if (name === "scene") document.dispatchEvent(new CustomEvent("fdm:scene", { detail: { index: value } }));
  });
  fetch(new URL("../fixtures/LiveSessionBar.json", location.href)).then((answer) => answer.json()).then((bar) => {
    const [hours, minutes] = bar.session.startedAt.split(":").map(Number);
    const [, ranHours = 0, ranMinutes = 0] = (/(\d+) h (\d+) min/.exec(bar.closing.duration) || []).map(Number);
    const timing = { ...DEFAULT_TIMING, now: () => new Date(2026, 0, 1, hours + ranHours, minutes + ranMinutes) };
    compose(props, (resolved) => mount(host, resolved, shell, timing));
  });
})();
