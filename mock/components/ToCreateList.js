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
  function createShell(fetcher = (input, init) => fetch(input, init), page = browserPage()) {
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

  // apps/ai/to-create-list/src/ids.ts
  var LIST_ENTITIES_TO_CREATE = "campaign.listEntitiesToCreate";
  var GET_ENTITY = "campaign.getEntity";
  var CREATE_FROM_BRIEF = "ai.createFromBrief";
  var DATA_CAPABILITY_VERSION = 1;
  var SESSION_AGGREGATE = "Session";
  var APPROVER = "role:approver";

  // apps/ai/to-create-list/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var figure = (value) => typeof value === "number" && Number.isFinite(value) ? value : null;
  function readItem(value) {
    if (!isRecord(value) || text(value.id) === "") return null;
    const entity = text(value.entity) === "" ? null : text(value.entity);
    return {
      id: text(value.id),
      type: text(value.type),
      name: text(value.name),
      session: text(value.session) === "" ? null : text(value.session),
      sessionTitle: text(value.sessionTitle),
      excerpt: text(value.excerpt),
      // to-create-leaves-once-created: an entry is created if and only if its element is set.
      status: value.status === "created" && entity !== null ? "created" : "todo",
      entity: value.status === "created" ? entity : null
    };
  }
  function parseItems(data) {
    const list = Array.isArray(data) ? data : isRecord(data) && Array.isArray(data.items) ? data.items : [];
    return list.map(readItem).filter((item) => item !== null);
  }
  var inSession = (items, session) => session === "" ? [...items] : items.filter((item) => item.session === session);
  var remaining = (items) => items.filter((item) => item.status === "todo").length;
  var countLabel = (items) => `${remaining(items)} \xE0 cr\xE9er \xB7 ${items.length - remaining(items)} cr\xE9\xE9e(s)`;
  var ABILITIES = ["FOR", "DEX", "CON", "INT", "SAG", "CHA"];
  function readSheet(entity) {
    if (!isRecord(entity.stats)) return null;
    const stats = entity.stats;
    const modifiers = isRecord(entity.modifiers) ? entity.modifiers : {};
    const abilities = ABILITIES.map((key) => ({ key, score: figure(stats[key]), modifier: figure(modifiers[key]) }));
    const features = Array.isArray(entity.features) ? entity.features.filter(isRecord).map((feature) => ({ name: text(feature.name), text: text(feature.text) })) : [];
    return { ca: figure(entity.ca), pvMax: figure(entity.pvMax), abilities, features };
  }
  function parseProposal(data) {
    if (!isRecord(data) || text(data.id) === "") return null;
    return {
      name: text(data.name),
      description: text(data.description),
      aliases: Array.isArray(data.aliases) ? data.aliases.filter((alias) => typeof alias === "string" && alias !== "") : [],
      background: text(data.background),
      behavior: text(data.behavior),
      sheet: readSheet(data)
    };
  }
  var signed = (modifier) => modifier < 0 ? `\u2212${Math.abs(modifier)}` : `+${modifier}`;
  var refinementOf = (typed) => typed.trim() === "" ? null : typed.trim();
  function commandBody(dataCapability, item, refinement) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload: { entry: item.id, ...refinement === null ? {} : { refinement } },
      ...item.session === null ? {} : { target: { aggregate: SESSION_AGGREGATE, id: item.session } },
      caller: { by: "dm", from: "ToCreateList", roles: [APPROVER] }
    };
  }
  var VIOLATIONS = {
    "to-create-leaves-once-created": "cet \xE9l\xE9ment est d\xE9j\xE0 cr\xE9\xE9",
    "not-found": "cet \xE9l\xE9ment n\u2019est plus dans la liste"
  };
  var reasons = (violations) => violations.map((violation) => VIOLATIONS[violation] ?? violation).join(", ");
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

  // apps/ai/to-create-list/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var MIC = '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
  var PAUSE = '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';
  var MIC_TEXTS = { idle: "Pr\xEAt \xE0 dicter", recording: "Enregistrement", paused: "En pause" };
  var clock = (seconds) => `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
  var freshView = () => ({ refining: false, typed: "", mic: "idle", seconds: 0, outcome: null, proposal: null });
  var TYPES = { pnj: "PNJ", lieu: "Lieu", "\xE9v\xE9nement": "\xC9v\xE9nement" };
  function buttons(actions, locked) {
    return '<div class="kit-button kit-button--bare kit-button--small">' + actions.map((a) => `<button type="button" class="${a.variant}" data-action="${a.action}"${a.writes === void 0 ? "" : ` data-writes="${a.writes}"`}${locked ? " disabled" : ""}>${esc(a.label)}</button>`).join("") + "</div>";
  }
  function statblock(name, sheet) {
    const vitals = [
      sheet.ca === null ? "" : `<li class="kit-statblock__vital"><strong>CA</strong> <span>${sheet.ca}</span></li>`,
      sheet.pvMax === null ? "" : `<li class="kit-statblock__vital"><strong>PV</strong> <span>${sheet.pvMax}</span></li>`
    ].join("");
    const abilities = sheet.abilities.map(
      (a) => `<li class="kit-statblock__ability"><span class="kit-statblock__abbr">${a.key}</span><span class="kit-statblock__score">${a.score ?? "\u2014"}</span>${a.modifier === null ? "" : `<span class="kit-statblock__mod">${signed(a.modifier)}</span>`}</li>`
    ).join("");
    const features = sheet.features.map((f) => `<li class="kit-statblock__feature"><strong>${esc(f.name)}.</strong> ${esc(f.text)}</li>`).join("");
    return `<div class="kit-statblock"><div class="kit-statblock__rule"></div><div class="kit-statblock__body"><h3 class="kit-statblock__name">${esc(name)}</h3><ul class="kit-statblock__vitals">${vitals}</ul><ul class="kit-statblock__abilities">${abilities}</ul><ul class="kit-statblock__features">${features}</ul></div></div>`;
  }
  function proposal(item, p) {
    let list = `<dl class="mf-tocreate__proposal" id="to-create-${esc(item.id)}-proposal" data-reads="${GET_ENTITY}"><dt>Description</dt><dd>${esc(p.description)}</dd>`;
    if (p.aliases.length > 0) list += `<dt>Alias propos\xE9s</dt><dd>${esc(p.aliases.join(", "))}</dd>`;
    if (p.background !== "") list += `<dt>Background</dt><dd>${esc(p.background)}</dd>`;
    if (p.behavior !== "") list += `<dt>Comportement, pour l\u2019interpr\xE9ter</dt><dd>${esc(p.behavior)}</dd>`;
    list += "</dl>";
    return list + (p.sheet === null ? "" : statblock(p.name || item.name, p.sheet));
  }
  function micButton(item, view) {
    const id = `to-create-${esc(item.id)}-mic`;
    return `<div class="kit-micbutton kit-micbutton--compact" id="${id}-box" data-state="${view.mic}"><button type="button" class="kit-micbutton__btn" id="${id}" data-action="dictate" aria-pressed="${view.mic === "recording"}" aria-label="Dicter la description \u2014 ${MIC_TEXTS[view.mic]}"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button><div class="kit-micbutton__text"><span class="kit-micbutton__label">Dicter la description</span><span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state">${MIC_TEXTS[view.mic]}</span> <span class="kit-micbutton__timer">${clock(view.seconds)}</span></span></div></div>`;
  }
  function refineForm(item, view, locked) {
    const field = `to-create-${esc(item.id)}-text`;
    return `<div class="mf-tocreate__refine" id="to-create-${esc(item.id)}-refine">` + micButton(item, view) + `<div class="kit-textarea is-wide"><label class="kit-textarea__label" for="${field}">Description affin\xE9e</label><textarea class="kit-textarea__field" id="${field}" rows="3" placeholder="Ajoutez ce que le brief ne disait pas\u2026"${locked ? " disabled" : ""}>${esc(view.typed)}</textarea><div class="kit-textarea__footer"><p class="kit-textarea__hint"></p></div></div>` + buttons(
      [
        { action: "propose-refined", label: "Proposer", variant: "primary", writes: CREATE_FROM_BRIEF },
        { action: "cancel", label: "Annuler", variant: "ghost" }
      ],
      locked
    ) + "</div>";
  }
  var statusLine = (outcome) => outcome === null ? "" : `<p class="item ${outcome.kind === "error" ? "is-error" : "muted"}" aria-live="polite">${esc(outcome.text)}</p>`;
  var sheetHref = (item) => item.entity === null ? "" : `fiche-entite.html?id=${encodeURIComponent(item.entity)}`;
  function itemHTML(item, view, locked) {
    const done = item.status === "created";
    let body = `<p class="quote">${esc(item.excerpt)}</p>`;
    if (done) {
      body += view.proposal === null ? "" : proposal(item, view.proposal);
      if (item.entity !== null) body += `<div class="kit-button kit-button--bare kit-button--small"><a class="secondary" href="${sheetHref(item)}">Ouvrir la fiche</a></div>`;
    } else if (view.refining) {
      body += refineForm(item, view, locked);
    } else {
      body += buttons(
        [
          { action: "from-brief", label: "Cr\xE9er depuis le brief", variant: "primary", writes: CREATE_FROM_BRIEF },
          { action: "refine", label: "Affiner", variant: "secondary" }
        ],
        locked
      );
    }
    const badges = `<span class="accent">${esc(TYPES[item.type] ?? item.type)}</span>${done ? '<span class="success">Cr\xE9\xE9</span>' : "<span>\xC0 cr\xE9er</span>"}`;
    return `<li class="mf-tocreate__item${done ? " is-created" : ""}" id="to-create-${esc(item.id)}" data-id="${esc(item.id)}"><div class="panel__head"><h3>${esc(item.name)}</h3><div class="kit-badge">${badges}</div></div><p class="small muted">${esc(item.sessionTitle)}</p>${body}${statusLine(view.outcome)}</li>`;
  }
  function skeleton() {
    return `<section class="panel mf-tocreate" id="to-create" aria-labelledby="to-create-title"><div class="panel__head"><h2 id="to-create-title">Entit\xE9s \xE0 cr\xE9er</h2><span class="small muted" id="to-create-count"></span></div><p class="small muted">Issues des briefs de s\xE9ance. Cr\xE9ez-les depuis le brief, ou affinez d\u2019abord la description \xE0 l\u2019oral ou \xE0 l\u2019\xE9crit.</p><p class="item is-error" id="to-create-error" hidden></p><ul class="mf-tocreate__list" id="to-create-items" data-reads="${LIST_ENTITIES_TO_CREATE}"></ul></section>`;
  }

  // apps/ai/to-create-list/src/style.ts
  var STYLE = `
.mf-tocreate { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-tocreate *, .mf-tocreate *::before, .mf-tocreate *::after { box-sizing: inherit; }
.mf-tocreate h2, .mf-tocreate h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-tocreate h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-tocreate h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-tocreate p { margin: 0; }
.mf-tocreate.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-tocreate .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-tocreate .muted { color: var(--color-text-muted); }
.mf-tocreate .small { font-size: var(--font-size-small); }
.mf-tocreate .quote { padding: var(--space-2) var(--space-3); border-left: calc(var(--border-width) * 3) solid var(--color-secondary); background: var(--color-surface); border-radius: var(--radius-small); color: var(--color-text-muted); font-style: italic; }
.mf-tocreate .item { padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-tocreate .item.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-tocreate .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-tocreate .kit-button--small button, .mf-tocreate .kit-button--small a { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-tocreate .kit-button a { font: inherit; font-weight: 600; letter-spacing: .04em; border-radius: var(--radius-pill); border: var(--border-width) solid var(--color-primary); background: var(--color-surface-2); color: var(--color-primary); text-decoration: none; box-shadow: var(--shadow-low); }
.mf-tocreate .kit-textarea.is-wide { max-width: none; }
.mf-tocreate .kit-micbutton--compact .kit-micbutton__btn { width: calc(var(--space-5) + var(--space-2)); height: calc(var(--space-5) + var(--space-2)); }
.mf-tocreate .kit-micbutton--compact .kit-micbutton__label { font-size: var(--font-size-base); }
.mf-tocreate__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-3); }
.mf-tocreate__item { display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) dashed var(--color-accent); border-radius: var(--radius-medium); }
.mf-tocreate__item.is-created { border-style: solid; border-color: var(--color-success); }
.mf-tocreate__refine { display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface-2); border-radius: var(--radius-medium); }
.mf-tocreate__proposal { display: grid; gap: var(--space-2); margin: 0; }
.mf-tocreate__proposal dt { font-family: var(--font-family-heading); text-transform: uppercase; font-size: var(--font-size-small); color: var(--color-accent); }
.mf-tocreate__proposal dd { margin: 0 0 var(--space-2); }
.mf-tocreate .kit-statblock { max-width: none; }
.mf-tocreate .kit-statblock .kit-statblock__vitals { list-style: none; }
.mf-tocreate .kit-statblock .kit-statblock__vital strong { font-weight: 700; color: var(--color-accent); }
`;

  // apps/ai/to-create-list/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="ToCreateList"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "ToCreateList";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var textOf = (props2, name) => typeof props2[name] === "string" ? props2[name] : "";
  function mount(host2, props2, shell2, timing = WAITING, dictation) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`ToCreateList: #${id} is not on the screen`);
      return found;
    };
    let campaign = textOf(props2, "campaign");
    let session = textOf(props2, "session");
    let all = [];
    let views = /* @__PURE__ */ new Map();
    let busy = false;
    let loading = 0;
    let generation = 0;
    const timers = /* @__PURE__ */ new Map();
    const viewOf = (id) => {
      let view = views.get(id);
      if (!view) {
        view = freshView();
        views.set(id, view);
      }
      return view;
    };
    const shown2 = () => inSession(all, session);
    const element = (id) => Array.from(host2.querySelectorAll("li[data-id]")).find((li) => li.dataset.id === id) ?? null;
    const fail = (text2) => {
      const line = part("to-create-error");
      line.hidden = text2 === null;
      line.textContent = text2 ?? "";
    };
    const paintItem = (item) => {
      const li = element(item.id);
      if (li) li.outerHTML = itemHTML(item, viewOf(item.id), busy);
    };
    const paint = () => {
      part("to-create-items").innerHTML = shown2().map((item) => itemHTML(item, viewOf(item.id), busy)).join("");
      part("to-create-count").textContent = countLabel(shown2());
    };
    const read = async () => parseItems((await shell2.capability(LIST_ENTITIES_TO_CREATE, void 0, { campaign })).data);
    const refresh = async () => {
      const mine = loading += 1;
      try {
        const items = await read();
        if (mine !== loading) return true;
        all = items;
        fail(null);
        paint();
        return true;
      } catch {
        return false;
      }
    };
    const load = () => {
      if (campaign === "") return;
      void refresh().then((ok) => {
        if (!ok) fail("Impossible de lire les \xE9l\xE9ments \xE0 cr\xE9er.");
      });
    };
    async function send(item, refinement) {
      const answer = await shell2.command(commandBody(CREATE_FROM_BRIEF, item, refinement));
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    const stopMic = (id) => {
      const timer = timers.get(id);
      if (timer !== void 0) clearInterval(timer);
      timers.delete(id);
    };
    async function create(item, refinement) {
      if (item.status === "created") return;
      if (campaign === "") return fail("Aucune campagne choisie.");
      const view = viewOf(item.id);
      busy = true;
      const mine = generation;
      stopMic(item.id);
      view.mic = "idle";
      view.seconds = 0;
      view.outcome = { kind: "busy", text: refinement === null ? "Cr\xE9ation depuis le brief\u2026" : "Proposition en cours\u2026" };
      paint();
      try {
        const state = await send(item, refinement);
        if (mine !== generation) return;
        if (state.status === "rejected") {
          view.outcome = { kind: "error", text: `Cr\xE9ation refus\xE9e : ${reasons(state.violations)}.` };
        } else if (state.status !== "applied") {
          view.outcome = { kind: "error", text: `La cr\xE9ation n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).` };
        } else {
          view.outcome = null;
          view.refining = false;
          view.typed = "";
          const read2 = await refresh();
          if (mine !== generation) return;
          if (!read2) view.outcome = { kind: "error", text: "L\u2019\xE9l\xE9ment a \xE9t\xE9 cr\xE9\xE9, mais la liste n\u2019a pas pu \xEAtre relue." };
          const made = all.find((candidate) => candidate.id === item.id);
          if (made?.entity) {
            try {
              const proposed = parseProposal((await shell2.capability(GET_ENTITY, void 0, { id: made.entity })).data);
              if (mine === generation) view.proposal = proposed;
            } catch {
              view.proposal = null;
            }
          }
        }
      } catch {
        view.outcome = { kind: "error", text: "La cr\xE9ation a \xE9chou\xE9." };
      } finally {
        busy = false;
        paint();
      }
    }
    function dictate(item) {
      const view = viewOf(item.id);
      if (view.mic !== "recording") {
        view.mic = "recording";
        timers.set(
          item.id,
          setInterval(() => {
            const li = element(item.id);
            if (!li || !host2.isConnected) return stopMic(item.id);
            view.seconds += 1;
            const timer = li.querySelector(".kit-micbutton__timer");
            if (timer) timer.textContent = clock(view.seconds);
            li.querySelector(".kit-micbutton")?.style.setProperty("--kit-level", (Math.random() * 0.9 + 0.1).toFixed(2));
          }, 1e3)
        );
      } else {
        view.mic = "paused";
        stopMic(item.id);
        const heard = dictation?.transcript() ?? "";
        if (heard !== "") view.typed = view.typed === "" ? heard : `${view.typed}
${heard}`;
      }
      paintItem(item);
      const box = element(item.id)?.querySelector(".kit-micbutton__state");
      if (box) box.textContent = MIC_TEXTS[view.mic];
    }
    host2.addEventListener("input", (event) => {
      const field = event.target;
      const id = field?.closest("li[data-id]")?.dataset.id;
      if (field?.tagName === "TEXTAREA" && id !== void 0) viewOf(id).typed = field.value;
    });
    host2.addEventListener("click", (event) => {
      const button = event.target?.closest("button[data-action]");
      const id = button?.closest("li[data-id]")?.dataset.id;
      const item = all.find((candidate) => candidate.id === id);
      if (!button || !item || busy) return;
      const view = viewOf(item.id);
      switch (button.dataset.action) {
        case "from-brief":
          void create(item, null);
          break;
        case "refine":
          view.refining = true;
          paintItem(item);
          break;
        case "cancel":
          stopMic(item.id);
          views.set(item.id, freshView());
          paintItem(item);
          break;
        case "propose-refined":
          void create(item, refinementOf(view.typed));
          break;
        case "dictate":
          dictate(item);
          break;
      }
    });
    paint();
    load();
    const unsubscribe = shell2.onDataVersion?.(() => {
      if (!host2.isConnected) return unsubscribe?.();
      if (!busy) load();
    });
    return {
      update(next) {
        const chosen = textOf(next, "campaign");
        session = textOf(next, "session");
        if (chosen !== campaign) {
          campaign = chosen;
          generation += 1;
          all = [];
          views = /* @__PURE__ */ new Map();
          for (const id of [...timers.keys()]) stopMic(id);
          load();
        }
        paint();
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

  // mock/src/ToCreateList.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  mockServer().load("ToCreateList").then((fixture) => {
    const heard = () => {
      const id = host.querySelector('.kit-micbutton[data-state="recording"]')?.closest("[data-id]")?.dataset.id;
      return id && fixture.proposals[id]?.dictation || "";
    };
    compose({ campaign: "cendres-valombre", ...props }, (resolved) => mount(host, resolved, shell, void 0, { transcript: heard }));
  });
})();
