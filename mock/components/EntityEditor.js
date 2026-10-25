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

  // apps/campaign/entity-editor/src/ids.ts
  var GET_ENTITY = "campaign.getEntity";
  var LIST_PLAYERS = "campaign.listPlayers";
  var LIST_MEDIA = "media.listMedia";
  var SAVE_ENTITY = "campaign.saveEntity";
  var PROPOSE_CHARACTER = "ai.proposeCharacter";
  var IMPORT_CHARACTER = "dndbeyond.importCharacter";
  var RESYNC_CHARACTER = "dndbeyond.resyncCharacter";
  var APPROVER = "role:approver";
  var DATA_CAPABILITY_VERSION = 1;
  var ENTITY_AGGREGATE = "Entity";
  var ENTITY_CONTEXT = "entity";
  var CAMPAIGN_CONTEXT = "campaign";
  var NEW_PREFIX = "new:";
  var EVENT_OPENED = "entity-editor:opened";
  var EVENT_UPDATED = "entity-editor:updated";
  var EVENT_HISTORY = "entity-editor:history";
  var BUS_OPEN = "fdm:open-entity";
  var BUS_QUERY = "fdm:entity-query";
  var BUS_OPENED = "fdm:entity-opened";

  // apps/campaign/entity-editor/src/types.ts
  var ABILITIES = ["FOR", "DEX", "CON", "INT", "SAG", "CHA"];

  // apps/campaign/entity-editor/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var records = (value) => Array.isArray(value) ? value.filter(isRecord) : [];
  var whole = (value, otherwise) => typeof value === "number" && Number.isFinite(value) ? value : otherwise;
  var KINDS = ["pj", "pnj", "lieu", "evenement"];
  var isKind = (value) => typeof value === "string" && KINDS.includes(value);
  var isCharacter = (kind) => kind === "pj" || kind === "pnj";
  var BASE_SCORE = 10;
  var DEFAULT_CROP = { zoom: 100, x: 50, y: 50 };
  var DEFAULT_COLOR = "accent";
  var modifier = (score) => Math.floor((score - BASE_SCORE) / 2);
  function signed(score) {
    const m = modifier(score);
    return m < 0 ? `\u2212${Math.abs(m)}` : `+${m}`;
  }
  function initialsOf(name) {
    const words = name.split(/\s+/).filter((word) => word !== "");
    const letters = words.length === 0 ? [] : words.length === 1 ? [...words[0] ?? ""].slice(0, 2) : [...words[0] ?? ""].slice(0, 1).concat([...words[1] ?? ""].slice(0, 1));
    return letters.join("").toUpperCase().slice(0, 2);
  }
  var hasPortrait = (entity) => entity.portraitMedia === void 0 ? entity.portrait : entity.portraitMedia !== null;
  function tokenHasImage(entity) {
    const token2 = entity.tokenMedia === void 0 ? entity.tokenImage : entity.tokenMedia !== null;
    return hasPortrait(entity) || token2;
  }
  var colorName = (color) => /^[a-z][a-z-]*$/.test(color) ? color : DEFAULT_COLOR;
  function scores(value) {
    const given = isRecord(value) ? value : {};
    return Object.fromEntries(ABILITIES.map((ability) => [ability, whole(given[ability], BASE_SCORE)]));
  }
  function crop(value) {
    const given = isRecord(value) ? value : {};
    return { zoom: whole(given.zoom, DEFAULT_CROP.zoom), x: whole(given.x, DEFAULT_CROP.x), y: whole(given.y, DEFAULT_CROP.y) };
  }
  function parseEntity(data) {
    if (!isRecord(data) || !isKind(data.kind) || text(data.id) === "") return null;
    const ddb2 = isRecord(data.ddb) && text(data.ddb.url) !== "" ? { url: text(data.ddb.url), syncedAt: typeof data.ddb.syncedAt === "string" ? data.ddb.syncedAt : null } : null;
    const features2 = records(data.features).map((feature) => ({ name: text(feature.name), text: text(feature.text) }));
    return {
      id: text(data.id),
      kind: data.kind,
      name: text(data.name),
      aliases: texts(data.aliases),
      description: text(data.description),
      notes: text(data.notes),
      player: text(data.player),
      prompt: text(data.prompt),
      background: text(data.background),
      behavior: text(data.behavior),
      stats: scores(data.stats),
      ca: whole(data.ca, BASE_SCORE),
      pvMax: whole(data.pvMax, 8),
      features: features2,
      color: isCharacter(data.kind) ? colorName(text(data.color)) : "",
      portraitMedia: void 0,
      tokenMedia: void 0,
      portrait: data.portrait === true,
      tokenImage: data.tokenImage === true,
      crop: crop(data.crop),
      ddb: ddb2
    };
  }
  function freshEntity(kind, name = "") {
    return {
      id: "",
      kind,
      name,
      aliases: [],
      description: "",
      notes: "",
      player: "",
      prompt: "",
      background: "",
      behavior: "",
      stats: scores(void 0),
      ca: BASE_SCORE,
      pvMax: 8,
      features: [],
      color: isCharacter(kind) ? DEFAULT_COLOR : "",
      portraitMedia: void 0,
      tokenMedia: void 0,
      portrait: false,
      tokenImage: false,
      crop: { ...DEFAULT_CROP },
      ddb: null
    };
  }
  var parsePlayers = (data) => records(data).filter((player) => text(player.id) !== "").map((player) => ({ id: text(player.id), name: text(player.name) || text(player.id) }));
  var parseIllustrations = (data) => records(isRecord(data) ? data.illustrations : []).filter((media) => text(media.id) !== "").map((media) => ({ id: text(media.id), title: text(media.title) || text(media.id) }));
  function parseTarget(prop) {
    if (typeof prop !== "string" || prop === "") return null;
    if (!prop.startsWith(NEW_PREFIX)) return { id: prop };
    const kind = prop.slice(NEW_PREFIX.length);
    return isKind(kind) ? { fresh: kind } : null;
  }
  var targetProp = (target) => "id" in target ? target.id : `${NEW_PREFIX}${target.fresh}`;
  var sameTarget = (a, b) => a === null || b === null ? a === b : targetProp(a) === targetProp(b);
  var parseAliases = (typed) => typed.split(",").map((alias) => alias.trim()).filter((alias) => alias !== "");
  var FIELDS = ["name", "aliases", "description", "notes", "player", "prompt", "background", "behavior", "ca", "pvMax", "features", "color", "crop"];
  var comparable = (entity) => ({
    kind: entity.kind,
    ...Object.fromEntries(FIELDS.map((field) => [field, entity[field]])),
    stats: entity.stats,
    portraitMedia: entity.portraitMedia,
    tokenMedia: entity.tokenMedia
  });
  function isDirty(draft, stored) {
    return JSON.stringify(comparable(draft)) !== JSON.stringify(comparable(stored));
  }
  function commandBody(dataCapability, payload, target, idempotencyKey) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload,
      target,
      idempotencyKey,
      caller: { by: "dm", from: "EntityEditor", roles: [APPROVER] }
    };
  }
  var entityTarget = (id) => id === "" ? { aggregate: ENTITY_AGGREGATE } : { aggregate: ENTITY_AGGREGATE, id };
  function newId() {
    const crypto2 = globalThis.crypto;
    if (typeof crypto2?.randomUUID === "function") return crypto2.randomUUID();
    const bytes = new Uint8Array(16);
    if (typeof crypto2?.getRandomValues === "function") crypto2.getRandomValues(bytes);
    else bytes.forEach((_, i) => bytes[i] = Math.floor(Math.random() * 256));
    bytes[6] = (bytes[6] ?? 0) & 15 | 64;
    bytes[8] = (bytes[8] ?? 0) & 63 | 128;
    const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }
  function savePayload(entity, campaign, isNew) {
    const sent = {
      id: entity.id,
      kind: entity.kind,
      name: entity.name.trim(),
      aliases: entity.aliases,
      description: entity.description,
      notes: entity.notes
    };
    if (isCharacter(entity.kind)) {
      sent.player = entity.kind === "pj" && entity.player !== "" ? entity.player : null;
      sent.prompt = entity.prompt;
      sent.background = entity.background;
      sent.behavior = entity.behavior;
      sent.stats = entity.stats;
      sent.ca = entity.ca;
      sent.pvMax = entity.pvMax;
      sent.features = entity.features;
      sent.crop = hasPortrait(entity) ? entity.crop : null;
      sent.color = entity.color;
      if (entity.portraitMedia !== void 0) sent.portrait = entity.portraitMedia;
      if (entity.tokenMedia !== void 0) sent.token = entity.tokenMedia;
    }
    return isNew ? { campaign, entity: sent } : { entity: sent };
  }
  var LABELS = {
    name: "Nom",
    aliases: "Alias",
    description: "Description",
    notes: "Notes du MJ",
    player: "Joueur",
    prompt: "Prompt initial",
    background: "Background",
    behavior: "Comportement",
    ca: "CA",
    pvMax: "PV max",
    features: "Sorts et capacit\xE9s",
    color: "Couleur du token",
    crop: "Recadrage du token"
  };
  var shown = (field, entity) => {
    if (field === "aliases") return entity.aliases.join(", ");
    if (field === "features") return entity.features.map((feature) => feature.name).join(", ");
    if (field === "crop") return `${entity.crop.zoom} % \xB7 ${entity.crop.x} / ${entity.crop.y}`;
    if (ABILITIES.includes(field)) return String(entity.stats[field]);
    const value = entity[field];
    return typeof value === "string" || typeof value === "number" ? String(value) : "";
  };
  function changesBetween(before, after) {
    const moved = (field, a, b) => JSON.stringify(a) === JSON.stringify(b) ? [] : [{ field: field in LABELS ? LABELS[field] : field, old: shown(field, before), new: shown(field, after) }];
    return [...FIELDS.flatMap((field) => moved(field, before[field], after[field])), ...ABILITIES.flatMap((ability) => moved(ability, before.stats[ability], after.stats[ability]))];
  }
  function overlay(draft, before, after) {
    const next = structuredClone(draft);
    const set = (key) => {
      next[key] = structuredClone(after[key]);
    };
    for (const field of FIELDS) if (JSON.stringify(before[field]) !== JSON.stringify(after[field])) set(field);
    for (const ability of ABILITIES) if (before.stats[ability] !== after.stats[ability]) next.stats[ability] = after.stats[ability];
    set("portrait");
    set("tokenImage");
    set("ddb");
    return next;
  }
  var VIOLATIONS = {
    "not-found": "l\u2019\xE9l\xE9ment ou sa demande est introuvable",
    "alias-unique-in-campaign": "un nom ou un alias est d\xE9j\xE0 celui d\u2019un autre \xE9l\xE9ment de la campagne",
    "character-is-pj-or-pnj": "seul un PJ a un joueur, et seul un PJ ou un PNJ a une fiche",
    "dex-modifier-derived": "le modificateur se calcule, il ne s\u2019\xE9crit pas",
    "dndbeyond-resync-scope": "la resynchronisation ne touche que les stats, les PV et les sorts",
    "proposal-fills-sheet-fields": "la proposition ne remplit pas les champs de la fiche"
  };
  var reasons = (violations) => violations.map((violation) => VIOLATIONS[violation] ?? violation).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    return { status: text(result.status), violations: texts(result.violations) };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }
  var campaignVariables = (campaign) => campaign === "" ? {} : { campaign };

  // apps/campaign/entity-editor/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var MIC = '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>';
  var PAUSE = '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>';
  var MIC_TEXTS = { idle: "Pr\xEAt \xE0 dicter", recording: "Enregistrement", paused: "En pause" };
  var KIND_LABELS = { pj: "PJ", pnj: "PNJ", lieu: "Lieu", evenement: "\xC9v\xE9nement" };
  function button(p) {
    const writes = p.writes === void 0 ? "" : ` data-writes="${p.writes}"`;
    return `<button type="button" class="${p.variant}" id="${p.id}"${writes}${p.disabled === true ? " disabled" : ""}>${esc(p.label)}</button>`;
  }
  var buttons = (small, ...items) => `<div class="kit-button kit-button--bare${small ? " kit-button--small" : ""}">${items.join("")}</div>`;
  function input(p) {
    const attrs = `${p.min === void 0 ? "" : ` min="${p.min}"`}${p.max === void 0 ? "" : ` max="${p.max}"`}${p.placeholder === void 0 ? "" : ` placeholder="${esc(p.placeholder)}"`}`;
    return `<div class="kit-input${p.wide === true ? " is-wide" : ""}"><label>${esc(p.label)}<input type="${p.type ?? "text"}" id="${p.id}" value="${esc(String(p.value))}"${attrs}>${p.hint === void 0 ? "" : `<small>${esc(p.hint)}</small>`}</label></div>`;
  }
  function textarea(p) {
    return `<div class="kit-textarea is-wide"><label class="kit-textarea__label" for="${p.id}">${esc(p.label)}</label><textarea class="kit-textarea__field" id="${p.id}" rows="${p.rows}"${p.max === void 0 ? "" : ` maxlength="${p.max}"`}${p.placeholder === void 0 ? "" : ` placeholder="${esc(p.placeholder)}"`}>${esc(p.value)}</textarea><div class="kit-textarea__footer"><p class="kit-textarea__hint">${esc(p.hint ?? "")}</p>${p.max === void 0 ? "" : `<span class="kit-textarea__counter" aria-live="polite">${p.value.length} / ${p.max}</span>`}</div></div>`;
  }
  function select(p) {
    const options = p.options.map((o) => `<option value="${esc(o.value)}"${o.value === p.value ? " selected" : ""}>${esc(o.label)}</option>`).join("");
    return `<div class="kit-select is-wide"><span id="${p.id}-label">${esc(p.label)}</span><select id="${p.id}" aria-labelledby="${p.id}-label">${options}</select></div>`;
  }
  var badges = (items) => `<div class="kit-badge">${items.map((b) => `<span class="${b.tone}">${esc(b.label)}</span>`).join("")}</div>`;
  function skeleton() {
    return `<div class="mf-entity" id="entity-editor" data-reads="${GET_ENTITY}"><div class="mf-entity__unsaved" id="entity-unsaved" role="alert" hidden><p id="entity-unsaved-text"></p>` + buttons(
      true,
      button({ id: "entity-unsaved-save", label: "Enregistrer et continuer", variant: "primary", writes: SAVE_ENTITY }),
      button({ id: "entity-unsaved-discard", label: "Abandonner", variant: "secondary" }),
      button({ id: "entity-unsaved-stay", label: "Rester", variant: "ghost" })
    ) + `</div><div class="mf-entity__notice" id="entity-notice" role="status" data-reads="${GET_ENTITY}" hidden></div><header class="mf-entity__head" id="entity-head"></header><div class="layout-2"><div class="stack"><form class="panel" id="entity-identity" aria-labelledby="entity-identity-title" onsubmit="return false"></form><section class="panel" id="entity-prompt" aria-labelledby="entity-prompt-title" hidden></section><section class="panel" id="entity-sheet" aria-labelledby="entity-sheet-title" hidden></section></div><div class="stack"><section class="panel" id="entity-ddb" aria-labelledby="entity-ddb-title" hidden></section><section class="panel" id="entity-table" aria-labelledby="entity-table-title"></section></div></div><div class="mf-entity__status" id="entity-status" aria-live="polite"></div><div class="row" id="entity-footer">${buttons(false, button({ id: "entity-save", label: "Enregistrer la fiche", variant: "primary", writes: SAVE_ENTITY }))}</div></div>`;
  }
  function head(entity, player, isNew) {
    const tones = [{ label: KIND_LABELS[entity.kind], tone: "primary" }];
    if (player !== null) tones.push({ label: `Jou\xE9 par ${player}`, tone: "success" });
    if (isNew) tones.push({ label: "Nouvelle fiche", tone: "accent" });
    const initials = initialsOf(entity.name) || "?";
    return `<div class="mf-entity__portrait" data-reads="${LIST_MEDIA}"><span class="mf-entity__glyph">${esc(initials)}</span></div><div class="mf-entity__title">${badges(tones)}<h1 id="entity-title">${esc(entity.name || "Sans nom")}</h1><p class="muted" id="entity-subtitle">${esc(entity.aliases.join(", ") || "Renseignez le nom, les alias et la description.")}</p></div>`;
  }
  function identity(entity, players) {
    const character = isCharacter(entity.kind);
    let html = '<h2 id="entity-identity-title">Identit\xE9</h2><div class="mf-entity__grid">' + input({ id: "entity-name", label: "Nom", value: entity.name, wide: true }) + input({ id: "entity-aliases", label: "Alias", value: entity.aliases.join(", "), wide: true, hint: "S\xE9par\xE9s par des virgules \u2014 servent \xE0 la d\xE9tection au micro" }) + "</div>";
    if (character) {
      const kind = (value, label, hint) => `<label><input type="radio" name="entity-kind" value="${value}"${entity.kind === value ? " checked" : ""}><span>${label}</span><small>${hint}</small></label>`;
      html += '<div class="mf-entity__grid"><div class="kit-radio"><fieldset id="entity-kind"><legend>Personnage</legend>' + kind("pnj", "PNJ", "jou\xE9 par le MJ") + kind("pj", "PJ", "li\xE9 \xE0 un joueur") + `</fieldset></div><div id="entity-player-field" data-reads="${LIST_PLAYERS}"${entity.kind === "pj" ? "" : " hidden"}>` + select({ id: "entity-player", label: "Joueur", value: entity.player, options: [{ value: "", label: "Choisir un joueur" }, ...players.map((p) => ({ value: p.id, label: p.name }))] }) + "</div></div>";
    }
    return html + textarea({ id: "entity-description", label: "Description", value: entity.description, rows: 3, max: 600 });
  }
  function prompt(entity, canPropose) {
    const pj = entity.kind === "pj";
    return `<div class="panel__head"><h2 id="entity-prompt-title">Aide \xE0 la cr\xE9ation</h2><span class="small muted">Seul le texte transcrit part vers le fournisseur IA</span></div><p class="small muted">Racontez le personnage \xE0 voix haute : l\u2019IA propose un background, un comportement pour l\u2019interpr\xE9ter${pj ? "" : " et un statblock cr\xE9dible"}. La fiche est modifi\xE9e et la version pr\xE9c\xE9dente reste dans l\u2019historique.</p><div class="row"><div class="kit-micbutton kit-micbutton--compact" id="entity-prompt-mic-box" data-state="idle"><button type="button" class="kit-micbutton__btn" id="entity-prompt-mic" aria-pressed="false" aria-label="Raconter au micro"><span class="kit-micbutton__ring"></span>${MIC}${PAUSE}</button><div class="kit-micbutton__text"><span class="kit-micbutton__label">Raconter au micro</span><span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span> <span class="kit-micbutton__timer">00:00</span></span></div></div></div>` + textarea({ id: "entity-prompt-text", label: "Prompt initial", value: entity.prompt, rows: 3, placeholder: "Ex. : un gobelin marchand, nerveux, qui doit de l\u2019argent \xE0 des gens dangereux\u2026", hint: canPropose ? "" : "Enregistrez la fiche pour demander une proposition \xE0 l\u2019IA." }) + buttons(false, button({ id: "entity-propose", label: pj ? "Proposer background et comportement" : "Proposer background, comportement et statblock", variant: "primary", writes: PROPOSE_CHARACTER, disabled: !canPropose })) + textarea({ id: "entity-background", label: "Background", value: entity.background, rows: 4 }) + textarea({ id: "entity-behavior", label: "Comportement, pour l\u2019interpr\xE9ter", value: entity.behavior, rows: 3 });
  }
  var features = (entity) => entity.features.map((f, i) => `<li data-feature="${i}"><span><b>${esc(f.name)}.</b> ${esc(f.text)}</span><button type="button" class="icon-btn" data-remove-feature="${i}" aria-label="Retirer ${esc(f.name)}">\xD7</button></li>`).join("");
  function statblock(entity) {
    const abilities = ABILITIES.map(
      (k) => `<li class="kit-statblock__ability"><span class="kit-statblock__abbr">${k}</span><span class="kit-statblock__score">${entity.stats[k]}</span><span class="kit-statblock__mod">${signed(entity.stats[k])}</span></li>`
    ).join("");
    const vitals = [
      { label: "CA", value: entity.ca },
      { label: "PV", value: entity.pvMax }
    ].map((v) => `<div class="kit-statblock__vital"><dt>${v.label}</dt><dd>${v.value}</dd></div>`).join("");
    const list = entity.features.map((f) => `<li class="kit-statblock__feature"><strong>${esc(f.name)}.</strong> ${esc(f.text)}</li>`).join("");
    return `<div class="kit-statblock"><div class="kit-statblock__rule"></div><div class="kit-statblock__body"><h3 class="kit-statblock__name">${esc(entity.name || "Sans nom")}</h3><p class="kit-statblock__type">${KIND_LABELS[entity.kind]}</p><dl class="kit-statblock__vitals">${vitals}</dl><ul class="kit-statblock__abilities">${abilities}</ul><ul class="kit-statblock__features">${list}</ul></div></div>`;
  }
  function sheet(entity) {
    const pj = entity.kind === "pj";
    return `<div class="panel__head"><h2 id="entity-sheet-title">Fiche</h2><span class="small muted">${pj ? "Mise \xE0 jour par D&amp;D Beyond" : "Sert \xE0 l\u2019initiative et aux PV en combat"}</span></div><div class="mf-entity__abilities" id="entity-abilities">${ABILITIES.map((k) => input({ id: `entity-stat-${k}`, label: `${k} ${signed(entity.stats[k])}`, type: "number", value: entity.stats[k], min: 1, max: 30 })).join("")}</div><div class="mf-entity__grid">${input({ id: "entity-ca", label: "CA", type: "number", value: entity.ca })}${input({ id: "entity-pv", label: "PV max", type: "number", value: entity.pvMax })}</div><h3 id="entity-features-title">Sorts et capacit\xE9s</h3><ul class="mf-entity__features" id="entity-features">${features(entity)}</ul><div class="mf-entity__grid" id="entity-feature-add">${input({ id: "entity-feature-name", label: "Intitul\xE9", value: "", placeholder: "Boule de feu" })}${input({ id: "entity-feature-text", label: "Effet", value: "", placeholder: "Sph\xE8re de 6 m, 8d6 d\xE9g\xE2ts de feu\u2026", wide: true })}</div>` + buttons(true, button({ id: "entity-feature-submit", label: "Ajouter le sort ou la capacit\xE9", variant: "secondary" })) + (pj ? "" : `<div id="entity-statblock">${statblock(entity)}</div>`);
  }
  function diffView(title, changes) {
    const rows = changes.map(
      (c, i) => `<li class="kit-diffview__row" data-index="${i}"><div class="kit-diffview__field">${esc(c.field)}</div><div class="kit-diffview__values"><del class="kit-diffview__old">${esc(c.old || "\u2014")}</del><ins class="kit-diffview__new">${esc(c.new || "\u2014")}</ins></div></li>`
    ).join("");
    return `<div class="kit-diffview"><header class="kit-diffview__head"><h2 class="kit-diffview__title">${esc(title)}</h2><span class="kit-diffview__meta">${changes.length} champ(s) mis \xE0 jour</span></header><ul class="kit-diffview__list">${rows}</ul></div>`;
  }
  var kept = (entity) => `<h3>Conserv\xE9s</h3><ul class="mf-entity__kept" id="entity-ddb-kept"><li>Alias : ${esc(entity.aliases.join(", "))}</li><li>Notes : ${esc(entity.notes.slice(0, 60))}${entity.notes.length > 60 ? "\u2026" : ""}</li><li>Token : recadrage ${entity.crop.zoom} %</li></ul>`;
  var when = (at) => {
    const date = new Date(at);
    return Number.isNaN(date.getTime()) ? at : date.toLocaleString("fr-FR", { dateStyle: "long", timeStyle: "short" });
  };
  function ddb(entity, url) {
    const linked = entity.ddb !== null;
    const syncedAt = entity.ddb?.syncedAt ?? null;
    return '<h2 id="entity-ddb-title">D&amp;D Beyond</h2>' + input({ id: "entity-ddb-url", label: "URL du personnage", value: url, placeholder: "https://www.dndbeyond.com/characters/\u2026", wide: true }) + buttons(
      true,
      button({ id: "entity-ddb-import", label: "Importer", variant: linked ? "ghost" : "primary", writes: IMPORT_CHARACTER }),
      button({ id: "entity-ddb-resync", label: "Resynchroniser", variant: "primary", writes: RESYNC_CHARACTER, disabled: !linked })
    ) + `<p class="small muted" id="entity-ddb-status">${syncedAt === null ? "Jamais import\xE9" : `Derni\xE8re synchronisation : ${esc(when(syncedAt))}`}</p><p class="small muted">La resynchronisation met \xE0 jour les stats, les PV et les sorts. Les alias, les notes et le token d\xE9finis ici sont conserv\xE9s.</p><div id="entity-ddb-diff"></div>`;
  }
  function token(entity) {
    const initials = esc(initialsOf(entity.name));
    const clip = tokenHasImage(entity) ? `<span class="kit-token__portrait"><span class="mf-entity__glyph">${initials}</span></span>` : `<span class="kit-token__initials">${initials}</span>`;
    const first = entity.name.trim().split(/\s+/)[0] ?? "";
    return `<div class="kit-token"><div class="kit-token__item"><div class="kit-token__disc" style="--kit-token-bg: var(--color-${colorName(entity.color)})"><div class="kit-token__clip">${clip}</div>${entity.kind === "pnj" ? '<span class="kit-token__num">1</span>' : ""}</div><div class="kit-token__name">${esc(first)}</div></div></div>`;
  }
  function sliders(entity) {
    const field = (id, name, label, value, min, max) => `<div class="kit-slider__field" data-name="${name}"><div class="kit-slider__head"><label class="kit-slider__label" for="${id}">${label}</label><span class="kit-slider__value"><output class="kit-slider__out" for="${id}">${value}</output><span class="kit-slider__unit">%</span></span></div><input class="kit-slider__input" id="${id}" type="range" name="${name}" min="${min}" max="${max}" value="${value}"><div class="kit-slider__bounds"><span>${min} %</span><span>${max} %</span></div></div>`;
    return '<div class="kit-slider kit-slider--bare">' + field("entity-crop-zoom", "zoom", "Recadrage \u2014 zoom", entity.crop.zoom, 100, 220) + field("entity-crop-x", "x", "Centre horizontal", entity.crop.x, 0, 100) + field("entity-crop-y", "y", "Centre vertical", entity.crop.y, 0, 100) + "</div>";
  }
  function mediaSelect(id, label, has, chosen, medias) {
    const value = chosen === void 0 ? "" : chosen ?? "none";
    const options = [{ value: "", label: has ? "Image actuelle (conserv\xE9e)" : "Aucune image" }, ...has ? [{ value: "none", label: "Retirer l\u2019image" }] : [], ...medias.map((m) => ({ value: m.id, label: m.title }))];
    return select({ id, label, value, options });
  }
  function table(entity, illustrations) {
    if (!isCharacter(entity.kind)) {
      const heading = entity.kind === "lieu" ? "Illustration" : "Liens";
      const body = entity.kind === "lieu" ? `<p class="muted" data-reads="${LIST_MEDIA}" id="entity-illustrations">${illustrations.length} illustration${illustrations.length > 1 ? "s" : ""} dans la m\xE9diath\xE8que de la campagne.</p>` : '<p class="muted">Un \xE9v\xE9nement se lie aux sc\xE8nes depuis la pr\xE9paration de s\xE9ance.</p>';
      return `<h2 id="entity-table-title">${heading}</h2>${body}` + textarea({ id: "entity-notes", label: "Notes du MJ", value: entity.notes, rows: 4, hint: "Jamais visibles sur la player view" });
    }
    return `<h2 id="entity-table-title">\xC0 la table</h2><div data-reads="${LIST_MEDIA}" id="entity-media">${mediaSelect("entity-portrait", "Portrait", entity.portrait, entity.portraitMedia, illustrations)}</div><h3>Token</h3><div class="mf-entity__token" id="entity-token"><div id="entity-token-preview">${token(entity)}</div>${sliders(entity)}</div>` + mediaSelect("entity-token-image", "Image de token d\xE9di\xE9e", entity.tokenImage, entity.tokenMedia, illustrations) + textarea({ id: "entity-notes", label: "Notes du MJ", value: entity.notes, rows: 4, hint: "Jamais visibles sur la player view" });
  }
  function notice(source, changes) {
    const fields = changes.map((c) => c.field).join(", ");
    return `<span><b>Mis \xE0 jour</b> \u2014 ${esc(source)}${fields === "" ? "" : ` : ${esc(fields)}`}.</span>` + buttons(true, button({ id: "entity-notice-diff", label: "Voir le diff", variant: "ghost" }));
  }

  // apps/campaign/entity-editor/src/style.ts
  var STYLE = `
.mf-entity { box-sizing: border-box; display: grid; gap: var(--space-4); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-entity *, .mf-entity *::before, .mf-entity *::after { box-sizing: inherit; }
.mf-entity [hidden] { display: none !important; }
.mf-entity h1, .mf-entity h2, .mf-entity h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-entity h1 { font-size: var(--font-size-heading); color: var(--color-primary); }
.mf-entity h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-entity h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-entity p { margin: 0; }
.mf-entity .layout-2 { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
@media (max-width: 960px) { .mf-entity .layout-2 { grid-template-columns: minmax(0, 1fr); } }
.mf-entity .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-entity .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-entity .muted { color: var(--color-text-muted); }
.mf-entity .small { font-size: var(--font-size-small); }
.mf-entity .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-entity .stack { display: grid; gap: var(--space-3); align-content: start; min-width: 0; }
.mf-entity .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-entity .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-entity .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-entity .kit-input.is-wide, .mf-entity .kit-select.is-wide { max-width: none; }
.mf-entity .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-entity .kit-select select:hover { border-color: var(--color-primary); }
.mf-entity .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-entity__unsaved { position: sticky; top: var(--space-2); z-index: 20; display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2) var(--space-4); padding: var(--space-2) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-left: calc(var(--border-width) * 4) solid var(--color-accent); border-radius: var(--radius-medium); box-shadow: var(--shadow-high); }
.mf-entity__notice { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2) var(--space-4); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-accent); border-radius: var(--radius-medium); }
.mf-entity__status { min-height: var(--space-4); }
.mf-entity__status p { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-entity__status .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-entity__status .is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-entity__head { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-4); align-items: center; }
.mf-entity__portrait { display: grid; place-items: center; width: calc(var(--space-5) * 2.5); height: calc(var(--space-5) * 2.5); border-radius: var(--radius-pill); overflow: hidden; border: calc(var(--border-width) * 3) solid var(--color-secondary); box-shadow: var(--shadow-high); background: var(--color-surface-2); }
.mf-entity__glyph { font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: var(--font-size-large); text-transform: uppercase; color: var(--color-text); }
.mf-entity .kit-token__portrait { display: grid; place-items: center; width: 100%; height: 100%; background: var(--color-surface-2); }
.mf-entity__title { display: grid; gap: var(--space-1); }
.mf-entity__grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(calc(var(--space-5) * 5), 1fr)); gap: var(--space-3); }
.mf-entity__abilities { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: var(--space-2); }
.mf-entity__abilities .kit-input label { font-size: var(--font-size-small); }
.mf-entity__abilities .kit-input input { width: 100%; text-align: center; padding: var(--space-1); }
.mf-entity__features { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-entity__features li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--space-2); align-items: start; padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-entity__features b { color: var(--color-accent); }
.mf-entity__token { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); align-items: center; }
.mf-entity__token .kit-token { padding: 0; background: transparent; }
.mf-entity__token .kit-token__portrait { transform-origin: center; }
.mf-entity__kept { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); font-size: var(--font-size-small); }
.mf-entity__kept li::before { content: "\u2713 "; color: var(--color-success); }
.mf-entity .kit-statblock { max-width: none; }
@media (max-width: 760px) { .mf-entity__abilities { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
`;

  // apps/campaign/entity-editor/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  var clicks = 0;
  var keyOf = (dataCapability, entity) => `EntityEditor:${dataCapability}:${entity}:${Date.now().toString(36)}-${clicks += 1}`;
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="EntityEditor"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "EntityEditor";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, props2, shell2, timing = WAITING, dictation) {
    const doc = host2.ownerDocument;
    ensureStyle(doc);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`EntityEditor: #${id} is not in the sheet`);
      return found;
    };
    const field = (id) => host2.querySelector(`#${id}`);
    let target = null;
    let stored = null;
    let draft = null;
    let players = [];
    let illustrations = [];
    let freshId = "";
    let pending = null;
    let deferred = null;
    let busy = false;
    let opening = 0;
    let ddbDiff = null;
    const dm = { url: "" };
    const campaign = () => {
      const known = shell2.context?.get(CAMPAIGN_CONTEXT);
      return typeof known === "string" ? known : "";
    };
    const isNew = () => stored !== null && stored.id === "";
    const dirty = () => draft !== null && stored !== null && isDirty(draft, stored);
    const tell = (name, detail) => {
      const view2 = doc.defaultView;
      if (view2) host2.dispatchEvent(new view2.CustomEvent(name, { bubbles: true, detail }));
    };
    let opened2 = null;
    const announce = (detail) => {
      opened2 = detail;
      tell(EVENT_OPENED, detail);
      const view2 = doc.defaultView;
      if (!view2) return;
      doc.dispatchEvent(new view2.CustomEvent(BUS_OPENED, { detail }));
      try {
        const url = new URL(view2.location.href);
        url.searchParams.delete("id");
        url.searchParams.delete("new");
        url.searchParams.set(detail.id === null ? "new" : "id", detail.id ?? detail.kind);
        view2.history.replaceState(null, "", url);
      } catch {
      }
    };
    const say = (outcome) => {
      part("entity-status").innerHTML = outcome.kind === "none" ? "" : `<p class="${outcome.kind === "busy" ? "" : `is-${outcome.kind}`}">${outcome.text.replace(/[&<>]/g, (c) => `&#${c.charCodeAt(0)};`)}</p>`;
    };
    const playerName = () => {
      if (draft === null || draft.kind !== "pj") return null;
      return players.find((p) => p.id === draft?.player)?.name ?? null;
    };
    const paintHead = () => {
      if (draft === null) return;
      part("entity-head").innerHTML = head(draft, playerName(), isNew());
      doc.title = `${draft.name || "Nouvelle fiche"} \u2014 Fiche d\u2019entit\xE9 \u2014 ForgeDesMondes`;
    };
    const stopMic = { stop: () => void 0 };
    const paintPrompt = () => {
      const box = part("entity-prompt");
      box.hidden = draft === null || !isCharacter(draft.kind);
      stopMic.stop();
      if (draft === null || box.hidden) return;
      box.innerHTML = prompt(draft, !isNew());
      wireMic();
    };
    const paintDdb = () => {
      const box = part("entity-ddb");
      box.hidden = draft === null || draft.kind !== "pj";
      if (draft === null || box.hidden) return;
      box.innerHTML = ddb(draft, dm.url);
      if (ddbDiff !== null) part("entity-ddb-diff").innerHTML = (ddbDiff.length > 0 ? diffView("Resynchronisation", ddbDiff) : "") + kept(draft);
    };
    const paintSliders = () => {
      for (const slider of host2.querySelectorAll(".kit-slider__input")) {
        const min = Number(slider.min);
        const max = Number(slider.max);
        slider.style.setProperty("--fill", `${(Number(slider.value) - min) / (max - min) * 100}%`);
        const out = slider.closest(".kit-slider__field")?.querySelector(".kit-slider__out");
        if (out) out.textContent = slider.value;
      }
    };
    const paintToken = () => {
      const box = host2.querySelector("#entity-token-preview");
      if (box === null || draft === null) return;
      box.innerHTML = token(draft);
      const portrait = box.querySelector(".kit-token__portrait");
      if (portrait) portrait.style.transform = `scale(${draft.crop.zoom / 100}) translate(${(50 - draft.crop.x) / 2}%, ${(50 - draft.crop.y) / 2}%)`;
    };
    const paintTable = () => {
      if (draft === null) return;
      part("entity-table").innerHTML = table(draft, illustrations);
      paintToken();
      paintSliders();
    };
    const paintSheet = () => {
      const box = part("entity-sheet");
      box.hidden = draft === null || !isCharacter(draft.kind);
      if (draft === null || box.hidden) return;
      box.innerHTML = sheet(draft);
    };
    const paintIdentity = () => {
      if (draft !== null) part("entity-identity").innerHTML = identity(draft, players);
    };
    const paintAll = () => {
      paintHead();
      paintIdentity();
      paintPrompt();
      paintSheet();
      paintDdb();
      paintTable();
      paintControls();
    };
    const paintLive = () => {
      if (draft === null) return;
      const title = host2.querySelector("#entity-title");
      if (title) title.textContent = draft.name || "Sans nom";
      const subtitle = host2.querySelector("#entity-subtitle");
      if (subtitle) subtitle.textContent = draft.aliases.join(", ") || "Renseignez le nom, les alias et la description.";
      const glyph = host2.querySelector(".mf-entity__portrait .mf-entity__glyph");
      if (glyph) glyph.textContent = initialsOf(draft.name) || "?";
      for (const ability of ABILITIES) {
        const label = field(`entity-stat-${ability}`)?.closest("label");
        if (label?.firstChild) label.firstChild.textContent = `${ability} ${signed(draft.stats[ability])}`;
      }
      const block = host2.querySelector("#entity-statblock");
      if (block) block.innerHTML = statblock(draft);
      const counter = host2.querySelector("#entity-description ~ .kit-textarea__footer .kit-textarea__counter");
      if (counter) counter.textContent = `${draft.description.length} / 600`;
      paintToken();
      paintSliders();
    };
    const paintControls = () => {
      const enable = (id, on) => {
        const control = host2.querySelector(`#${id}`);
        if (control) control.disabled = busy || !on;
      };
      enable("entity-save", true);
      enable("entity-propose", !isNew());
      enable("entity-ddb-import", true);
      enable("entity-ddb-resync", draft?.ddb != null && !isNew());
    };
    const lock = (locked) => {
      busy = locked;
      for (const control of host2.querySelectorAll("button, input, textarea, select")) control.disabled = locked;
      if (locked) return;
      paintControls();
      const next = deferred;
      deferred = null;
      if (next !== null) request(next);
    };
    const text2 = (id) => field(id)?.value ?? null;
    const num = (id, otherwise) => {
      const value = text2(id);
      const parsed = value === null || value.trim() === "" ? NaN : Number(value);
      return Number.isFinite(parsed) ? parsed : otherwise;
    };
    const collect = () => {
      if (draft === null) return;
      const d = draft;
      d.name = text2("entity-name") ?? d.name;
      const aliases = text2("entity-aliases");
      if (aliases !== null) d.aliases = parseAliases(aliases);
      d.description = text2("entity-description") ?? d.description;
      d.notes = text2("entity-notes") ?? d.notes;
      d.prompt = text2("entity-prompt-text") ?? d.prompt;
      d.background = text2("entity-background") ?? d.background;
      d.behavior = text2("entity-behavior") ?? d.behavior;
      d.player = text2("entity-player") ?? d.player;
      for (const ability of ABILITIES) d.stats[ability] = num(`entity-stat-${ability}`, d.stats[ability]);
      d.ca = num("entity-ca", d.ca);
      d.pvMax = num("entity-pv", d.pvMax);
      d.crop = { zoom: num("entity-crop-zoom", d.crop.zoom), x: num("entity-crop-x", d.crop.x), y: num("entity-crop-y", d.crop.y) };
      dm.url = text2("entity-ddb-url") ?? dm.url;
      const media = (id, current) => {
        const chosen = text2(id);
        return chosen === null ? current : chosen === "" ? void 0 : chosen === "none" ? null : chosen;
      };
      d.portraitMedia = media("entity-portrait", d.portraitMedia);
      d.tokenMedia = media("entity-token-image", d.tokenMedia);
    };
    const read = async (id) => {
      const answer = await shell2.capability(GET_ENTITY, void 0, { id });
      return parseEntity(answer.data);
    };
    let listsFor = null;
    async function readLists() {
      const known = campaign();
      const variables = campaignVariables(known);
      [players, illustrations] = await Promise.all([
        shell2.capability(LIST_PLAYERS, void 0, variables).then((answer) => parsePlayers(answer.data), () => []),
        shell2.capability(LIST_MEDIA, void 0, variables).then((answer) => parseIllustrations(answer.data), () => [])
      ]);
      listsFor = known;
    }
    async function open(wanted) {
      const mine = opening += 1;
      let entity;
      try {
        if ("fresh" in wanted) entity = freshEntity(wanted.fresh);
        else entity = await read(wanted.id);
        if (mine !== opening) return false;
        await readLists();
      } catch {
        if (mine === opening) say({ kind: "error", text: "Impossible de lire la fiche." });
        return false;
      }
      if (mine !== opening) return false;
      if (entity === null) {
        say({ kind: "error", text: "Cette fiche est introuvable." });
        return false;
      }
      target = wanted;
      stored = entity;
      draft = structuredClone(entity);
      freshId = newId();
      pending = null;
      ddbDiff = null;
      dm.url = entity.ddb?.url ?? "";
      part("entity-unsaved").hidden = true;
      part("entity-notice").hidden = true;
      say({ kind: "none" });
      paintAll();
      shell2.context?.set(ENTITY_CONTEXT, targetProp(wanted));
      announce({ id: entity.id === "" ? null : entity.id, name: entity.name, kind: entity.kind });
      return true;
    }
    const showBar = () => {
      if (draft === null || pending === null) return;
      part("entity-unsaved-text").textContent = `\xAB ${draft.name || "Nouvelle fiche"} \xBB a des modifications non enregistr\xE9es. Ouvrir ${"fresh" in pending ? "une nouvelle fiche" : "l\u2019\xE9l\xE9ment demand\xE9"} ?`;
      part("entity-unsaved").hidden = false;
    };
    function request(wanted) {
      if (target !== null && sameTarget(wanted, target)) {
        pending = null;
        part("entity-unsaved").hidden = true;
        return;
      }
      if (busy) {
        deferred = wanted;
        return;
      }
      collect();
      if (draft !== null && dirty()) {
        pending = wanted;
        showBar();
        return;
      }
      void open(wanted);
    }
    async function send(dataCapability, payload, to, key) {
      const answer = await shell2.command(commandBody(dataCapability, payload, to, key));
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    function applied(state, what) {
      if (state.status === "rejected") say({ kind: "error", text: `${what} refus\xE9${reasons(state.violations) === "" ? "" : ` : ${reasons(state.violations)}`}.` });
      else if (state.status !== "applied") say({ kind: "error", text: `${what} n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).` });
      return state.status === "applied";
    }
    async function save() {
      if (busy || draft === null || stored === null) return false;
      collect();
      if (draft.name.trim() === "") {
        say({ kind: "error", text: "Donnez un nom \xE0 la fiche." });
        field("entity-name")?.focus();
        return false;
      }
      const creating = isNew();
      if (creating && campaign() === "") {
        say({ kind: "error", text: "Aucune campagne choisie." });
        return false;
      }
      const id = creating ? freshId : stored.id;
      const sending = { ...structuredClone(draft), id };
      lock(true);
      try {
        say({ kind: "busy", text: "Enregistrement en cours\u2026" });
        const state = await send(SAVE_ENTITY, savePayload(sending, campaign(), creating), entityTarget(id), keyOf(SAVE_ENTITY, id));
        if (!applied(state, "L\u2019enregistrement")) return false;
        let fresh = null;
        try {
          fresh = await read(id);
        } catch {
        }
        stored = fresh ?? structuredClone({ ...sending, portraitMedia: void 0, tokenMedia: void 0 });
        draft = structuredClone(stored);
        target = { id };
        busy = false;
        paintAll();
        shell2.context?.set(ENTITY_CONTEXT, id);
        if (creating) announce({ id, name: stored.name, kind: stored.kind });
        else if (opened2 !== null) opened2 = { ...opened2, name: stored.name };
        say({ kind: "ok", text: "Fiche enregistr\xE9e." });
        return true;
      } catch {
        say({ kind: "error", text: "L\u2019enregistrement a \xE9chou\xE9." });
        return false;
      } finally {
        lock(false);
      }
    }
    async function change(what, then) {
      if (busy || draft === null || stored === null || isNew()) return;
      collect();
      const before = stored;
      const id = before.id;
      lock(true);
      try {
        say({ kind: "busy", text: what.busy });
        const state = await send(what.dataCapability, what.payload, entityTarget(id), keyOf(what.dataCapability, id));
        if (!applied(state, what.failed)) return;
        const after = await read(id).catch(() => null);
        if (after === null) {
          say({ kind: "error", text: `${what.failed} a abouti, mais la fiche n\u2019a pas pu \xEAtre relue.` });
          return;
        }
        const changes = changesBetween(before, after);
        draft = overlay(draft, before, after);
        stored = after;
        then?.(changes);
        busy = false;
        paintAll();
        if (changes.length === 0) say({ kind: "ok", text: "Aucun champ de la fiche n\u2019a chang\xE9." });
        else {
          say({ kind: "none" });
          const box = part("entity-notice");
          box.innerHTML = notice(what.source, changes);
          box.hidden = false;
          tell(EVENT_UPDATED, { id, origin: what.origin, source: what.source, changes });
        }
      } catch {
        say({ kind: "error", text: `${what.failed} a \xE9chou\xE9.` });
      } finally {
        lock(false);
      }
    }
    const propose = () => {
      collect();
      if (draft === null || draft.prompt.trim() === "") {
        field("entity-prompt-text")?.focus();
        return Promise.resolve();
      }
      return change({ dataCapability: PROPOSE_CHARACTER, payload: { prompt: draft.prompt }, busy: "Proposition en cours\u2026", failed: "La proposition", origin: "ia", source: "Proposition de l\u2019IA" });
    };
    const resync = () => change({ dataCapability: RESYNC_CHARACTER, payload: {}, busy: "Resynchronisation en cours\u2026", failed: "La resynchronisation", origin: "dndbeyond", source: "Resynchronisation D&D Beyond" }, (changes) => {
      ddbDiff = changes;
    });
    async function importCharacter() {
      collect();
      const url = dm.url.trim();
      if (draft === null || stored === null || busy) return;
      if (url === "") {
        field("entity-ddb-url")?.focus();
        return;
      }
      const known = campaign();
      const payload = { url, ...known === "" ? {} : { campaign: known }, ...draft.player === "" ? {} : { player: draft.player } };
      if (!isNew()) return change({ dataCapability: IMPORT_CHARACTER, payload, busy: "Import en cours\u2026", failed: "L\u2019import", origin: "dndbeyond", source: "Import D&D Beyond" });
      lock(true);
      try {
        say({ kind: "busy", text: "Import en cours\u2026" });
        const state = await send(IMPORT_CHARACTER, payload, entityTarget(""), keyOf(IMPORT_CHARACTER, freshId));
        if (applied(state, "L\u2019import")) say({ kind: "ok", text: "Personnage import\xE9 : retrouvez-le dans la liste des \xE9l\xE9ments." });
      } catch {
        say({ kind: "error", text: "L\u2019import a \xE9chou\xE9." });
      } finally {
        lock(false);
      }
    }
    async function onBar(choice) {
      const wanted = pending;
      if (wanted === null) return;
      if (choice === "stay") {
        pending = null;
        part("entity-unsaved").hidden = true;
        if (target !== null) shell2.context?.set(ENTITY_CONTEXT, targetProp(target));
        return;
      }
      if (choice === "save") {
        part("entity-unsaved").hidden = true;
        if (!await save()) {
          pending = wanted;
          showBar();
          return;
        }
      }
      await open(wanted);
    }
    function wireMic() {
      const mic = host2.querySelector("#entity-prompt-mic-box");
      const button2 = host2.querySelector("#entity-prompt-mic");
      if (mic === null || button2 === null) return;
      const label = mic.querySelector(".kit-micbutton__state");
      const timer = mic.querySelector(".kit-micbutton__timer");
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
      const set = (next) => {
        state = next;
        stopTimers();
        if (next === "recording") {
          tick = setInterval(() => {
            if (!mic.isConnected) return stopTimers();
            seconds += 1;
            if (timer) timer.textContent = clock();
          }, 1e3);
          pulse = setInterval(() => mic.style.setProperty("--kit-level", (Math.random() * 0.9 + 0.1).toFixed(2)), 140);
        }
        mic.setAttribute("data-state", next);
        if (label) label.textContent = MIC_TEXTS[next];
        if (timer) timer.textContent = clock();
        button2.setAttribute("aria-pressed", String(next === "recording"));
        button2.setAttribute("aria-label", `Raconter au micro \u2014 ${MIC_TEXTS[next]}`);
      };
      stopMic.stop = stopTimers;
      button2.addEventListener("click", () => {
        if (state !== "recording") return set("recording");
        set("paused");
        const heard = dictation?.transcript() ?? "";
        const prompt2 = field("entity-prompt-text");
        if (heard === "" || prompt2 === null) return;
        prompt2.value = prompt2.value === "" ? heard : `${prompt2.value}
${heard}`;
        collect();
      });
      set("idle");
    }
    host2.addEventListener("input", (event) => {
      const el = event.target;
      if (!el?.id.startsWith("entity-") || draft === null) return;
      collect();
      if (el.id !== "entity-ddb-url") paintLive();
    });
    host2.addEventListener("change", (event) => {
      const el = event.target;
      if (el === null || draft === null) return;
      if (el.name === "entity-kind" && isKind(el.value) && el.value !== draft.kind) {
        collect();
        draft.kind = el.value;
        if (draft.kind !== "pj") draft.player = "";
        paintAll();
        return;
      }
      if (el.id.startsWith("entity-")) {
        collect();
        if (el.id === "entity-player") paintHead();
        paintLive();
      }
    });
    host2.addEventListener("click", (event) => {
      const clicked = event.target?.closest("button");
      if (!clicked || draft === null) return;
      const removed = clicked.dataset.removeFeature;
      if (removed !== void 0) {
        collect();
        draft.features.splice(Number(removed), 1);
        part("entity-features").innerHTML = features(draft);
        paintLive();
        return;
      }
      const writes = clicked.dataset.writes;
      if (clicked.id === "entity-unsaved-save") void onBar("save");
      else if (writes === SAVE_ENTITY) void save();
      else if (writes === PROPOSE_CHARACTER) void propose();
      else if (writes === IMPORT_CHARACTER) void importCharacter();
      else if (writes === RESYNC_CHARACTER) void resync();
      else if (clicked.id === "entity-unsaved-discard") void onBar("discard");
      else if (clicked.id === "entity-unsaved-stay") void onBar("stay");
      else if (clicked.id === "entity-feature-submit") {
        collect();
        const name = text2("entity-feature-name")?.trim() ?? "";
        if (name === "") return;
        draft.features.push({ name, text: text2("entity-feature-text")?.trim() ?? "" });
        for (const id of ["entity-feature-name", "entity-feature-text"]) {
          const input2 = field(id);
          if (input2) input2.value = "";
        }
        part("entity-features").innerHTML = features(draft);
        paintLive();
      } else if (clicked.id === "entity-notice-diff" && stored !== null) tell(EVENT_HISTORY, { id: stored.id });
    });
    const view = doc.defaultView;
    if (view) {
      const listen = (name, handle) => {
        const listener = (event) => {
          if (!host2.isConnected) return doc.removeEventListener(name, listener);
          handle(event.detail ?? {});
        };
        doc.addEventListener(name, listener);
      };
      listen(BUS_OPEN, (asked) => {
        const wanted = parseTarget(asked.new ? `${NEW_PREFIX}${asked.new}` : asked.id);
        if (wanted !== null) request(wanted);
      });
      listen(BUS_QUERY, () => {
        if (opened2 !== null) doc.dispatchEvent(new view.CustomEvent(BUS_OPENED, { detail: opened2 }));
      });
    }
    const stopFollowing = shell2.context?.on?.(() => {
      if (!host2.isConnected) return stopFollowing?.();
      if (draft === null || campaign() === listsFor) return;
      void readLists().then(() => {
        if (busy) return;
        collect();
        paintAll();
      });
    });
    const query = new URLSearchParams(view?.location.search ?? "");
    const first = parseTarget(props2.entity) ?? parseTarget(query.get("id") || (query.get("new") ? `${NEW_PREFIX}${query.get("new")}` : null));
    if (first === null) say({ kind: "error", text: "Aucune fiche choisie." });
    else void open(first);
    return {
      update(next) {
        const wanted = parseTarget(next.entity);
        if (wanted !== null) request(wanted);
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
  var json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  var notFound = () => json({ error: "not-found" }, 404);
  var refused = (why) => json({ error: why }, 409);
  var same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  var FINAL2 = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  var LEGACY = { reads: ["../src/reads.js", "READS"], commands: ["../src/commands.js", "COMMANDS"] };
  var HandlerError = class extends Error {
  };
  var shown2 = (path) => `mock/${path.replace(/^\.\.\//, "")}`;
  async function moduleAt(path) {
    const url = new URL(path, location.href).href;
    const served = await fetch(url).then((r) => r.ok, () => false);
    if (!served) return null;
    try {
      return await import(url);
    } catch (e) {
      throw new HandlerError(`${shown2(path)} does not load: ${e && e.message ? e.message : e}`);
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
      const kept2 = Object.hasOwn(legacy[kind], id) ? legacy[kind][id] : void 0;
      if (own && kept2) throw new HandlerError(`${id} is defined twice: mock/handlers/${id}.js and ${shown2(LEGACY[kind][0])} \u2014 keep one`);
      return own || kept2;
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
    const answer = (entry, replayed = false) => json({ commandId: entry.commandId, partition: entry.partition, replayed, warnings: replayed ? [] : entry.warnings || [], result: resultOf(entry) }, 202);
    const drive = async () => {
      for (const partition of new Set([...s.queue.values()].filter((e) => !FINAL2.has(e.state)).map((e) => e.partition))) {
        for (; ; ) {
          const head2 = pendingIn(partition).find((e) => e.state !== "parked");
          if (!head2) break;
          if (head2.state === "awaiting_review") break;
          if (head2.state === "awaiting_confirmation") {
            if (head2.atHeadSince === void 0) head2.atHeadSince = Date.now();
            if (Date.now() - head2.atHeadSince < s.parkAfterMs) break;
            head2.state = "parked";
            s.tell("Parked", head2.by, head2.commandId, { reason: "reached the head unconfirmed", actions: ["requeue", "cancel"] });
            continue;
          }
          const verdict = await invariants(head2.aggregate, head2.payload);
          if (verdict.reject.length) {
            head2.state = "rejected";
            head2.violations = verdict.reject;
            s.tell("CommandRejected", head2.by, head2.commandId, { violations: verdict.reject });
            continue;
          }
          if (head2.handler) {
            const said = await head2.handler(s, { payload: head2.payload, targetId: head2.targetId, version: head2.version, basedOn: head2.basedOn });
            const violations = Array.isArray(said) ? said : said && said.violations || [];
            if (violations.length) {
              head2.state = "rejected";
              head2.violations = violations;
              s.tell("CommandRejected", head2.by, head2.commandId, { violations });
              continue;
            }
          }
          head2.state = "applied";
          s.bump();
          head2.dataVersion = s.dataVersion;
          for (const [f, v] of Object.entries(head2.payload)) s.confirmed.set(`${partition}.${f}`, v);
          s.tell("CommandApplied", [head2.by, "infrastructure"], head2.commandId, { dataVersion: s.dataVersion });
          for (const behind of pendingIn(partition)) {
            if (behind.aheadOf === head2.commandId) s.tell("AheadResolved", behind.by, behind.commandId, { reason: `${head2.commandId} was resolved` });
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
      const aggregate = target.aggregate || null;
      const declared = (aggregate && await s.load(`aggregates/${aggregate}`) || { fields: {} }).fields || {};
      const derived = Object.keys(payload).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      const known = (await s.manifests())[dataCapability];
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
    s.fetch = async (input2, init) => {
      try {
        await s.ready;
        if (s.problems.length) return json({ error: `the mock server did not start: ${s.problems.join("; ")}` }, 500);
        return await respond(input2, init);
      } catch (e) {
        if (!(e instanceof HandlerError)) throw e;
        console.error(e.message);
        return json({ error: e.message }, 500);
      }
    };
    const respond = async (input2, init) => {
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

  // mock/src/EntityEditor.js
  var param = (name) => new URLSearchParams(location.search).get(name);
  var { host, props } = mountPoint();
  var server = mockServer();
  var shell = shellOnce((s) => createShell((input2, init) => s.fetch(new URL(input2, location.href), init)));
  var opened = null;
  host.addEventListener("entity-editor:opened", (e) => {
    opened = e.detail;
  });
  host.addEventListener("entity-editor:updated", (e) => Mock.emit("entity-updated", { id: e.detail.id, versioned: true }));
  host.addEventListener("entity-editor:history", (e) => Mock.emit("open-versions", { id: e.detail.id }));
  Promise.all([server.load("EntityEditor"), server.load("campaign.getCampaign")]).then(([editor, campaign]) => {
    shell.context.set("campaign", campaign.id);
    const dictation = { transcript: () => opened && editor.dictations[opened.id] || editor.dictations.default };
    const first = param("id") || (param("new") ? `new:${param("new")}` : null) || props.entity || editor.defaultEntity;
    compose({ ...props, entity: first }, (resolved) => mount(host, resolved, shell, void 0, dictation));
  });
})();
