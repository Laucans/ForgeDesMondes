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

  // apps/ai/ai-settings/src/ids.ts
  var GET_SETTINGS = "ai.getSettings";
  var SAVE_SETTINGS = "ai.saveSettings";
  var TEST_CONNECTION = "ai.testConnection";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/ai/ai-settings/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var models = (value) => ({ generationModel: text(value.generationModel), summaryModel: text(value.summaryModel) });
  function entries(value, read) {
    const out = {};
    if (isRecord(value)) {
      for (const [name, entry] of Object.entries(value)) if (isRecord(entry)) out[name] = read(entry);
    }
    return out;
  }
  function lastTest(value) {
    if (!isRecord(value) || typeof value.ok !== "boolean") return null;
    return {
      ok: value.ok,
      at: text(value.at),
      provider: typeof value.provider === "string" ? value.provider : null,
      latency: typeof value.latency === "string" ? value.latency : null,
      message: text(value.message)
    };
  }
  function parseSettings(data) {
    const answer = isRecord(data) ? data : {};
    const binary = isRecord(answer.binary) ? { binary: text(answer.binary.binary), ...models(answer.binary) } : null;
    return {
      mode: answer.mode === "binary" ? "binary" : "api",
      provider: typeof answer.provider === "string" ? answer.provider : null,
      binary,
      providers: entries(answer.providers, (p) => ({ label: text(p.label), url: text(p.url), key: text(p.key), ...models(p) })),
      binaries: entries(answer.binaries, (b) => ({ label: text(b.label), ...models(b) })),
      lastTest: lastTest(answer.lastTest),
      local: texts(answer.local),
      sent: texts(answer.sent)
    };
  }
  var stored = (binary) => binary === "claude" ? "claude-code" : binary;
  function binaryModels(settings, binary) {
    const own = settings.binaries[stored(binary)] ?? (settings.binary?.binary === stored(binary) ? settings.binary : void 0);
    const api = settings.providers[binary === "claude" ? "cloud" : "opencode"];
    const found = own ?? api;
    return { generationModel: found?.generationModel ?? "", summaryModel: found?.summaryModel ?? "" };
  }
  function initialForm(settings) {
    if (settings.mode === "binary") {
      const binary = settings.provider === "claude-code" ? "claude" : "opencode";
      return { mode: "binary", binary, url: "", key: "", ...binaryModels(settings, binary) };
    }
    const active = settings.provider === null ? void 0 : settings.providers[settings.provider];
    return { mode: "api", binary: "opencode", url: active?.url ?? "", key: "", generationModel: active?.generationModel ?? "", summaryModel: active?.summaryModel ?? "" };
  }
  function withMode(settings, form, mode) {
    if (mode === form.mode) return form;
    return mode === "binary" ? { ...form, mode, ...binaryModels(settings, form.binary) } : { ...form, mode };
  }
  function withBinary(settings, form, binary) {
    return { ...form, binary, ...binaryModels(settings, binary) };
  }
  function savedTypeLabel(settings) {
    if (settings.mode !== "api" || settings.provider === null) return null;
    return settings.providers[settings.provider]?.label || null;
  }
  function testedTypeLabel(settings) {
    const test = settings.lastTest;
    return test?.ok && test.provider !== null ? settings.providers[test.provider]?.label || null : null;
  }
  var isHttp = (url) => {
    try {
      return /^https?:$/.test(new URL(url).protocol);
    } catch {
      return false;
    }
  };
  function problem(form) {
    if (form.mode === "api" && !isHttp(form.url.trim())) return "Renseignez une URL d\u2019API valide (http ou https).";
    if (!form.generationModel.trim()) return "Le mod\xE8le de g\xE9n\xE9ration est obligatoire.";
    if (!form.summaryModel.trim()) return "Le mod\xE8le de r\xE9sum\xE9 est obligatoire.";
    return "";
  }
  function payload(form) {
    const both = { generationModel: form.generationModel.trim(), summaryModel: form.summaryModel.trim() };
    return form.mode === "api" ? { mode: "api", url: form.url.trim(), key: form.key, ...both } : { mode: "binary", binary: form.binary, ...both };
  }
  function commandBody(dataCapability, body, roles) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, payload: body, caller: { by: "dm", from: "AiSettings", roles } };
  }
  var VIOLATIONS = {
    "api-type-recognized": "type d\u2019API non reconnu",
    "mode-fields": "les champs ne correspondent pas au mode choisi",
    "single-settings-row": "les deux mod\xE8les sont obligatoires",
    "text-only-payload": "seul du texte est accept\xE9"
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

  // apps/ai/ai-settings/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var attr = (name, value) => value === "" ? "" : ` ${name}="${esc(value)}"`;
  function input(p) {
    return `<div class="kit-input is-wide"><label>${esc(p.label)}<input type="${p.type ?? "text"}" id="${p.id}"${attr("value", p.value)}${attr("placeholder", p.placeholder ?? "")}><small>${esc(p.hint)}</small></label></div>`;
  }
  function skeleton() {
    const reads = `data-reads="${GET_SETTINGS}"`;
    return `<div class="mf-ai layout-2" id="ai-settings"><form class="panel" id="ai-settings-form" aria-labelledby="ai-settings-title"><h2 id="ai-settings-title">Fournisseur de g\xE9n\xE9ration</h2><div id="ai-provider" ${reads}></div><div class="mf-ai__fields" id="ai-fields" ${reads}></div><div class="mf-ai__detected" id="ai-detected" ${reads} aria-live="polite"></div><div class="row" id="ai-actions"><div class="kit-button kit-button--bare"><button type="button" class="secondary" id="ai-test" data-writes="${TEST_CONNECTION}">Tester la connexion</button><button type="button" class="primary" id="ai-save" data-writes="${SAVE_SETTINGS}">Enregistrer</button></div></div><div class="mf-ai__result" id="ai-test-result" ${reads} aria-live="polite"></div></form><aside class="panel" id="ai-privacy" aria-labelledby="ai-privacy-title"><h2 id="ai-privacy-title">Ce qui sort de la machine</h2><p class="muted">La transcription et la d\xE9tection restent locales. Seul du texte part vers le fournisseur choisi.</p><section id="ai-privacy-local" class="stack"><h3>Reste sur cette machine</h3><ul class="list" id="ai-local-list" ${reads}></ul></section><section id="ai-privacy-sent" class="stack"><h3>Part vers le fournisseur, en texte</h3><ul class="list" id="ai-sent-list" ${reads}></ul></section></aside></div>`;
  }
  function modeChoice(form) {
    const option = (value, label, hint) => `<label><input type="radio" name="ai-mode" value="${value}"${form.mode === value ? " checked" : ""}><span>${label}</span><small>${esc(hint)}</small></label>`;
    return '<div class="kit-radio"><fieldset id="ai-mode-choice"><legend>Mode de connexion</legend>' + option("api", "API", "une URL, le type d\u2019API est d\xE9tect\xE9") + option("binary", "Binaire", "un programme sur cette machine") + "</fieldset></div>";
  }
  function keyPlaceholder(form, settings) {
    const saved = settings.provider === null ? void 0 : settings.providers[settings.provider];
    return saved !== void 0 && saved.url === form.url.trim() && saved.key || "Aucune cl\xE9";
  }
  function fields(form, settings) {
    const models2 = input({ id: "ai-generation-model", label: "Mod\xE8le de g\xE9n\xE9ration", value: form.generationModel, hint: "Statblocks, backgrounds, briefs et notes" }) + input({ id: "ai-summary-model", label: "Mod\xE8le de r\xE9sum\xE9", value: form.summaryModel, hint: "R\xE9sum\xE9 du journal \xE0 la cl\xF4ture de s\xE9ance" });
    if (form.mode === "api") {
      return input({ id: "ai-url", label: "URL de l\u2019API", value: form.url, hint: "Le type d\u2019API (serveur maison ou cloud) est d\xE9tect\xE9 \xE0 partir de l\u2019URL" }) + input({ id: "ai-key", label: "Cl\xE9 d\u2019API", value: form.key, type: "password", placeholder: keyPlaceholder(form, settings), hint: "Facultative selon l\u2019API. Stock\xE9e sur cette machine uniquement, jamais renvoy\xE9e \xE0 l\u2019\xE9cran : \xE0 saisir de nouveau pour tester ou enregistrer" }) + models2;
    }
    const option = (value, label) => `<option value="${value}"${form.binary === value ? " selected" : ""}>${label}</option>`;
    return `<div class="kit-select is-wide"><span id="ai-binary-label">Binaire</span><select id="ai-binary" aria-labelledby="ai-binary-label">${option("opencode", "opencode")}${option("claude", "Claude Code")}</select><small>Programme lanc\xE9 sur cette machine</small></div>` + models2;
  }
  function detected(label) {
    return label === null ? "" : `<div class="kit-badge"><span class="success" id="ai-detected-badge">Type d\u2019API d\xE9tect\xE9 : ${esc(label)}</span></div>`;
  }
  var when = (at) => {
    const date = new Date(at);
    return Number.isNaN(date.getTime()) ? at : date.toLocaleString("fr-FR", { dateStyle: "long", timeStyle: "short" });
  };
  function lastTestText(test) {
    return test.ok ? `Dernier test r\xE9ussi le ${when(test.at)}${test.latency === null ? "" : ` \u2014 r\xE9ponse en ${test.latency}`}.` : `Dernier test \xE9chou\xE9 le ${when(test.at)} : ${test.message || "connexion impossible"}`;
  }
  function result(outcome) {
    switch (outcome.kind) {
      case "none":
        return "";
      case "busy":
        return `<p class="muted">${esc(outcome.text)}</p>`;
      case "last":
        return `<p class="${outcome.test.ok ? "is-ok" : "is-error"}" id="ai-test-message">${esc(lastTestText(outcome.test))}</p>`;
      case "ok":
      case "error":
        return `<p class="${outcome.kind === "ok" ? "is-ok" : "is-error"}" id="ai-test-message">${esc(outcome.text)}</p>`;
    }
  }
  var items = (list) => list.map((item) => `<li>${esc(item)}</li>`).join("");

  // apps/ai/ai-settings/src/style.ts
  var STYLE = `
.mf-ai { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-ai *, .mf-ai *::before, .mf-ai *::after { box-sizing: inherit; }
.mf-ai.layout-2 { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
@media (max-width: 960px) { .mf-ai.layout-2 { grid-template-columns: minmax(0, 1fr); } }
.mf-ai h2, .mf-ai h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-ai h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-ai h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-ai p { margin: 0; }
.mf-ai .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-ai .muted { color: var(--color-text-muted); }
.mf-ai .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-ai .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-ai .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-ai .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-ai .kit-input.is-wide, .mf-ai .kit-select.is-wide { max-width: none; }
.mf-ai .kit-select small { display: block; margin-top: var(--space-2); font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-ai .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-ai .kit-select select:hover { border-color: var(--color-primary); }
.mf-ai .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-ai__fields { display: grid; grid-template-columns: repeat(auto-fit, minmax(calc(var(--space-5) * 6), 1fr)); gap: var(--space-3); }
.mf-ai__detected { min-height: var(--space-4); }
.mf-ai__result { min-height: var(--space-4); }
.mf-ai__result p { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-ai__result .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-ai__result .is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-ai .list li::before { content: "\u2022 "; color: var(--color-secondary); }
.mf-ai #ai-local-list li::before { content: "\u2713 "; color: var(--color-success); }
`;

  // apps/ai/ai-settings/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="AiSettings"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "AiSettings";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, _props, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`AiSettings: #${id} is not in the card`);
      return found;
    };
    let settings = parseSettings(void 0);
    let form = initialForm(settings);
    let outcome = { kind: "none" };
    let known = null;
    const paintDetected = () => {
      const shown2 = form.mode === "api" && known !== null && known.url === form.url.trim();
      part("ai-detected").innerHTML = detected(shown2 ? known?.label ?? null : null);
    };
    const paintResult = () => {
      part("ai-test-result").innerHTML = result(outcome);
    };
    const paintFields = () => {
      part("ai-fields").innerHTML = fields(form, settings);
      paintDetected();
    };
    const paintAll = () => {
      part("ai-provider").innerHTML = modeChoice(form);
      paintFields();
      paintResult();
      part("ai-local-list").innerHTML = items(settings.local);
      part("ai-sent-list").innerHTML = items(settings.sent);
    };
    const say = (next) => {
      outcome = next;
      paintResult();
    };
    const lock = (locked) => {
      for (const control of host2.querySelectorAll("#ai-settings-form input, #ai-settings-form select, #ai-settings-form button")) control.disabled = locked;
    };
    const read = async () => parseSettings((await shell2.capability(GET_SETTINGS, void 0)).data);
    const refresh = async () => {
      try {
        settings = await read();
        return true;
      } catch {
        return false;
      }
    };
    async function send(dataCapability, roles) {
      const answer = await shell2.command(commandBody(dataCapability, payload(form), roles));
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    let busy = false;
    async function act(wrong, run) {
      if (busy) return;
      const why = problem(form);
      if (why) return say({ kind: "error", text: why });
      busy = true;
      lock(true);
      try {
        await run();
      } catch {
        say({ kind: "error", text: wrong });
      } finally {
        busy = false;
        lock(false);
      }
    }
    const testConnection = () => act("Le test de connexion a \xE9chou\xE9.", async () => {
      const before = settings.lastTest;
      say({ kind: "busy", text: "Test en cours\u2026" });
      const state = await send(TEST_CONNECTION, []);
      if (state.status === "rejected") return say({ kind: "error", text: `Test refus\xE9 : ${reasons(state.violations)}.` });
      if (state.status !== "applied") return say({ kind: "error", text: `Le test n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).` });
      if (!await refresh()) return say({ kind: "error", text: "Le test a eu lieu, mais son r\xE9sultat n\u2019a pas pu \xEAtre lu." });
      const test = settings.lastTest;
      if (test === null || before !== null && before.at === test.at && before.ok === test.ok && before.message === test.message) {
        return say({ kind: "error", text: "Le r\xE9sultat du test n\u2019est pas encore disponible." });
      }
      if (form.mode === "api") {
        const label = testedTypeLabel(settings);
        const url = form.url.trim();
        known = label !== null ? { url, label } : known?.url === url ? null : known;
        paintDetected();
      }
      say({ kind: test.ok ? "ok" : "error", text: test.message || (test.ok ? "Connexion r\xE9ussie." : "Connexion impossible.") });
    });
    const saveSettings = () => act("L\u2019enregistrement a \xE9chou\xE9.", async () => {
      say({ kind: "busy", text: "Enregistrement\u2026" });
      const state = await send(SAVE_SETTINGS, ["role:approver"]);
      if (state.status === "rejected") return say({ kind: "error", text: `Enregistrement refus\xE9 : ${reasons(state.violations)}.` });
      if (state.status !== "applied") return say({ kind: "error", text: `L\u2019enregistrement n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).` });
      form = { ...form, key: "" };
      const read2 = await refresh();
      const label = savedTypeLabel(settings);
      if (read2 && form.mode === "api" && label !== null) known = { url: form.url.trim(), label };
      paintFields();
      say(read2 ? { kind: "ok", text: "Param\xE8tres enregistr\xE9s." } : { kind: "ok", text: "Param\xE8tres enregistr\xE9s, mais ils n\u2019ont pas pu \xEAtre relus." });
    });
    host2.addEventListener("submit", (event) => event.preventDefault());
    host2.addEventListener("change", (event) => {
      const target = event.target;
      if (target?.name === "ai-mode") form = withMode(settings, form, target.value === "binary" ? "binary" : "api");
      else if (target?.id === "ai-binary") form = withBinary(settings, form, target.value === "claude" ? "claude" : "opencode");
      else return;
      say({ kind: "none" });
      paintFields();
    });
    host2.addEventListener("input", (event) => {
      const target = event.target;
      const value = target?.value ?? "";
      if (target?.id === "ai-url") {
        form = { ...form, url: value };
        paintDetected();
        host2.querySelector("#ai-key")?.setAttribute("placeholder", keyPlaceholder(form, settings));
      } else if (target?.id === "ai-key") form = { ...form, key: value };
      else if (target?.id === "ai-generation-model") form = { ...form, generationModel: value };
      else if (target?.id === "ai-summary-model") form = { ...form, summaryModel: value };
    });
    host2.addEventListener("click", (event) => {
      const button = event.target?.closest("button");
      if (!button) return;
      if (button.dataset.writes === TEST_CONNECTION) void testConnection();
      else if (button.dataset.writes === SAVE_SETTINGS) void saveSettings();
    });
    paintAll();
    lock(true);
    read().then(
      (loaded) => {
        settings = loaded;
        form = initialForm(settings);
        const label = savedTypeLabel(settings);
        known = label === null ? null : { url: form.url.trim(), label };
        outcome = settings.lastTest ? { kind: "last", test: settings.lastTest } : { kind: "none" };
        paintAll();
        lock(false);
      },
      () => {
        say({ kind: "error", text: "Impossible de lire les param\xE8tres IA." });
        lock(false);
      }
    );
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
  function ruleHolds(rule, payload2) {
    const m = /^\s*([A-Za-z_][\w.]*)\s*(<=|>=|==|!=|<|>|=)\s*(.+?)\s*$/.exec(String(rule || ""));
    if (!m) return true;
    const [, field, op, raw] = m;
    if (!(field in payload2)) return true;
    const left = payload2[field];
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
  function undeclaredKeys(manifest, payload2) {
    const declared = Object.keys(manifest && manifest.payload || {});
    if (!declared.length) return [];
    const id = manifest.target && typeof manifest.target.id === "string" ? manifest.target.id.replace(/^\$+/, "") : null;
    const declares = (key) => declared.some((d) => d === key || d.startsWith(key) && d[key.length] === ".");
    return Object.keys(payload2 || {}).filter((key) => key !== id && !declares(key)).sort();
  }
  function fieldsOf(manifest, payload2) {
    const touches = Array.isArray(manifest && manifest.touches) ? manifest.touches.filter((t) => typeof t === "string") : [];
    return touches.length ? touches.map((t) => t.split(".").pop()) : Object.keys(payload2 || {});
  }
  function reviewNeeded(aggregate, fields2, caller) {
    const declared = aggregate && aggregate.fields || {};
    const roles = caller && Array.isArray(caller.roles) ? caller.roles : [];
    return fields2.filter((f) => Object.hasOwn(declared, f) && declared[f] && declared[f].criticality === "critical").map((f) => [f, declared[f].review || "role:approver"]).filter(([, role]) => !roles.includes(role)).map(([f, role]) => `${f} (${role})`);
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
        for (const problem2 of index.problems || []) console.error(`mock server: ${problem2}`);
      }
      return index && index.manifests || {};
    };
    s.aggregates = async () => {
      const index = await s.product("aggregates.json");
      if (index && !s.aggregatesSaid) {
        s.aggregatesSaid = true;
        for (const problem2 of index.problems || []) console.error(`mock server: ${problem2}`);
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
    const invariants = async (aggregate, payload2) => {
      const out = { reject: [], review: [] };
      if (!aggregate) return out;
      const manifest = await s.aggregate(aggregate);
      for (const inv of manifest && manifest.invariants || []) {
        if (!ruleHolds(inv.rule, payload2)) (inv.onViolation === "review" ? out.review : out.reject).push(inv.id);
      }
      return out;
    };
    const pendingIn = (partition) => [...s.queue.values()].filter((e) => e.partition === partition && !FINAL2.has(e.state)).sort((a, b) => a.position - b.position);
    const declaredAhead = (entry, merging = /* @__PURE__ */ new Set()) => {
      const fields2 = Object.keys(entry.payload).filter((f) => !merging.has(f));
      const ahead = pendingIn(entry.partition).find((e) => e.position < entry.position && e.state !== "parked" && fields2.some((f) => f in e.payload));
      if (ahead) {
        const f = fields2.find((x) => x in ahead.payload);
        return { by: ahead.by, command: ahead.commandId, value: ahead.payload[f], field: f };
      }
      const read2 = entry.basedOn && entry.basedOn.values || {};
      for (const f of fields2) {
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
      const payload2 = body.payload || {};
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
      const derived = Object.keys(payload2).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json2({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload: payload2, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      const keys = known ? undeclaredKeys(known.manifest, payload2) : [];
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
      const verdict = await invariants(aggregate, payload2);
      if (verdict.reject.length || verdict.review.length) {
        entry.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId2, { violations: verdict.reject.concat(verdict.review) });
      }
      warnUndeclared();
      const needed = reviewNeeded(document2, fieldsOf(known && known.manifest, payload2), body.caller);
      const needsReview = verdict.review.length > 0 || needed.length > 0;
      const merging = new Set(Object.keys(declared).filter((f) => declared[f].merge));
      const ahead = declaredAhead(entry, merging);
      if (needsReview) {
        entry.state = "awaiting_review";
        entry.reviewId = `r_${s.next()}`;
        entry.warnings.push(`review ${entry.reviewId} required`);
        const first = fieldsOf(known && known.manifest, payload2)[0];
        const approver = first && declared[first] && declared[first].review || "role:approver";
        s.tell("ReviewRequired", approver, commandId2, { actions: ["approve", "reject"], reason: needed.length ? `critical: ${needed.join(", ")}` : "an invariant wants a review" });
      } else if (ahead && (body.mode || "confirm_on_stale") === "confirm_on_stale") {
        entry.state = "awaiting_confirmation";
        entry.aheadOf = ahead.command;
        entry.warnings.push("a value is declared ahead: confirm or cancel");
        s.tell("ValueDeclaredAhead", by, commandId2, { field: `${partition}.${ahead.field}`, yourValue: payload2[ahead.field], declaredAhead: { value: ahead.value, by: ahead.by, command: ahead.command }, actions: ["confirm_overwrite", "cancel", "edit"] });
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

  // mock/src/AiSettings.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input2, init) => server.fetch(new URL(input2, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
