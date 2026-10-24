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
    const first = parseTarget(props2.entity) ?? parseTarget(query.get("id") ?? (query.get("new") ? `${NEW_PREFIX}${query.get("new")}` : null));
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

  // mock/src/reads.js
  var bridged = /* @__PURE__ */ new WeakSet();
  async function instanceOf(server2, id) {
    const data = await server2.load("table.getBattlemapInstance");
    if (!data) return void 0;
    if (id && data.id !== id) return server2.instances ? server2.instances.get(id) : void 0;
    const store = window.Mock && window.Mock.store;
    if (store && !bridged.has(data)) {
      bridged.add(data);
      data.state = store.get(`map:${data.id}`, data.state);
      store.on(`map:${data.id}`, (state) => {
        if (state && state !== data.state) {
          data.state = state;
          server2.bump();
        }
      });
    }
    return data;
  }
  function shareInstance(data) {
    if (window.Mock) window.Mock.store.set(`map:${data.id}`, data.state);
  }
  function healthBand(hp, hpMax) {
    if (typeof hp !== "number" || typeof hpMax !== "number") return null;
    const current = Math.max(0, Math.min(hpMax, hp));
    return current === 0 ? "mort" : current === hpMax ? "indemne" : 2 * current > hpMax ? "bless\xE9" : "en sang";
  }
  async function dressed(server2, tokens) {
    const characters = await server2.load("campaign.listCharacters") || [];
    return tokens.map((t) => {
      const c = characters.find((x) => x.id === t.char);
      return c ? { ...t, initials: c.initials, color: c.color, portrait: c.portrait } : t;
    });
  }
  var diffusionBridged = false;
  async function diffusionRow(server2) {
    const fixture = await server2.load("live.getDiffusion");
    const store = window.Mock && window.Mock.store;
    if (store && !diffusionBridged) {
      diffusionBridged = true;
      store.on("diffusion", () => server2.bump());
    }
    return store ? store.get("diffusion", fixture) : structuredClone(fixture);
  }
  async function ratioOfIllustration(server2, id) {
    const media = await server2.load("media.listMedia");
    const found = media && media.illustrations.find((i) => i.id === id);
    return found && found.ratio > 0 ? found.ratio : 1;
  }
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e" };
  var READS = {
    // live.getDiffusion (crates/live/capabilities/get-diffusion): the one row both
    // player views read, so view 'dm' and view 'player' answer the same values —
    // position, size, zoom and pan. The window's x and w are fractions of the stage's
    // width, y of its height; h, derived from the image's ratio, is in the width's unit
    // and never stored. Nothing else leaves the row: no token, no HP figure, no note.
    "live.getDiffusion": async (server2) => {
      const row = await diffusionRow(server2);
      const win = row.window;
      const ratio = win ? await ratioOfIllustration(server2, win.id || win.media) : 1;
      return {
        base: { kind: row.base.kind, id: row.base.id || "" },
        window: win ? { media: win.id || win.media, x: win.x, y: win.y, w: win.w, h: win.w / ratio } : null,
        view: row.view ? { zoom: row.view.z ?? row.view.zoom, panX: row.view.px ?? row.view.panX, panY: row.view.py ?? row.view.panY } : null
      };
    },
    // table.getBattlemapInstance (crates/table/capabilities/get-battlemap-instance):
    // the DM sees everything; the players no hidden token, no hit point figure, no
    // note — each visible NPC gives its HealthBand@1 band instead.
    "table.getBattlemapInstance": async (server2, { instance, view } = {}) => {
      const data = await instanceOf(server2, instance);
      if (!data) return data;
      const dressedTokens = (await dressed(server2, data.state.tokens)).map((t) => t.kind === "pnj" ? { ...t, dead: t.hp === 0 } : t);
      if (view !== "player") return { ...data, state: { ...data.state, tokens: dressedTokens } };
      const tokens = dressedTokens.filter((t) => t.visible).map((t) => ({ id: t.id, char: t.char, kind: t.kind, name: t.name, num: t.num, q: t.q, r: t.r, visible: true, states: t.states, dead: t.dead, initials: t.initials, color: t.color, portrait: t.portrait, healthBand: t.kind === "pnj" ? healthBand(t.hp, t.hpMax) : null }));
      const seen = new Set(tokens.map((t) => t.id));
      const combat = data.state.combat ? { ...data.state.combat, current: seen.has(data.state.combat.current) ? data.state.combat.current : "", order: data.state.combat.order.filter((o) => seen.has(o.id)) } : null;
      return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat } };
    },
    // session.getSession (crates/session/capabilities/get-session): one session's
    // header and the PCs present, a session of `session.listSessions` (the ones
    // created since included), its status labelled as SessionLifecycle@1 says. An
    // unknown id is no session: `null`, as the server answers it.
    "session.getSession": async (server2, { id }) => {
      const session = (await server2.load("session.listSessions")).find((s) => s.id === id);
      if (!session) return null;
      const status = session.status === "played" ? "closed" : session.status;
      return { id: session.id, order: session.order, title: session.title, date: session.date, status, statusLabel: STATUS_LABELS[status] ?? null, present: session.present };
    },
    // campaign.listImportedNotes (crates/campaign/capabilities/list-imported-notes):
    // the notes already imported, newest first, and the proposals of the latest
    // analysed note still awaiting validation. The imports are the fixture's
    // list; the proposals are what `ai.extractFromNotes` recorded (commands.js).
    "campaign.listImportedNotes": async (server2) => {
      const fixture = await server2.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture && fixture.imports || [];
      return { imports: [...imports].sort((x, y) => String(y.date).localeCompare(String(x.date))), proposals: server2.noteProposals || [] };
    },
    // soundbox.listSounds (crates/soundbox/capabilities/list-sounds): the
    // campaign's sounds with their effective gain — the own volume times the
    // master volume, both percents (EffectiveVolume@1) — and whether one plays
    // now (the fixture's `playing`, which soundbox.playSound and soundbox.stopAll move). The optional `search` keeps the
    // names that hold it and `tag` the sounds that carry it, whatever the case
    // (SoundTag@1); the fixture's `duration` is kept, the store has none yet.
    "soundbox.listSounds": async (server2, { search, tag }) => {
      const { masterVolume, sounds } = await server2.load("soundbox.listSounds");
      const lower = (text2) => String(text2 ?? "").trim().toLowerCase();
      return {
        masterVolume,
        sounds: sounds.filter((s) => lower(s.name).includes(lower(search)) && (!lower(tag) || s.tags.some((t) => lower(t) === lower(tag)))).map((s) => ({ ...s, gain: s.volume * masterVolume / 1e4, playing: s.playing === true }))
      };
    },
    // media.getBattlemap (crates/media/capabilities/get-battlemap): one
    // battlemap of the media library, by its id, its grid as HexGrid@1 states
    // it — the opacity a fraction of 1 where the mock-up's fixtures write a
    // percentage, and a hex always 5 feet. An illustration or an unknown id
    // is no battlemap: `null`, as the server answers it, not a `not-found`.
    "media.getBattlemap": async (server2, { id }) => {
      const battlemap = (await server2.load("media")).battlemaps.find((b) => b.id === id);
      if (!battlemap) return null;
      const { opacity, ...grid } = battlemap.grid;
      return { ...battlemap, grid: { ...grid, opacity: opacity / 100, feetPerHex: 5 } };
    },
    // session.listScenes (crates/session/capabilities/list-scenes): a session's scenes by rank. The fixture
    // is the scenes of session 4, the one in preparation; another session has none yet.
    "session.listScenes": async (server2, { session }) => session === "s4" ? await server2.load("session.listScenes") : [],
    // One element's history, newest first: the version list of `entity` in the fixture; an element the fixture
    // does not know but `entities` does has never been saved (an empty history), any other has no answer (null).
    "campaign.listEntityVersions": async (server2, { entity }) => {
      const history = (await server2.load("campaign.listEntityVersions"))[entity];
      if (history) return history;
      const entities = await server2.load("entities");
      const known = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === entity);
      return known ? { name: known.name, versions: [] } : null;
    },
    // campaign.getEntity (crates/campaign/capabilities/get-entity): one element's sheet, as the
    // Capability answers it — the lists `campaign.saveEntity` writes, so a save is read back. A PC or
    // an NPC has its stats with their modifier computed from the score (AbilityModifier@1), its token
    // (`tokenImage`: a dedicated image) and its D&D Beyond link; a place or an event has its kind
    // (`lieu`, `evenement`), aliases, description and notes alone. An unknown element is `null`.
    "campaign.getEntity": async (server2, { id }) => {
      const lists = [["campaign.listCharacters", null], ["campaign.listPlaces", "lieu"], ["campaign.listEvents", "evenement"]];
      for (const [name, kind] of lists) {
        const found = (await server2.load(name) || []).find((e) => e.id === id);
        if (!found) continue;
        const base = { id: found.id, name: found.name, aliases: found.aliases || [], kind: kind || found.kind, description: found.description || "", notes: found.notes || "" };
        if (kind) return base;
        const modifiers = Object.fromEntries(Object.entries(found.stats).map(([ability, score]) => [ability, Math.floor((score - 10) / 2)]));
        const initials = found.initials || found.name.split(/\s+/).map((w) => w.charAt(0)).join("").slice(0, 2).toUpperCase();
        return { ...base, player: found.kind === "pj" ? found.player || null : null, prompt: found.prompt || "", background: found.background || "", behavior: found.behavior || "", stats: found.stats, modifiers, ca: found.ca, pvMax: found.pvMax, features: found.features || [], color: found.color || "accent", initials, portrait: Boolean(found.portrait), tokenImage: Boolean(found.token || found.tokenImage), crop: found.crop || null, ddb: found.ddb && found.ddb.url ? found.ddb : null };
      }
      return null;
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
  var normalized = (term) => String(term ?? "").trim().toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "").normalize("NFC");
  var LISTS = { pj: "campaign.listCharacters", pnj: "campaign.listCharacters", lieu: "campaign.listPlaces", evenement: "campaign.listEvents" };
  var SAVED = ["kind", "name", "aliases", "description", "notes", "player", "prompt", "background", "behavior", "stats", "ca", "pvMax", "spells", "features", "actions", "portrait", "token", "crop", "color"];
  var elementOf = async (server2, id) => {
    for (const name of new Set(Object.values(LISTS))) {
      const element = (await server2.load(name) || []).find((e) => e.id === id);
      if (element) return { element };
    }
    return {};
  };
  var changedBy = async (server2, element, patch, origin, source, changes) => {
    Object.assign(element, patch);
    const history = await server2.load("campaign.listEntityVersions") || {};
    const entry = history[element.id] || (history[element.id] = { name: element.name, versions: [] });
    const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
    entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin, source, restoredFrom: null, seen: false, changes });
  };
  var onScreen = (fraction, kept2) => typeof fraction === "number" && Number.isFinite(fraction) ? Math.max(0, Math.min(1, fraction)) : kept2;
  async function onWindow(server2, change) {
    const row = await diffusionRow(server2);
    if (!row.window) return ["window-fields-together"];
    const violations = await change(row.window);
    if (violations) return violations;
    if (window.Mock) window.Mock.store.set("diffusion", row);
    return void 0;
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
  async function onToken(server2, target, token2, change) {
    const data = await instanceOf(server2, target && target.id);
    const found = data && data.state.tokens.find((t) => t.id === token2);
    if (!found) return ["instance-isolated"];
    const violations = change(found, data.state);
    if (violations) return violations;
    shareInstance(data);
    return void 0;
  }
  function dexModifier(characters, id) {
    const sheet2 = characters.find((c) => c.id === id);
    return Math.floor(((sheet2 && sheet2.stats ? sheet2.stats.DEX : 10) - 10) / 2);
  }
  function advance(combat, state) {
    const { order } = combat;
    const from = Math.max(0, order.findIndex((o) => o.id === combat.current));
    for (let step = 1; step <= order.length; step += 1) {
      const at = (from + step) % order.length;
      const token2 = state.tokens.find((t) => t.id === order[at].id);
      if (token2 && !token2.dead) {
        if (at <= from) combat.round += 1;
        combat.current = order[at].id;
        return;
      }
    }
  }
  async function onCombat(server2, id, change) {
    const data = await instanceOf(server2, id);
    if (!data) return ["instance-isolated"];
    if (!data.state.combat) return ["one-active-combat"];
    const violations = await change(data.state.combat, data.state);
    if (violations) return violations;
    shareInstance(data);
    return void 0;
  }
  var soundTags = (tags) => [...new Set((Array.isArray(tags) ? tags : []).map((t) => [...String(t).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "")].map((c) => c.toLowerCase()).join("")).filter(Boolean))];
  var COMMANDS = {
    // table.moveToken (crates/table/data-capabilities/move-token): (q, r) is the
    // one position both views read.
    "table.moveToken": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      t.q = payload.q;
      t.r = payload.r;
    }),
    // table.renameToken: only that token's name; an empty name is refused.
    "table.renameToken": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      if (!String(payload.name || "").trim()) return ["name-required"];
      t.name = String(payload.name).trim();
      return void 0;
    }),
    // table.setTokenVisibility: hidden tokens keep their place; the players do not read them.
    "table.setTokenVisibility": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      t.visible = payload.visible === true;
    }),
    // table.setTokenCondition (crates/table/data-capabilities/set-token-condition): the badge
    // `payload.state` put on that token when `payload.on`, after the others and once, or taken off,
    // the others keeping their order — its conditions alone. A state that is no name without the
    // blanks around it, or holds U+0000, or an `on` that is no boolean, is `state-persisted-reset`.
    "table.setTokenCondition": (server2, { payload, targetId }) => onToken(server2, { id: targetId || payload.instance }, payload.token, (t) => {
      const state = typeof payload.state === "string" ? payload.state.trim() : "";
      if (!state || state.includes("\0") || typeof payload.on !== "boolean") return ["state-persisted-reset"];
      const held = t.states || [];
      t.states = payload.on ? held.includes(state) ? held : [...held, state] : held.filter((s) => s !== state);
      return void 0;
    }),
    // table.removeToken: the token leaves the map.
    "table.removeToken": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t, state) => {
      state.tokens = state.tokens.filter((x) => x !== t);
    }),
    // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
    // hp-change-one-token — the targeted token only; pj-hp-not-tracked — a PC has
    // no hit points; the current ones stay within 0 and the maximum, and an NPC
    // is dead iff they are 0 (dead-derived).
    "table.changeHitPoints": (server2, { payload, targetId }) => onToken(server2, { id: targetId }, payload.token, (t) => {
      if (t.kind !== "pnj" || typeof t.hp !== "number") return ["pj-hp-not-tracked"];
      t.hp = Math.max(0, Math.min(t.hpMax, t.hp + Math.trunc(Number(payload.delta) || 0)));
      t.dead = t.hp === 0;
      return void 0;
    }),
    // table.startCombat: the NPC roll a d20 plus the DEX modifier of their sheet,
    // the DM enters the PCs' initiative; the order is the initiative, then the DEX,
    // then at random (initiative-tiebreak). A hidden token is in the order and keeps its rank.
    "table.startCombat": async (server2, { payload, targetId }) => {
      const data = await instanceOf(server2, targetId);
      if (!data) return ["instance-isolated"];
      if (data.state.combat) return ["one-active-combat"];
      const characters = await server2.load("campaign.listCharacters") || [];
      const rolls = payload.rolls || {};
      const entered = payload.initiative || {};
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
    "table.nextTurn": (server2, { targetId }) => onCombat(server2, targetId, (combat, state) => {
      advance(combat, state);
      return void 0;
    }),
    // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative): the open combat
    // reordered as `payload.order` gives it, every participant once, named by its token — the ranks
    // 1 to n in that order; the turn and the round stay. An order that is no list, omits a
    // participant, repeats one or names one not in the combat is `initiative-tiebreak`.
    "table.reorderInitiative": (server2, { payload, targetId }) => onCombat(server2, targetId || payload.instance, (combat) => {
      const named = Array.isArray(payload.order) ? payload.order : [];
      if (named.length !== combat.order.length) return ["initiative-tiebreak"];
      const order = [];
      for (const id of named) {
        const found = combat.order.find((o) => o.id === id && !order.includes(o));
        if (!found) return ["initiative-tiebreak"];
        order.push(found);
      }
      combat.order = order;
      return void 0;
    }),
    // table.addParticipant (crates/table/data-capabilities/add-participant): a token joins the running
    // order at the place its initiative earns. An NPC rolls a d20 plus its DEX modifier and is given no
    // figure; a PC takes the `payload.initiative` the DM entered (`npc-initiative-bounds`). A token not
    // on the instance is `not-found`; one already in the order is `initiative-tiebreak`.
    "table.addParticipant": (server2, { payload, targetId }) => onCombat(server2, targetId || payload.instance, async (combat, state) => {
      const token2 = state.tokens.find((t) => t.id === payload.token);
      if (!token2) return ["not-found"];
      if (combat.order.some((o) => o.id === token2.id)) return ["initiative-tiebreak"];
      const pc = token2.kind === "pj";
      if (pc ? !Number.isInteger(payload.initiative) : payload.initiative != null) return ["npc-initiative-bounds"];
      const dex = dexModifier(await server2.load("campaign.listCharacters") || [], token2.char);
      const init = pc ? payload.initiative : 1 + Math.floor(Math.random() * 20) + dex;
      const at = combat.order.findIndex((o) => o.init < init || o.init === init && o.dex < dex);
      combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token2.id, init, dex });
      return void 0;
    }),
    // table.removeParticipant (crates/table/data-capabilities/remove-participant): the participant
    // `payload.participant` — named by its token, as the carousel shows it — taken out of the open combat;
    // its token stays on the map and the order closes up. The participant that holds the turn is refused
    // by `next-turn`: the Battlemap sends table.nextTurn first. One not in the combat is `not-found`.
    "table.removeParticipant": (server2, { payload, targetId }) => onCombat(server2, targetId, (combat) => {
      if (!combat.order.some((o) => o.id === payload.participant)) return ["not-found"];
      if (combat.current === payload.participant) return ["next-turn"];
      combat.order = combat.order.filter((o) => o.id !== payload.participant);
      return void 0;
    }),
    // table.endCombat: the combat ends; the tokens stay as they are.
    "table.endCombat": (server2, { targetId }) => onCombat(server2, targetId, (combat, state) => {
      state.combat = null;
      return void 0;
    }),
    // table.resetBattlemap: state-persisted-reset — the instance's tokens, combat and initiative are removed: an empty map.
    "table.resetBattlemap": async (server2, { targetId }) => {
      const data = await instanceOf(server2, targetId);
      if (!data) return ["instance-isolated"];
      data.state = { tokens: [], combat: null };
      data.pjInitiative = {};
      shareInstance(data);
      return void 0;
    },
    // table.placeTokens: one character dropped, once or « × n »; the new tokens
    // are numbered after those already taken for it (multi-drop-numbering), a
    // monster takes its hit points from its sheet, a PC has one token.
    "table.placeTokens": async (server2, { payload, targetId }) => {
      const data = await instanceOf(server2, targetId);
      const character = (await server2.load("campaign.listCharacters") || []).find((c) => c.id === payload.character);
      if (!data || !character) return ["instance-isolated"];
      const { tokens } = data.state;
      const mine = tokens.filter((t) => t.char === character.id);
      const pnj = character.kind === "pnj";
      if (!pnj && (mine.length || !(data.roster || []).includes(character.id))) return ["token-roster"];
      const count = pnj ? Math.max(1, Math.min(12, Number(payload.count) || 1)) : 1;
      const base = character.tokenName || String(character.name).split(" ")[0];
      let num = Math.max(0, ...mine.map((t) => t.num || 0));
      const numbered = pnj && (count > 1 || mine.length > 0);
      for (const hex of freeHexes({ q: payload.q, r: payload.r }, tokens, count)) {
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
    // live.moveWindow (crates/live/data-capabilities/move-window): x and y alone,
    // brought back onto the screen (0 to 1); the size and the content stay.
    "live.moveWindow": (server2, { payload }) => onWindow(server2, (w) => {
      w.x = onScreen(payload.x, w.x);
      w.y = onScreen(payload.y, w.y);
    }),
    // live.closeWindow (crates/live/data-capabilities/close-window): the window's
    // media, position and width are emptied together (window-fields-together);
    // the base and the framing stay. A window already closed is closed again.
    "live.closeWindow": async (server2) => {
      const row = await diffusionRow(server2);
      row.window = null;
      if (window.Mock) window.Mock.store.set("diffusion", row);
      return void 0;
    },
    // live.resizeWindow (crates/live/data-capabilities/resize-window): only the width
    // is stored, the largest that fits the box asked for at the image's ratio — w
    // alone with no h — from 0.1 to the screen's width, to 4 decimals
    // (window-proportional); the position stays. A w that is no number, which the
    // server refuses, keeps the width.
    "live.resizeWindow": (server2, { payload }) => onWindow(server2, async (w) => {
      if (typeof payload.w !== "number" || !Number.isFinite(payload.w)) return void 0;
      const ratio = await ratioOfIllustration(server2, w.id || w.media);
      const fit = typeof payload.h === "number" && Number.isFinite(payload.h) ? Math.min(payload.w, payload.h * ratio) : payload.w;
      w.w = Math.round(Math.max(0.1, Math.min(1, fit)) * 1e4) / 1e4;
      return void 0;
    }),
    // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
    // a battlemap instance of the session, or an illustration of the campaign,
    // becomes the player view's base, framed as the DM framed it (1 and 0, 0
    // when no framing is sent); the floating window stays as it is. Any other
    // base would leave it incoherent (`base-coherent`).
    "live.showOnPlayerView": async (server2, { payload }) => {
      let base;
      if (payload.kind === "battlemap") {
        const data = await instanceOf(server2, payload.instance);
        if (!data) return ["base-coherent"];
        base = { kind: "battlemap", id: data.id };
      } else if (payload.kind === "illustration") {
        const listed = [await server2.load("media"), await server2.load("media.listMedia")];
        if (!listed.some((media) => media && media.illustrations.some((i) => i.id === payload.media))) return ["base-coherent"];
        base = { kind: "illustration", id: payload.media };
      } else return ["base-coherent"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.base = base;
        diffusion.view = { z: payload.zoom ?? 1, px: payload.panX ?? 0, py: payload.panY ?? 0 };
        store.set("diffusion", diffusion);
      }
      return void 0;
    },
    // live.showCover (crates/live/data-capabilities/show-cover): the player
    // view goes back to the campaign's cover, with neither a media nor an
    // instance as its base (`base-coherent`); the floating window and the
    // framing stay as they are.
    "live.showCover": async () => {
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.base = { kind: "cover" };
        store.set("diffusion", diffusion);
      }
      return void 0;
    },
    // live.showWindowOnPlayerView (crates/live/data-capabilities/show-window-on-player-view):
    // an illustration of the campaign opens in the floating window at its
    // default position and width, the three set together
    // (`window-fields-together`); its height follows the image's ratio and is
    // never stored (`window-proportional`). Another illustration replaces the
    // window's; the base and the framing stay as they are.
    "live.showWindowOnPlayerView": async (server2, { payload }) => {
      const id = payload.media ?? payload.id;
      if (!id) return payload.kind && payload.kind !== "illustration" ? ["window-proportional"] : void 0;
      const listed = [await server2.load("media"), await server2.load("media.listMedia")];
      if (!listed.some((media) => media && media.illustrations.some((i) => i.id === id))) return ["window-proportional"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.window = { id, x: 0.6, y: 0.12, w: 0.3 };
        store.set("diffusion", diffusion);
      }
      return void 0;
    },
    // soundbox.addSound (crates/soundbox/data-capabilities/add-sound): a sound
    // added to the library. One with no name or no file is refused before
    // anything is queued, as the server refuses it. Its tags, a list of texts,
    // are normalised (`tags-normalized`); its own volume is a percentage from
    // 0 to 100 — a number or the text of one — full when none is given
    // (`sound-volume-range`).
    "soundbox.addSound": async (server2, { payload }) => {
      const name = typeof payload.name === "string" ? payload.name.trim() : "";
      const file = typeof payload.file === "string" ? payload.file.trim() : "";
      if (!name || !file) return ["unprocessable"];
      const tags = payload.tags ?? [];
      if (!Array.isArray(tags) || !tags.every((t) => typeof t === "string")) return ["tags-normalized"];
      const given = payload.volume ?? 100;
      const volume = typeof given === "number" || typeof given === "string" && given.trim() !== "" ? Number(given) : NaN;
      if (!(volume >= 0 && volume <= 100)) return ["sound-volume-range"];
      const library = await server2.load("soundbox.listSounds");
      library.sounds.unshift({ id: server2.uuid(), name, tags: soundTags(tags), duration: 0, volume: Math.round(volume), file });
      return void 0;
    },
    // session.createSession (crates/session/data-capabilities/create-session):
    // the next session of the campaign, at the position after the last, in
    // preparation, with the PCs ticked present. `present-only-pj` refuses a
    // character that is not a PC of the fixtures; `oneshot-single-session` a
    // second session of a oneshot.
    "session.createSession": async (server2, { payload }) => {
      const sessions = await server2.load("session.listSessions");
      const characters = await server2.load("campaign.listCharacters");
      const campaign = (await server2.load("campaign.listCampaigns") || []).find((c) => c.id === payload.campaign);
      const present = Array.isArray(payload.present) ? payload.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
      if (campaign && campaign.type === "oneshot" && (campaign.sessions ?? 0) > 0) return ["oneshot-single-session"];
      if (campaign) campaign.sessions = (campaign.sessions ?? 0) + 1;
      sessions.push({
        id: server2.uuid(),
        order: sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1,
        title: String(payload.title || ""),
        date: payload.plannedOn || null,
        status: "prep",
        label: "En pr\xE9paration",
        present,
        summary: null
      });
      return void 0;
    },
    // session.addPresentCharacter (crates/session/data-capabilities/add-present-character):
    // one more PC present at the session — `targetId`, or `payload.session` —
    // listed in its `present` from then on. `present-only-pj` refuses a
    // character that is not a PC of the fixtures, which hold one campaign; a
    // session they do not list is `not-found`. A PC present already stays
    // present once.
    "session.addPresentCharacter": async (server2, { payload, targetId }) => {
      const session = (await server2.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
      if (!session) return ["not-found"];
      const characters = await server2.load("campaign.listCharacters");
      if (!characters.some((c) => c.kind === "pj" && c.id === payload.character)) return ["present-only-pj"];
      if (!session.present.includes(payload.character)) session.present.push(payload.character);
      return void 0;
    },
    // session.reorderScenes (crates/session/data-capabilities/reorder-scenes):
    // the session's scenes in the order the DM gives, every rank rewritten at
    // once, 1 to n. An order that omits a scene of the fixtures' session,
    // repeats one or names one it does not hold is `scene-order-contiguous`,
    // and no rank changes.
    "session.reorderScenes": async (server2, { payload }) => {
      const scenes = await server2.load("session.listScenes");
      const order = Array.isArray(payload.order) ? payload.order : [];
      const known = new Set(scenes.map((sc) => sc.id));
      if (order.length !== known.size || new Set(order).size !== order.length || order.some((id) => !known.has(id))) return ["scene-order-contiguous"];
      for (const sc of scenes) sc.order = order.indexOf(sc.id) + 1;
      scenes.sort((a, b) => a.order - b.order);
      return void 0;
    },
    // session.removePresentCharacter (crates/session/data-capabilities/remove-present-character):
    // a PC no longer present at the session — `targetId`, or `payload.session`
    // — taken out of its `present` (which `session.getSession` reads too), and
    // so no longer offered (`present-only-pj`); the character itself stays in
    // the fixtures. A PC who is not present changes nothing; a session they do
    // not list is `not-found`.
    "session.removePresentCharacter": async (server2, { payload, targetId }) => {
      const session = (await server2.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
      if (!session) return ["not-found"];
      session.present = session.present.filter((pc) => pc !== payload.character);
      return void 0;
    },
    // media.uploadMedia (crates/media/data-capabilities/upload-media): a
    // battlemap or an illustration added to the library, the path the DM chose
    // recorded and no file copied. A battlemap arrives with its default grid
    // (`grid-only-for-battlemap`) — a 32 px hex, pointy, 45 % opaque — at 5
    // feet per hex; an illustration has none. A place, when one is named, is a
    // lieu of the campaign (`media-place-is-lieu`); a kind that is neither is
    // refused (`media-kind`, the table's CHECK).
    "media.uploadMedia": async (server2, { payload }) => {
      const { kind, place = null } = payload;
      if (kind !== "battlemap" && kind !== "illustration") return ["media-kind"];
      if (place && !(await server2.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["media-place-is-lieu"];
      const base = { id: server2.uuid(), title: String(payload.title || "").trim() || "Sans titre", file: payload.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      for (const name of ["media", "media.listMedia"]) {
        const media = await server2.load(name);
        if (!media) continue;
        if (kind === "battlemap") media.battlemaps.push({ ...base, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 } });
        else media.illustrations.push({ ...base, ratio: payload.width > 0 && payload.height > 0 ? Math.round(payload.width / payload.height * 100) / 100 : null });
      }
      return void 0;
    },
    // media.saveGridSettings (crates/media/data-capabilities/save-grid-settings):
    // a battlemap's grid kept — the six fields of HexGrid@1, the opacity a
    // fraction of 1 where the mock-up's fixtures write a percentage. A hex size
    // of 0 or an opacity outside 0..1 is refused by `grid-values-range`; an
    // illustration or an unknown id has no grid (`grid-only-for-battlemap`).
    "media.saveGridSettings": async (server2, { payload, targetId }) => {
      const battlemap = (await server2.load("media")).battlemaps.find((b) => b.id === targetId);
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
    "media.setMediaPlace": async (server2, { payload, targetId }) => {
      const id = targetId || payload.media;
      const listed = [await server2.load("media"), await server2.load("media.listMedia")];
      const found = listed.flatMap((media) => media.battlemaps.concat(media.illustrations)).filter((m) => m.id === id);
      if (!found.length) return ["not-found"];
      const place = payload.place ?? null;
      if (place !== null && !(await server2.load("campaign.listPlaces")).some((p) => p.id === place)) return ["media-place-is-lieu"];
      for (const medium of found) medium.place = place;
      return void 0;
    },
    // session.addScene (crates/session/data-capabilities/add-scene): a new
    // scene, ranked after the session's last (`scene-order-contiguous`), with
    // its title, its notes and the places it links — each a place of the
    // fixtures' campaign, else `scene-links-same-campaign` — whose media
    // follow it to the session (`session-media-by-place`, derived, never
    // stored). Its notes are kept as written. A blank title is
    // `scene-title-required`: the mock answers violation ids, where the
    // server answers 422.
    "session.addScene": async (server2, { payload }) => {
      const title = String(payload.title || "").trim();
      if (!title) return ["scene-title-required"];
      const known = new Set((await server2.load("campaign.listPlaces")).map((p) => p.id));
      const places = [...new Set(Array.isArray(payload.places) ? payload.places : [])];
      if (places.some((id) => !known.has(id))) return ["scene-links-same-campaign"];
      const library = await server2.load("media.listMedia");
      const media = library.battlemaps.concat(library.illustrations).filter((m) => places.includes(m.place)).map((m) => m.id);
      const scenes = await server2.load("session.listScenes");
      scenes.push({
        id: server2.uuid(),
        order: scenes.reduce((last, sc) => Math.max(last, sc.order), 0) + 1,
        title,
        places,
        characters: [],
        events: [],
        media,
        toCreate: [],
        battlemap: null,
        notes: typeof payload.notes === "string" ? payload.notes : null
      });
      return void 0;
    },
    // session.addPlaceToSession (crates/session/data-capabilities/add-place-to-session):
    // a place of the campaign joins a scene of the session — `payload.scene`,
    // else the first — after its last element, and stays there once (a place
    // already in the scene is not linked twice). Its media follow by
    // `session-media-by-place`: they are listed in the scene's media, never
    // stored apart. A place that is no lieu of the campaign is refused by
    // `scene-links-same-campaign`; a scene the session does not have is
    // `not-found` — the server answers 422 `scene-links-same-campaign` for a
    // scene of another session, which the fixtures, one session's, never list.
    "session.addPlaceToSession": async (server2, { payload }) => {
      const place = payload.place;
      const scenes = await server2.load("session.listScenes") || [];
      const first = scenes.reduce((low, sc) => !low || sc.order < low.order ? sc : low, null);
      const scene = payload.scene ? scenes.find((sc) => sc.id === payload.scene) : first;
      if (!scene) return ["not-found"];
      if (!(await server2.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["scene-links-same-campaign"];
      if (!scene.places.includes(place)) scene.places.push(place);
      const library = await server2.load("media.listMedia") || { battlemaps: [], illustrations: [] };
      for (const medium of library.battlemaps.concat(library.illustrations)) {
        if (medium.place === place && !scene.media.includes(medium.id)) scene.media.push(medium.id);
      }
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
    "table.createBattlemapInstance": async (server2, { payload }) => {
      const session = await server2.load("session.getSession");
      const scene = (await server2.load("session.listScenes")).find((sc) => sc.id === payload.scene);
      const map = (await server2.load("media.listMedia")).battlemaps.find((m) => m.id === payload.battlemap);
      if (!session || session.id !== payload.session || !scene || !map) return ["not-found"];
      if (scene.battlemap) return ["one-instance-per-scene"];
      const id = `bmi-${map.id}-${scene.id}`;
      server2.instances = server2.instances || /* @__PURE__ */ new Map();
      server2.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
      scene.battlemap = id;
      return void 0;
    },
    "campaign.restoreEntityVersion": async (server2, { payload, targetId }) => {
      const history = (await server2.load("campaign.listEntityVersions"))[targetId];
      const target = history && history.versions.find((v) => v.v === payload.version);
      if (!target) return ["not-found"];
      const sheet2 = (upTo) => history.versions.slice().sort((a, b) => a.v - b.v).filter((v) => v.v <= upTo).reduce((fields, v) => {
        for (const c of v.changes) fields[c.field] = c.new;
        return fields;
      }, {});
      const before = sheet2(history.versions[0].v);
      let after = sheet2(target.v);
      if (Array.isArray(payload.changes)) {
        const undone = (history.versions.find((v) => v.v === payload.version + 1) || { changes: [] }).changes;
        after = { ...before };
        for (const i of payload.changes) if (undone[i]) after[undone[i].field] = undone[i].old;
      }
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].sort().filter((field) => (before[field] ?? "") !== (after[field] ?? "")).map((field) => ({ field, old: before[field] ?? "", new: after[field] ?? "" }));
      history.versions.unshift({ v: history.versions[0].v + 1, date: (/* @__PURE__ */ new Date()).toISOString(), origin: "manuelle", source: "restauration", restoredFrom: target.v, changes });
      return void 0;
    },
    // campaign.exportCampaign (crates/dataguard/data-capabilities/export-campaign):
    // stamps the campaign's last export with today's date and changes nothing
    // else (`export-stamps-date`). An unknown campaign is `not-found`.
    "campaign.exportCampaign": async (server2, { payload }) => {
      const campaign = await server2.load("campaign.getCampaign");
      if (!campaign || campaign.id !== payload.id) return ["not-found"];
      const now = /* @__PURE__ */ new Date();
      const pad = (n) => String(n).padStart(2, "0");
      campaign.lastExport = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
      return void 0;
    },
    // soundbox.updateSound (crates/soundbox/data-capabilities/update-sound): the
    // name, tags and volume of one sound, tags normalised and volume ranged as
    // for an addition; a sound that is not there is `not-found`.
    "soundbox.updateSound": async (server2, { payload, targetId }) => {
      const sound = (await server2.load("soundbox.listSounds")).sounds.find((s) => s.id === targetId);
      if (!sound) return ["not-found"];
      if ("volume" in payload && !(Number(payload.volume) >= 0 && Number(payload.volume) <= 100)) return ["sound-volume-range"];
      if ("name" in payload) sound.name = String(payload.name);
      if ("tags" in payload) sound.tags = soundTags(payload.tags);
      if ("volume" in payload) sound.volume = Number(payload.volume);
      if ("file" in payload) sound.file = String(payload.file);
      return void 0;
    },
    // soundbox.removeSound (crates/soundbox/data-capabilities/remove-sound): the
    // sound leaves the library's readers (a soft delete, its tombstone is the
    // server's).
    "soundbox.removeSound": async (server2, { targetId }) => {
      const library = await server2.load("soundbox.listSounds");
      const at = library.sounds.findIndex((s) => s.id === targetId);
      if (at < 0) return ["not-found"];
      library.sounds.splice(at, 1);
      return void 0;
    },
    // soundbox.setMasterVolume (crates/soundbox/data-capabilities/set-master-volume):
    // the master slider, in percent, kept to the whole percent; every sound
    // follows it, keeping their ratio (EffectiveVolume@1). A volume outside
    // 0..100, or no number, is refused by `master-volume-range`, never clamped;
    // a request naming no campaign is `not-found`.
    "soundbox.setMasterVolume": async (server2, { payload, targetId }) => {
      if (!String(targetId || payload.campaign || "").trim()) return ["not-found"];
      const { volume } = payload;
      if (typeof volume !== "number" || !(volume >= 0 && volume <= 100)) return ["master-volume-range"];
      for (const name of ["soundbox.listSounds", "sounds"]) {
        const fixture = await server2.load(name);
        if (fixture) fixture.masterVolume = Math.round(volume);
      }
      return void 0;
    },
    // soundbox.stopAll (crates/soundbox/data-capabilities/stop-all): every
    // running playback gets its stop (`stop-all`); with none running it still
    // succeeds and changes nothing.
    "soundbox.stopAll": async (server2) => {
      for (const sound of (await server2.load("soundbox.listSounds")).sounds) sound.playing = false;
      return void 0;
    },
    // soundbox.setSoundVolume (crates/soundbox/data-capabilities/set-sound-volume):
    // one sound's own volume, in percent, from 0 to 100; out of range it is
    // refused by `sound-volume-range`, never clamped. Nothing else of the sound
    // changes; its effective gain follows (EffectiveVolume@1).
    "soundbox.setSoundVolume": async (server2, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const fixtures = (await Promise.all(["soundbox.listSounds", "sounds"].map((name) => server2.load(name)))).filter(Boolean);
      const found = fixtures.map((fixture) => fixture.sounds.find((candidate) => candidate.id === id)).filter(Boolean);
      if (!found.length) return ["not-found"];
      const volume = payload?.volume;
      if (typeof volume !== "number" || !(volume >= 0 && volume <= 100)) return ["sound-volume-range"];
      for (const sound of found) sound.volume = volume;
      return void 0;
    },
    // campaign.createCampaign (crates/campaign/data-capabilities/create-campaign):
    // a new campaign at the top of the home page, its master volume at full
    // (`master-volume-range`); a oneshot comes with its single session, in
    // preparation (`oneshot-single-session`), a campagne with none. A campaign
    // with no name is refused, as one of a kind there is not.
    "campaign.createCampaign": async (server2, { payload }) => {
      const name = String(payload.name ?? "").trim();
      if (!name) return ["campaign-name-required"];
      if (!["campagne", "oneshot"].includes(payload.type)) return ["campaign-type-unknown"];
      const text2 = (field) => typeof payload[field] === "string" ? payload[field] : null;
      (await server2.load("campaign.listCampaigns")).unshift({
        id: server2.uuid(),
        name,
        type: payload.type,
        art: text2("art"),
        glyph: text2("glyph"),
        pitch: text2("pitch"),
        sessions: payload.type === "oneshot" ? 1 : 0,
        lastPlayed: null,
        next: null
      });
      return void 0;
    },
    // ai.saveSettings (crates/ai/data-capabilities/save-settings): the DM's
    // provider saved and made active. The API type is derived from the URL,
    // never taken from the payload: the cloud host by itself, the home server
    // at the URL whose model list the fixtures know; any other URL is refused
    // by `api-type-recognized` (« type d'API non reconnu »). `binary: claude`
    // is stored « claude-code ». The key is kept masked, never echoed.
    "ai.saveSettings": async (server2, { payload }) => {
      const settings = await server2.load("ai.getSettings");
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
    "ai.testConnection": async (server2, { payload }) => {
      const settings = await server2.load("ai.getSettings");
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
    "ai.extractFromNotes": async (server2, { payload }) => {
      const text2 = String(payload.text || "");
      const { proposals } = await server2.load("NoteImport");
      server2.noteText = text2;
      server2.noteProposals = proposals.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
      return void 0;
    },
    // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
    // one element per accepted proposal, the raw note kept as an import listed
    // newest first, the proposals no longer awaiting.
    "campaign.createEntitiesFromNote": async (server2, { payload }) => {
      const proposals = server2.noteProposals || [];
      const accepted = Array.isArray(payload.accepted) ? payload.accepted : [];
      const created = proposals.filter((p) => accepted.includes(p.id));
      const fixture = await server2.load("campaign.listImportedNotes");
      const imports = Array.isArray(fixture) ? fixture : fixture.imports;
      const note = server2.noteText || "";
      imports.unshift({ id: `imp-${server2.uuid()}`, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), title: note.split("\n")[0].replace(/[.\s]+$/, "") || "Notes import\xE9es", chars: note.length, proposed: proposals.length, created: created.length });
      server2.noteProposals = [];
      return void 0;
    },
    // campaign.saveEntity (crates/dataguard/data-capabilities/save-entity): the
    // DM's explicit save of an element, its sheet and its aliases, which adds a
    // version (EntityVersion@1, source `dm`) whose changes are the fields that
    // moved. An alias naming another element, by its name or an alias
    // (AliasMatch@1), is refused by `alias-unique-in-campaign`; a player on
    // anything but a PC by `character-is-pj-or-pnj`. Only the editor's fields
    // are kept: a DEX modifier given is never written.
    "campaign.saveEntity": async (server2, { payload }) => {
      const given = payload.entity || payload;
      const lists = await Promise.all(Object.values(LISTS).filter((v2, i, all2) => all2.indexOf(v2) === i).map(async (name) => [name, await server2.load(name) || []]));
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
      const id = before.id || given.id || server2.uuid();
      after.id = id;
      const list = await server2.load(LISTS[kind]) || [];
      if (found && found.name !== LISTS[kind]) {
        const old = lists.find(([name]) => name === found.name)[1];
        old.splice(old.indexOf(before), 1);
      }
      const at = list.indexOf(before);
      if (at >= 0) list[at] = after;
      else list.push(after);
      const history = await server2.load("campaign.listEntityVersions") || {};
      const entry = history[id] || (history[id] = { name: after.name, versions: [] });
      entry.name = after.name;
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => f !== "id").sort().filter((f) => JSON.stringify(before[f] ?? null) !== JSON.stringify(after[f] ?? null)).map((field) => ({ field, old: before[field] ?? null, new: after[field] ?? null }));
      const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
      entry.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin: "dm", source: "Fiche enregistr\xE9e par le DM", seen: true, changes });
      return void 0;
    },
    // ai.proposeCharacter (crates/ai/data-capabilities/propose-character): the
    // initial prompt told or typed gives the character's background and behaviour and, for an NPC,
    // a statblock — the proposals of fixtures/EntityEditor.json, the fake llm's answers. It applies
    // at once and is a version like any other (source ia), unseen until the DM looks. A prompt with
    // no text, or an element the lists do not hold, is refused.
    "ai.proposeCharacter": async (server2, { payload, targetId }) => {
      const { element } = await elementOf(server2, targetId);
      if (!element || !String(payload.prompt ?? "").trim()) return ["not-found"];
      const { proposals } = await server2.load("EntityEditor");
      const proposal = proposals[element.id] || proposals.default;
      const patch = { prompt: payload.prompt, background: proposal.background, behavior: proposal.behavior };
      if (element.kind === "pnj") Object.assign(patch, { stats: { ...proposal.stats }, ca: proposal.ca, pvMax: proposal.pvMax, features: proposal.features.map((f) => ({ ...f })) });
      await changedBy(server2, element, patch, "ia", "Prompt initial", element.kind === "pnj" ? proposal.changes : proposal.changes.slice(0, 1));
      return void 0;
    },
    // dndbeyond.resyncCharacter (crates/dndbeyond/data-capabilities/resync-character, planned): the
    // stats and hit points of a PC linked to D&D Beyond are read again — the patch of
    // fixtures/EntityEditor.json, whose changes the history keeps — and the aliases, the notes and the token are left as they are
    // (`dndbeyond-resync-scope`). A sheet with no link is refused.
    "dndbeyond.resyncCharacter": async (server2, { targetId }) => {
      const { element } = await elementOf(server2, targetId);
      if (!element || !element.ddb || !element.ddb.url) return ["not-found"];
      const { ddbResync } = await server2.load("EntityEditor");
      const resync = ddbResync[element.id] || ddbResync.default;
      const { stats, pvMax } = resync.patch;
      await changedBy(server2, element, { ...stats ? { stats: { ...element.stats, ...stats } } : {}, ...pvMax ? { pvMax } : {}, ddb: { ...element.ddb, syncedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19) } }, "dndbeyond", "Resynchronisation D&D Beyond", resync.changes.filter((c) => c.field === "PV max" || stats && c.field in stats));
      return void 0;
    },
    // dndbeyond.importCharacter (crates/dndbeyond/data-capabilities/import-character, planned): a
    // PC of the campaign linked to that D&D Beyond URL is updated (upsert); a URL that is not
    // D&D Beyond's is refused, and a URL no sheet is linked to creates nothing on the mock-up.
    "dndbeyond.importCharacter": async (server2, { payload }) => {
      if (!/^https:\/\/(www\.)?dndbeyond\.com\/characters\/\d+/.test(String(payload.url ?? ""))) return ["not-found"];
      return void 0;
    },
    // soundbox.playSound (crates/soundbox/data-capabilities/play-sound): a
    // playback of the sound starts, whatever else plays (`simultaneous-playback`),
    // from the soundbox (the default) or a suggestion's pastille; any other
    // source, the player view's, is refused (`dm-side-only`). The sound plays
    // as soundbox.listSounds says it: `playing`.
    "soundbox.playSound": async (server2, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const sound = (await server2.load("soundbox.listSounds")).sounds.find((candidate) => candidate.id === id);
      if (!sound) return ["not-found"];
      const source = payload?.source ?? "soundbox";
      if (!["soundbox", "suggestion"].includes(source)) return ["dm-side-only"];
      sound.playing = true;
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
