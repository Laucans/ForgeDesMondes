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

  // mock/src/reads.js
  var bridged = /* @__PURE__ */ new WeakSet();
  var pristine = /* @__PURE__ */ new Map();
  async function instanceOf(server, id) {
    const data = await server.load("table.getBattlemapInstance");
    if (!data || id && data.id !== id) return void 0;
    const store = window.Mock && window.Mock.store;
    if (!pristine.has(data.id)) pristine.set(data.id, structuredClone(data.state));
    if (store && !bridged.has(data)) {
      bridged.add(data);
      data.state = store.get(`map:${data.id}`, data.state);
      store.on(`map:${data.id}`, (state) => {
        if (state && state !== data.state) {
          data.state = state;
          server.bump();
        }
      });
    }
    return data;
  }
  var pristineState = (id) => structuredClone(pristine.get(id));
  function shareInstance(data) {
    if (window.Mock) window.Mock.store.set(`map:${data.id}`, data.state);
  }
  function healthBand(hp, hpMax) {
    if (typeof hp !== "number" || typeof hpMax !== "number") return null;
    const current = Math.max(0, Math.min(hpMax, hp));
    return current === 0 ? "mort" : current === hpMax ? "indemne" : 2 * current > hpMax ? "bless\xE9" : "en sang";
  }
  async function dressed(server, tokens) {
    const characters = await server.load("campaign.listCharacters") || [];
    return tokens.map((t) => {
      const c = characters.find((x) => x.id === t.char);
      return c ? { ...t, initials: c.initials, color: c.color, portrait: c.portrait } : t;
    });
  }
  var READS = {
    // table.getBattlemapInstance (crates/table/capabilities/get-battlemap-instance):
    // the DM sees everything; the players no hidden token, no hit point figure, no
    // note — each visible NPC gives its HealthBand@1 band instead.
    "table.getBattlemapInstance": async (server, { instance, view } = {}) => {
      const data = await instanceOf(server, instance);
      if (!data) return data;
      const dressedTokens = (await dressed(server, data.state.tokens)).map((t) => t.kind === "pnj" ? { ...t, dead: t.hp === 0 } : t);
      if (view !== "player") return { ...data, state: { ...data.state, tokens: dressedTokens } };
      const tokens = dressedTokens.filter((t) => t.visible).map(({ hp, hpMax, ...rest }) => ({ ...rest, healthBand: rest.kind === "pnj" ? healthBand(hp, hpMax) : null }));
      const seen = new Set(tokens.map((t) => t.id));
      const combat = data.state.combat ? { ...data.state.combat, current: seen.has(data.state.combat.current) ? data.state.combat.current : "", order: data.state.combat.order.filter((o) => seen.has(o.id)) } : null;
      return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat } };
    },
    // media.getBattlemap (crates/media/capabilities/get-battlemap): one battlemap
    // by its id, its grid per HexGrid@1 — the opacity a fraction of 1, a hex 5 feet.
    "media.getBattlemap": async (server, { id } = {}) => {
      const map = await server.load("media.getBattlemap");
      if (!map || id && map.id !== id) return void 0;
      return { ...map, grid: { ...map.grid, opacity: map.grid.opacity > 1 ? map.grid.opacity / 100 : map.grid.opacity, feetPerHex: 5 } };
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
  async function onToken(server, target, token, change) {
    const data = await instanceOf(server, target && target.id);
    const found = data && data.state.tokens.find((t) => t.id === token);
    if (!found) return ["instance-isolated"];
    const violations = change(found, data.state);
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
  async function onCombat(server, id, change) {
    const data = await instanceOf(server, id);
    if (!data) return ["instance-isolated"];
    if (!data.state.combat) return ["one-active-combat"];
    const violations = await change(data.state.combat, data.state);
    if (violations) return violations;
    shareInstance(data);
    return void 0;
  }
  var COMMANDS = {
    // table.moveToken (crates/table/data-capabilities/move-token): (q, r) is the
    // one position both views read.
    "table.moveToken": (server, { payload: payload2, targetId }) => onToken(server, { id: targetId }, payload2.token, (t) => {
      t.q = payload2.q;
      t.r = payload2.r;
    }),
    // table.renameToken: only that token's name; an empty name is refused.
    "table.renameToken": (server, { payload: payload2, targetId }) => onToken(server, { id: targetId }, payload2.token, (t) => {
      if (!String(payload2.name || "").trim()) return ["name-required"];
      t.name = String(payload2.name).trim();
      return void 0;
    }),
    // table.setTokenVisibility: hidden tokens keep their place; the players do not read them.
    "table.setTokenVisibility": (server, { payload: payload2, targetId }) => onToken(server, { id: targetId }, payload2.token, (t) => {
      t.visible = payload2.visible === true;
    }),
    // table.setTokenCondition: the badges of that token alone.
    "table.setTokenCondition": (server, { payload: payload2, targetId }) => onToken(server, { id: targetId }, payload2.token, (t) => {
      t.states = [...payload2.conditions || []];
    }),
    // table.removeToken: the token leaves the map.
    "table.removeToken": (server, { payload: payload2, targetId }) => onToken(server, { id: targetId }, payload2.token, (t, state) => {
      state.tokens = state.tokens.filter((x) => x !== t);
    }),
    // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
    // hp-change-one-token — the targeted token only; pj-hp-not-tracked — a PC has
    // no hit points; the current ones stay within 0 and the maximum, and an NPC
    // is dead iff they are 0 (dead-derived).
    "table.changeHitPoints": (server, { payload: payload2, targetId }) => onToken(server, { id: targetId }, payload2.token, (t) => {
      if (t.kind !== "pnj" || typeof t.hp !== "number") return ["pj-hp-not-tracked"];
      t.hp = Math.max(0, Math.min(t.hpMax, t.hp + Math.trunc(Number(payload2.delta) || 0)));
      t.dead = t.hp === 0;
      return void 0;
    }),
    // table.startCombat: the NPC roll a d20 plus the DEX modifier of their sheet,
    // the DM enters the PCs' initiative; the order is the initiative, then the DEX,
    // then at random (initiative-tiebreak). A hidden token is in the order and keeps its rank.
    "table.startCombat": async (server, { payload: payload2, targetId }) => {
      const data = await instanceOf(server, targetId);
      if (!data) return ["instance-isolated"];
      if (data.state.combat) return ["one-active-combat"];
      const characters = await server.load("campaign.listCharacters") || [];
      const rolls = payload2.rolls || {};
      const entered = payload2.initiative || {};
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
    "table.nextTurn": (server, { targetId }) => onCombat(server, targetId, (combat, state) => {
      advance(combat, state);
      return void 0;
    }),
    // table.reorderInitiative: the participant takes the rank `position`; nobody else changes relative rank.
    "table.reorderInitiative": (server, { payload: payload2, targetId }) => onCombat(server, targetId, (combat) => {
      const from = combat.order.findIndex((o) => o.id === payload2.token);
      if (from < 0) return ["instance-isolated"];
      const [moved] = combat.order.splice(from, 1);
      combat.order.splice(Math.max(0, Math.min(combat.order.length, Number(payload2.position) || 0)), 0, moved);
      return void 0;
    }),
    // table.addParticipant: a token joins the running order at the place its initiative earns.
    "table.addParticipant": (server, { payload: payload2, targetId }) => onCombat(server, targetId, async (combat, state) => {
      const token = state.tokens.find((t) => t.id === payload2.token);
      if (!token || combat.order.some((o) => o.id === token.id)) return ["instance-isolated"];
      const dex = dexModifier(await server.load("campaign.listCharacters") || [], token.char);
      const roll = Number(payload2.roll);
      const init = token.kind === "pnj" ? roll + dex : Number(payload2.initiative);
      if (!(init >= 1) || token.kind === "pnj" && !(roll >= 1 && roll <= 20)) return ["npc-initiative-bounds"];
      const at = combat.order.findIndex((o) => o.init < init || o.init === init && o.dex < dex);
      combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token.id, init, dex });
      return void 0;
    }),
    // table.removeParticipant: the token leaves the order, not the map; the turn passes on if it was its turn.
    "table.removeParticipant": (server, { payload: payload2, targetId }) => onCombat(server, targetId, (combat, state) => {
      if (!combat.order.some((o) => o.id === payload2.token)) return ["instance-isolated"];
      const rest = { ...combat, order: combat.order.filter((o) => o.id !== payload2.token) };
      if (combat.current === payload2.token) {
        const gone = { ...state, tokens: state.tokens.map((t) => t.id === payload2.token ? { ...t, dead: true } : t) };
        advance(combat, gone);
        rest.round = combat.round;
        rest.current = combat.current === payload2.token ? rest.order[0] && rest.order[0].id || "" : combat.current;
      }
      if (rest.order.length) Object.assign(combat, rest);
      else state.combat = null;
      return void 0;
    }),
    // table.endCombat: the combat ends; the tokens stay as they are.
    "table.endCombat": (server, { targetId }) => onCombat(server, targetId, (combat, state) => {
      state.combat = null;
      return void 0;
    }),
    // table.resetBattlemap: state-persisted-reset — the instance goes back to the state it was opened with: no combat, no initiative.
    "table.resetBattlemap": async (server, { targetId }) => {
      const data = await instanceOf(server, targetId);
      if (!data) return ["instance-isolated"];
      data.state = pristineState(data.id);
      data.pjInitiative = {};
      shareInstance(data);
      return void 0;
    },
    // table.placeTokens: one character dropped, once or « × n »; the new tokens
    // are numbered after those already taken for it (multi-drop-numbering), a
    // monster takes its hit points from its sheet, a PC has one token.
    "table.placeTokens": async (server, { payload: payload2, targetId }) => {
      const data = await instanceOf(server, targetId);
      const character = (await server.load("campaign.listCharacters") || []).find((c) => c.id === payload2.character);
      if (!data || !character) return ["instance-isolated"];
      const { tokens } = data.state;
      const mine = tokens.filter((t) => t.char === character.id);
      const pnj = character.kind === "pnj";
      if (!pnj && mine.length) return ["token-roster"];
      const count = pnj ? Math.max(1, Math.min(12, Number(payload2.count) || 1)) : 1;
      const base = character.tokenName || String(character.name).split(" ")[0];
      let num = Math.max(0, ...mine.map((t) => t.num || 0));
      const numbered = pnj && (count > 1 || mine.length > 0);
      for (const hex of freeHexes({ q: payload2.q, r: payload2.r }, tokens, count)) {
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
    // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
    // the battlemap becomes the player view's base, framed as the DM framed it.
    "live.showOnPlayerView": async (server, { payload: payload2 }) => {
      const data = await instanceOf(server, payload2.instance);
      if (payload2.kind !== "battlemap" || !data) return ["base-coherent"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.base = { kind: "battlemap", id: data.id };
        diffusion.view = { z: payload2.zoom, px: payload2.panX, py: payload2.panY };
        store.set("diffusion", diffusion);
      }
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
    const declaredAhead = (entry) => {
      const fields2 = Object.keys(entry.payload);
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
      const commandId2 = uuid();
      const aggregate = target.aggregate || null;
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
      const critical = (await s.load(`aggregates/${aggregate}`) || { fields: {} }).fields || {};
      const needsReview = verdict.review.length > 0 || Object.keys(payload2).some((f) => critical[f] && critical[f].criticality === "critical");
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
