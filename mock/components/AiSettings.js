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
  function createShell(fetcher = (input2, init) => fetch(input2, init)) {
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
      const shown = form.mode === "api" && known !== null && known.url === form.url.trim();
      part("ai-detected").innerHTML = detected(shown ? known?.label ?? null : null);
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
    "session.addPresentCharacter": async (server, { payload: payload2 }) => {
      const session = await server.load("session.getSession");
      const character = (await server.load("campaign.listCharacters")).find((c) => c.id === payload2.character);
      if (!character || character.kind !== "pj") return ["present-only-pj"];
      if (!session.present.includes(character.id)) session.present.push(character.id);
      return void 0;
    },
    "session.removePresentCharacter": async (server, { payload: payload2 }) => {
      const session = await server.load("session.getSession");
      session.present = session.present.filter((id) => id !== payload2.character);
      return void 0;
    },
    // session.addScene: a scene at the end, its rank the next one.
    "session.addScene": async (server, { payload: payload2 }) => {
      const scenes = await server.load("session.listScenes");
      const next = Math.max(0, ...scenes.map((sc) => sc.order)) + 1;
      scenes.push({ id: `sc-${server.uuid().slice(0, 8)}`, order: next, title: String(payload2.title || "Nouvelle sc\xE8ne"), places: [], characters: [], events: [], media: [], toCreate: [], battlemap: null, notes: "" });
      return void 0;
    },
    // session.reorderScenes: every rank rewritten at once. `scene-order-contiguous`:
    // the ranks must be 1 to n, each scene once.
    "session.reorderScenes": async (server, { payload: payload2 }) => {
      const scenes = await server.load("session.listScenes");
      const order = Array.isArray(payload2.order) ? payload2.order : [];
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
    "media.uploadMedia": async (server, { payload: payload2 }) => {
      const { kind, place = null } = payload2;
      if (kind !== "battlemap" && kind !== "illustration") return ["media-kind"];
      if (place && !(await server.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["media-place-is-lieu"];
      const media = await server.load("media");
      const base = { id: server.uuid(), title: String(payload2.title || "").trim() || "Sans titre", file: payload2.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      if (kind === "battlemap") media.battlemaps.push({ ...base, width: payload2.width || 0, height: payload2.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 } });
      else media.illustrations.push(base);
      return void 0;
    },
    // media.saveGridSettings (crates/media/data-capabilities/save-grid-settings):
    // a battlemap's grid kept — the six fields of HexGrid@1, the opacity a
    // fraction of 1 where the mock-up's fixtures write a percentage. A hex size
    // of 0 or an opacity outside 0..1 is refused by `grid-values-range`; an
    // illustration or an unknown id has no grid (`grid-only-for-battlemap`).
    "media.saveGridSettings": async (server, { payload: payload2, targetId }) => {
      const battlemap = (await server.load("media")).battlemaps.find((b) => b.id === targetId);
      if (!battlemap) return ["grid-only-for-battlemap"];
      const { size, ox, oy, orient, color, opacity } = payload2;
      if (!(size > 0) || !(opacity >= 0 && opacity <= 1)) return ["grid-values-range"];
      battlemap.grid = { size, ox, oy, orient, color, opacity: Math.round(opacity * 100) };
      return void 0;
    },
    // media.setMediaPlace (crates/media/data-capabilities/set-media-place): a
    // media — battlemap or illustration — attached to one of the campaign's
    // places, or detached with `place: null`, and nothing else of it changes. A
    // place that is no lieu the fixtures list is refused by
    // `media-place-is-lieu`; an unknown media is `not-found`.
    "media.setMediaPlace": async (server, { payload: payload2, targetId }) => {
      const id = targetId || payload2.media;
      const listed = [await server.load("media"), await server.load("media.listMedia")];
      const found = listed.flatMap((media) => media.battlemaps.concat(media.illustrations)).filter((m) => m.id === id);
      if (!found.length) return ["not-found"];
      const place = payload2.place ?? null;
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
    "table.createBattlemapInstance": async (server, { payload: payload2 }) => {
      const session = await server.load("session.getSession");
      const scene = (await server.load("session.listScenes")).find((sc) => sc.id === payload2.scene);
      const map = (await server.load("media.listMedia")).battlemaps.find((m) => m.id === payload2.battlemap);
      if (!session || session.id !== payload2.session || !scene || !map) return ["not-found"];
      if (scene.battlemap) return ["one-instance-per-scene"];
      const id = `bmi-${map.id}-${scene.id}`;
      server.instances = server.instances || /* @__PURE__ */ new Map();
      server.instances.set(id, { id, battlemap: map.id, session: payload2.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
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
    "table.placeTokens": async (server, { payload: payload2, targetId }) => {
      const count = payload2.count ?? 1;
      if (!Number.isInteger(payload2.q) || !Number.isInteger(payload2.r)) return ["single-position-source"];
      if (!Number.isInteger(count) || count < 1 || count > 12) return ["multi-drop-numbering"];
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instance = id === fixture.id ? fixture : server.instances && server.instances.get(id);
      const character = (await server.load("campaign.listCharacters")).find((c) => c.id === payload2.character);
      if (!instance || !character || !["pj", "pnj"].includes(character.kind)) return ["not-found"];
      const tokens = instance.state.tokens;
      const placed = tokens.filter((t) => t.char === character.id);
      const pnj = character.kind === "pnj";
      if (!pnj) {
        const session = await server.load("session.getSession");
        if (count !== 1 || placed.length || !session.present.includes(character.id)) return ["token-roster"];
      }
      let number = Math.max(0, ...placed.map((t) => t.num || 0));
      for (let i = 0; i < count; i++) {
        const num = pnj ? ++number : null;
        tokens.push({
          id: server.uuid(),
          char: character.id,
          kind: character.kind,
          name: pnj ? `${character.name.trim()} ${num}` : character.name.trim(),
          num,
          q: payload2.q,
          r: payload2.r,
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
    "table.renameToken": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => String(a ?? "").toLowerCase() === String(b ?? "").toLowerCase();
      const name = typeof payload2.name === "string" ? payload2.name.trim() : "";
      if (!name) return ["multi-drop-numbering"];
      const tokenId = payload2.token ?? targetId;
      const instance = payload2.token != null && targetId && !same2(targetId, tokenId) ? targetId : null;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const inst = instances.find((i) => (instance == null || same2(i.id, instance)) && i.state.tokens.some((t) => same2(t.id, tokenId)));
      if (!inst) return ["not-found"];
      inst.state.tokens.find((t) => same2(t.id, tokenId)).name = name;
      return void 0;
    },
    "campaign.restoreEntityVersion": async (server, { payload: payload2, targetId }) => {
      const history = (await server.load("campaign.listEntityVersions"))[targetId];
      const target = history && history.versions.find((v) => v.v === payload2.version);
      if (!target) return ["not-found"];
      const sheet = (upTo) => history.versions.slice().sort((a, b) => a.v - b.v).filter((v) => v.v <= upTo).reduce((fields2, v) => {
        for (const c of v.changes) fields2[c.field] = c.new;
        return fields2;
      }, {});
      const before = sheet(history.versions[0].v);
      let after = sheet(target.v);
      if (Array.isArray(payload2.changes)) {
        const undone = (history.versions.find((v) => v.v === payload2.version + 1) || { changes: [] }).changes;
        after = { ...before };
        for (const i of payload2.changes) if (undone[i]) after[undone[i].field] = undone[i].old;
      }
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].sort().filter((field) => (before[field] ?? "") !== (after[field] ?? "")).map((field) => ({ field, old: before[field] ?? "", new: after[field] ?? "" }));
      history.versions.unshift({ v: history.versions[0].v + 1, date: (/* @__PURE__ */ new Date()).toISOString(), origin: "manuelle", source: "restauration", restoredFrom: target.v, changes });
      return void 0;
    },
    // campaign.exportCampaign (crates/dataguard/data-capabilities/export-campaign):
    // stamps the campaign's last export with today's date and changes nothing
    // else (`export-stamps-date`). An unknown campaign is `not-found`.
    "campaign.exportCampaign": async (server, { payload: payload2 }) => {
      const campaign = await server.load("campaign.getCampaign");
      if (!campaign || campaign.id !== payload2.id) return ["not-found"];
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
    "ai.saveSettings": async (server, { payload: payload2 }) => {
      const settings = await server.load("ai.getSettings");
      const models2 = { generationModel: payload2.generationModel, summaryModel: payload2.summaryModel };
      if (!String(models2.generationModel || "") || !String(models2.summaryModel || "")) return ["single-settings-row"];
      if (payload2.mode === "api") {
        const url = String(payload2.url || "");
        const type = apiType(settings, url);
        if (!type) return ["api-type-recognized"];
        const key = String(payload2.key || "");
        settings.providers[type] = { ...settings.providers[type], url, ...models2, key: key ? `\u2022\u2022\u2022\u2022${key.slice(-4)}` : "" };
        Object.assign(settings, { mode: "api", provider: type, binary: null });
        return void 0;
      }
      if (payload2.mode === "binary") {
        const binary = { claude: "claude-code", "claude-code": "claude-code", opencode: "opencode" }[payload2.binary];
        if (!binary) return ["mode-fields"];
        Object.assign(settings, { mode: "binary", provider: binary, binary: { binary, ...models2 } });
        return void 0;
      }
      return ["mode-fields"];
    },
    // ai.testConnection (crates/ai/data-capabilities/test-connection): the
    // configuration on screen tested and nothing of it saved — only `lastTest`
    // changes (`test-does-not-save`). Its type is detected as a save's; a URL
    // of no known type is the active provider's failed test, « type d'API non
    // reconnu ». The key is never kept.
    "ai.testConnection": async (server, { payload: payload2 }) => {
      const settings = await server.load("ai.getSettings");
      const at = (/* @__PURE__ */ new Date()).toISOString();
      const provider = payload2.mode === "api" ? apiType(settings, String(payload2.url || "")) : payload2.mode === "binary" ? { claude: "claude-code", "claude-code": "claude-code", opencode: "opencode" }[payload2.binary] || null : null;
      if (!provider) {
        settings.lastTest = { ok: false, at, provider: settings.provider, latency: null, message: "type d'API non reconnu" };
        return void 0;
      }
      const message = payload2.mode === "api" && settings.testResults?.[provider]?.message || `Connexion r\xE9ussie (binaire ${provider})`;
      const latency = message.match(/répond en ([\d,]+ s)/)?.[1] || "0,01 s";
      settings.lastTest = { ok: true, at, provider, latency, message };
      return void 0;
    },
    // ai.extractFromNotes (crates/ai/data-capabilities/extract-from-notes): the
    // note analysed into proposals, nothing created (`no-creation-before-
    // validation`). The mock's AI proposes what the fixture NoteImport lists; a
    // proposal that names an existing element starts unchecked.
    "ai.extractFromNotes": async (server, { payload: payload2 }) => {
      const text2 = String(payload2.text || "");
      const { proposals } = await server.load("NoteImport");
      server.noteText = text2;
      server.noteProposals = proposals.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
      return void 0;
    },
    // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
    // one element per accepted proposal, the raw note kept as an import listed
    // newest first, the proposals no longer awaiting.
    "campaign.createEntitiesFromNote": async (server, { payload: payload2 }) => {
      const proposals = server.noteProposals || [];
      const accepted = Array.isArray(payload2.accepted) ? payload2.accepted : [];
      const created = proposals.filter((p) => accepted.includes(p.id));
      const fixture = await server.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture.imports;
      const note = server.noteText || "";
      imports.unshift({ id: `imp-${server.uuid()}`, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), title: note.split("\n")[0].replace(/[.\s]+$/, "") || "Notes import\xE9es", chars: note.length, proposed: proposals.length, created: created.length });
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
    "campaign.saveEntity": async (server, { payload: payload2 }) => {
      const given = payload2.entity || payload2;
      const lists = await Promise.all(Object.values(LISTS).filter((v2, i, all2) => all2.indexOf(v2) === i).map(async (name) => [name, await server.load(name) || []]));
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
      const id = before.id || given.id || server.uuid();
      after.id = id;
      const list = await server.load(LISTS[kind]) || [];
      if (found && found.name !== LISTS[kind]) {
        const old = lists.find(([name]) => name === found.name)[1];
        old.splice(old.indexOf(before), 1);
      }
      const at = list.indexOf(before);
      if (at >= 0) list[at] = after;
      else list.push(after);
      const history = await server.load("campaign.listEntityVersions") || {};
      const entry = history[id] || (history[id] = { name: after.name, versions: [] });
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
    "table.moveToken": async (server, { payload: payload2, targetId }) => {
      const axis = (n) => Number.isInteger(n) && n >= -(2 ** 31) && n < 2 ** 31;
      if (!axis(payload2.q) || !axis(payload2.r)) return ["single-position-source"];
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => t.id === payload2.token));
      if (!holder) return ["not-found"];
      if (holder.id !== id) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => t.id === payload2.token);
      token.q = payload2.q;
      token.r = payload2.r;
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
    "table.setTokenVisibility": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      if (typeof payload2.visible !== "boolean") return ["players-see-band-only"];
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload2.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      holder.state.tokens.find((t) => same2(t.id, payload2.token)).visible = payload2.visible;
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
    "table.setTokenCondition": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const state = typeof payload2.state === "string" ? payload2.state.trim() : "";
      if (!state || state.includes("\0") || typeof payload2.on !== "boolean") return ["state-persisted-reset"];
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload2.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => same2(t.id, payload2.token));
      const held = token.states || [];
      token.states = payload2.on ? held.includes(state) ? held : [...held, state] : held.filter((s) => s !== state);
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
    "table.changeHitPoints": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      if (!Number.isInteger(payload2.delta)) return ["pj-hp-not-tracked"];
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload2.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => same2(t.id, payload2.token));
      if (!Number.isInteger(token.hp) || !Number.isInteger(token.hpMax)) return ["pj-hp-not-tracked"];
      token.hp = Math.max(0, Math.min(token.hpMax, token.hp + payload2.delta));
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
    "table.removeToken": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const named = payload2.token != null;
      const tokenId = named ? payload2.token : targetId;
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
    "table.nextTurn": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload2?.instance;
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
    "table.startCombat": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      if (instance.state.combat) return ["one-active-combat"];
      const tokens = instance.state.tokens;
      if (!tokens.length) return ["npc-initiative-bounds"];
      const figure = (figures, key) => Object.entries(figures || {}).find(([k]) => same2(k, key))?.[1];
      if (Object.keys(payload2.rolls || {}).some((k) => !tokens.some((t) => same2(t.id, k) && t.kind === "pnj"))) return ["npc-initiative-bounds"];
      const characters = await server.load("campaign.listCharacters");
      const seated = [];
      for (const t of tokens) {
        if (t.kind !== "pj" && t.kind !== "pnj") return ["npc-initiative-bounds"];
        const score = characters.find((c) => same2(c.id, t.char))?.stats?.DEX;
        const pc = t.kind === "pj";
        const dexScore = pc ? score ?? null : score ?? 10;
        const roll = pc ? null : figure(payload2.rolls, t.id) ?? 1 + Math.floor(Math.random() * 20);
        const init = pc ? figure(payload2.initiative, t.char) : roll + Math.floor((dexScore - 10) / 2);
        if (!Number.isInteger(init) || roll !== null && (!Number.isInteger(roll) || roll < 1 || roll > 20)) return ["npc-initiative-bounds"];
        seated.push({ id: t.id, init, roll, dexScore, draw: Math.random() });
      }
      const order = seated.sort((a, b) => b.init - a.init || (b.dexScore ?? -Infinity) - (a.dexScore ?? -Infinity) || b.draw - a.draw).map(({ id: token, init, dexScore }) => ({ id: token, init, dex: dexScore === null ? 0 : Math.floor((dexScore - 10) / 2) }));
      instance.state.combat = { round: 1, current: order[0].id, order };
      return void 0;
    },
    // table.addParticipant (crates/table/data-capabilities/add-participant,
    // Initiative@1): the token `payload.token` of the instance `targetId` (or
    // `payload.instance`) joins its open combat. An NPC rolls a d20 plus its DEX
    // modifier (DEX 10 when its sheet gives none); a PC rolls nothing and takes
    // the initiative `payload.initiative` the DM entered — an NPC is given none
    // (`npc-initiative-bounds`). It goes after the last participant that ranks
    // before it or ties with it, by initiative then DEX, the others keeping their
    // order (`initiative-tiebreak`); the turn and the round stay. A token not on
    // the instance is `not-found`; an instance with no open combat, or a token
    // already in it, is refused by `initiative-tiebreak`, a token that is no PC
    // nor NPC, a PC with no figure or an NPC given one by `npc-initiative-bounds`,
    // as the server refuses them (422). Ids are compared in any case and without
    // the blanks around them, as the server does.
    "table.addParticipant": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      const token = instance?.state.tokens.find((t) => same2(t.id, payload2.token));
      if (!token) return ["not-found"];
      const combat = instance.state.combat;
      if (!combat) return ["initiative-tiebreak"];
      const order = combat.order || [];
      if (order.some((o) => same2(o.id, token.id))) return ["initiative-tiebreak"];
      if (token.kind !== "pj" && token.kind !== "pnj") return ["npc-initiative-bounds"];
      const pc = token.kind === "pj";
      if (pc ? !Number.isInteger(payload2.initiative) : payload2.initiative != null) return ["npc-initiative-bounds"];
      const characters = await server.load("campaign.listCharacters");
      const score = characters.find((c) => same2(c.id, token.char))?.stats?.DEX;
      const dexScore = pc ? score ?? null : score ?? 10;
      const dex = dexScore === null ? 0 : Math.floor((dexScore - 10) / 2);
      const init = pc ? payload2.initiative : 1 + Math.floor(Math.random() * 20) + dex;
      const before = order.findLastIndex((o) => o.init > init || o.init === init && o.dex >= dex);
      order.splice(before + 1, 0, { id: token.id, init, dex });
      combat.order = order;
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
      const history = (await server.load("campaign.listEntityVersions"))[entity];
      if (history) return history;
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
  var json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  var notFound = () => json({ error: "not-found" }, 404);
  var refused = (why) => json({ error: why }, 409);
  var same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  var FINAL2 = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
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
    const invariants = async (aggregate, payload2) => {
      const out = { reject: [], review: [] };
      if (!aggregate) return out;
      const manifest = await s.load(`aggregates/${aggregate}`);
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
      const payload2 = body.payload || {};
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
      const derived = Object.keys(payload2).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload: payload2, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
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
      const verdict = await invariants(aggregate, payload2);
      if (verdict.reject.length || verdict.review.length) {
        entry.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId2, { violations: verdict.reject.concat(verdict.review) });
      }
      const needsReview = verdict.review.length > 0 || Object.keys(payload2).some((f) => declared[f] && declared[f].criticality === "critical");
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
      return json({ result: resultOf(entry) });
    };
    s.fetch = async (input2, init) => {
      await delay(LATENCY_MS);
      const url = new URL(typeof input2 === "string" ? input2 : input2 instanceof URL ? input2.href : input2.url);
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

  // mock/src/AiSettings.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input2, init) => server.fetch(new URL(input2, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
