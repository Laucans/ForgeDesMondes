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

  // apps/table/battlemap/src/ids.ts
  var GET_BATTLEMAP_INSTANCE = "table.getBattlemapInstance";
  var GET_BATTLEMAP = "media.getBattlemap";
  var LIST_CHARACTERS = "campaign.listCharacters";
  var MOVE_TOKEN = "table.moveToken";
  var PLACE_TOKENS = "table.placeTokens";
  var REMOVE_TOKEN = "table.removeToken";
  var RENAME_TOKEN = "table.renameToken";
  var SET_TOKEN_VISIBILITY = "table.setTokenVisibility";
  var SET_TOKEN_CONDITION = "table.setTokenCondition";
  var SHOW_ON_PLAYER_VIEW = "live.showOnPlayerView";
  var AGGREGATE = "BattlemapInstance";
  var DATA_CAPABILITY_VERSION = 1;

  // apps/table/battlemap/src/hexgrid.ts
  var FEET_PER_HEX = 5;
  var SQRT3 = Math.sqrt(3);
  var feet = (hexes) => typeof hexes === "number" && Number.isInteger(hexes) && hexes >= 0 ? hexes * FEET_PER_HEX : null;
  var label = (hexes) => `${hexes} hex \xB7 ${feet(hexes) ?? 0} pi`;
  function center(grid2, hex) {
    const s = grid2.size;
    const [x, y] = grid2.orient === "pointy" ? [s * SQRT3 * (hex.q + hex.r / 2), s * 1.5 * hex.r] : [s * 1.5 * hex.q, s * SQRT3 * (hex.r + hex.q / 2)];
    return { x: x + grid2.ox + s, y: y + grid2.oy + s };
  }
  function round(q, r) {
    const x = q;
    const z = r;
    const y = -x - z;
    let rx = Math.round(x);
    const ry0 = Math.round(y);
    let rz = Math.round(z);
    const dx = Math.abs(rx - x);
    const dy = Math.abs(ry0 - y);
    const dz = Math.abs(rz - z);
    if (dx > dy && dx > dz) rx = -ry0 - rz;
    else if (dy <= dz) rz = -rx - ry0;
    return { q: rx + 0, r: rz + 0 };
  }
  function toHex(grid2, point) {
    const s = grid2.size;
    const x = point.x - grid2.ox - s;
    const y = point.y - grid2.oy - s;
    return grid2.orient === "pointy" ? round((SQRT3 / 3 * x - y / 3) / s, 2 / 3 * y / s) : round(2 / 3 * x / s, (-x / 3 + SQRT3 / 3 * y) / s);
  }
  function distance(a, b) {
    const dq = a.q - b.q;
    const dr = a.r - b.r;
    return (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2;
  }
  function line(a, b) {
    const n = distance(a, b);
    const out = [];
    for (let i = 0; i <= n; i += 1) {
      const t = n === 0 ? 0 : i / n;
      out.push(round(a.q + (b.q - a.q) * t + 1e-6, a.r + (b.r - a.r) * t + 2e-6));
    }
    return out;
  }
  function corners(grid2, hex) {
    const c = center(grid2, hex);
    const offset = grid2.orient === "pointy" ? -30 : 0;
    return Array.from({ length: 6 }, (_, i) => {
      const angle = (60 * i + offset) * Math.PI / 180;
      return { x: c.x + grid2.size * Math.cos(angle), y: c.y + grid2.size * Math.sin(angle) };
    });
  }
  var points = (list) => list.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ");
  var polygon = (grid2, hex) => points(corners(grid2, hex));
  var trail = (grid2, hexes) => points(hexes.map((h) => center(grid2, h)));
  function onMap(grid2, hex, width, height) {
    const c = center(grid2, hex);
    return c.x >= 0 && c.x <= width && c.y >= 0 && c.y <= height;
  }
  function gridPath(grid2, width, height) {
    if (!(grid2.size > 0)) return "";
    const s = grid2.size;
    const n = Math.ceil(Math.max(width, height) / s) + 2;
    let d = "";
    for (let q = -n; q <= n; q += 1) {
      for (let r = -n; r <= n; r += 1) {
        const c = center(grid2, { q, r });
        if (c.x < -s || c.x > width + s || c.y < -s || c.y > height + s) continue;
        d += `M${corners(grid2, { q, r }).map((p) => `${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join("L")}Z`;
      }
    }
    return d;
  }

  // apps/table/battlemap/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var num = (value, fallback) => typeof value === "number" && Number.isFinite(value) ? value : fallback;
  var orNull = (value) => typeof value === "number" && Number.isFinite(value) ? value : null;
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var CONDITIONS = {
    "\xC0 terre": "\u2193",
    "Empoisonn\xE9": "\u2620",
    "Effray\xE9": "!",
    "\xC9tourdi": "\u2726",
    "Agripp\xE9": "\u270A",
    "Invisible": "?",
    "Concentration": "\u25CE"
  };
  var MAX_COUNT = 12;
  function parseProps(raw) {
    const view = raw.view === void 0 || raw.view === "dm" ? "dm" : raw.view === "prep" ? "prep" : "player";
    return { instance: text(raw.instance), view, pilot: raw.pilot === true, fill: raw.fill === true, campaign: text(raw.campaign) };
  }
  function token(value) {
    const name = text(value.name);
    return {
      id: text(value.id),
      char: text(value.char),
      kind: value.kind === "pnj" ? "pnj" : "pj",
      name,
      num: orNull(value.num),
      q: num(value.q, 0),
      r: num(value.r, 0),
      visible: value.visible !== false,
      hp: orNull(value.hp),
      hpMax: orNull(value.hpMax),
      healthBand: text(value.healthBand ?? value.health_band ?? value.band) || null,
      states: texts(value.states),
      dead: value.dead === true,
      initials: text(value.initials) || name.slice(0, 2).toUpperCase(),
      color: text(value.color),
      portrait: value.portrait === true
    };
  }
  function parseInstance(data) {
    if (!isRecord(data)) return null;
    const state = isRecord(data.state) ? data.state : data;
    const tokens = Array.isArray(state.tokens) ? state.tokens.filter(isRecord).map(token) : [];
    return { id: text(data.id), battlemap: text(data.battlemap), session: text(data.session), label: text(data.label), roster: texts(data.roster), tokens };
  }
  var GRID = { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 0.45 };
  function grid(value) {
    const g = isRecord(value) ? value : {};
    return {
      size: num(g.size, 0) > 0 ? num(g.size, GRID.size) : GRID.size,
      ox: num(g.ox, GRID.ox),
      oy: num(g.oy, GRID.oy),
      orient: g.orient === "flat" ? "flat" : "pointy",
      color: text(g.color) || GRID.color,
      opacity: Math.min(1, Math.max(0, num(g.opacity, GRID.opacity)))
    };
  }
  var SHAPES = /* @__PURE__ */ new Set(["rect", "ellipse", "circle", "path"]);
  var ATTRIBUTES = /* @__PURE__ */ new Set(["x", "y", "width", "height", "cx", "cy", "rx", "ry", "r", "d", "class"]);
  function shapes(value) {
    if (!Array.isArray(value)) return [];
    return value.filter(isRecord).filter((s) => SHAPES.has(text(s.tag))).map((s) => ({
      tag: text(s.tag),
      attrs: Object.fromEntries(Object.entries(s).filter(([k, v]) => ATTRIBUTES.has(k) && (typeof v === "string" || typeof v === "number")).map(([k, v]) => [k, String(v)]))
    }));
  }
  function parseMap(data) {
    if (!isRecord(data)) return null;
    return { id: text(data.id), title: text(data.title), width: num(data.width, 800), height: num(data.height, 560), grid: grid(data.grid), terrain: shapes(data.terrain) };
  }
  function parseCharacters(data) {
    const out = {};
    if (!Array.isArray(data)) return out;
    for (const entry of data) {
      if (!isRecord(entry) || text(entry.id) === "") continue;
      const name = text(entry.name);
      out[text(entry.id)] = {
        id: text(entry.id),
        name,
        tokenName: text(entry.tokenName) || name.split(" ")[0] || name,
        kind: entry.kind === "pnj" ? "pnj" : "pj",
        initials: text(entry.initials) || name.slice(0, 2).toUpperCase(),
        color: text(entry.color),
        portrait: entry.portrait === true
      };
    }
    return out;
  }
  var shown = (tokens, view) => view === "player" ? tokens.filter((t) => t.visible) : [...tokens];
  var bandLabel = (band) => band === null || band === "" ? "" : band.charAt(0).toUpperCase() + band.slice(1);
  var nextNumber = (tokens, char) => Math.max(0, ...tokens.filter((t) => t.char === char).map((t) => t.num ?? 0)) + 1;
  var existingToken = (tokens, character) => character.kind === "pj" ? tokens.find((t) => t.char === character.id) : void 0;
  var countOf = (raw) => Math.max(1, Math.min(MAX_COUNT, Math.floor(Number(raw)) || 1));
  function numbering(tokens, character, count) {
    const first = nextNumber(tokens, character.id);
    if (count > 1) return `Num\xE9rot\xE9es de ${character.tokenName} ${first} \xE0 ${character.tokenName} ${first + count - 1}, renommables`;
    return first > 1 ? `Num\xE9rot\xE9e ${character.tokenName} ${first}, renommable` : "Un seul token, renommable";
  }
  var withCondition = (states, condition) => states.includes(condition) ? states.filter((s) => s !== condition) : [...states, condition];
  function commandBody(dataCapability, aggregate, id, payload) {
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, target: id === void 0 ? { aggregate } : { aggregate, id }, payload, caller: { by: "dm", from: "Battlemap", roles: [] } };
  }
  var tableCommand = (dataCapability, instance, payload) => commandBody(dataCapability, AGGREGATE, instance, payload);
  var VIOLATIONS = {
    "token-roster": "un PJ absent de la s\xE9ance, ou d\xE9j\xE0 sur la carte, ne peut pas \xEAtre plac\xE9",
    "instance-isolated": "ce token n\u2019est pas sur cette carte",
    "base-coherent": "cette carte n\u2019est pas celle de la s\xE9ance",
    "unhandled-data-capability": "commande inconnue du serveur"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  var isFinal = (status) => FINAL.has(status);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    return { status: text(result.status), violations: texts(result.violations) };
  }
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }
  var SCALE = `1 hex = ${FEET_PER_HEX} pi`;

  // apps/table/battlemap/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var attr = (name, value) => value === "" ? "" : ` ${name}="${esc(value)}"`;
  var tone = (color) => /^[a-z][a-z-]*$/.test(color) ? color : "secondary";
  function skeleton(props2) {
    const modes = `${props2.fill ? " bm--fill" : ""}${props2.view === "player" && !props2.pilot ? " bm--readonly" : ""}`;
    return `<div class="bm${modes}" id="battlemap" data-reads="${GET_BATTLEMAP_INSTANCE}"><div class="bm-toolbar" id="battlemap-toolbar"></div><div class="bm-body" id="battlemap-body"><aside class="bm-side" id="battlemap-roster" aria-labelledby="battlemap-roster-title" data-reads="${LIST_CHARACTERS}" hidden></aside><div class="bm-map" id="battlemap-map" data-reads="${GET_BATTLEMAP}"></div><aside class="bm-side" id="token-inspector" aria-labelledby="inspector-title" aria-live="polite" hidden></aside></div><p class="bm-status" id="battlemap-status" role="status" aria-live="polite" hidden></p></div>`;
  }
  function button(p) {
    return `<button type="button" class="${p.variant}" id="${p.id}"${attr("data-writes", p.writes ?? "")}${p.disabled ? " disabled" : ""}>${esc(p.label)}</button>`;
  }
  function toolbar(map, instance, props2) {
    if (props2.view === "player") return "";
    const hidden = instance.tokens.filter((t) => !t.visible).length;
    const count = `${instance.tokens.length} token${instance.tokens.length > 1 ? "s" : ""}${hidden > 0 ? ` \xB7 ${hidden} cach\xE9${hidden > 1 ? "s" : ""}` : ""}`;
    return `<div class="bm-title"><h2 id="battlemap-title">${esc(map.title)}</h2><span class="small muted">${esc(instance.label)} \xB7 ${count} \xB7 ${SCALE}</span></div><div class="kit-button kit-button--bare kit-button--small">${button({ id: "bm-send", label: "Envoyer aux joueurs", variant: "secondary", writes: SHOW_ON_PLAYER_VIEW })}</div>`;
  }
  function shape(tag, attrs) {
    return `<${tag}${Object.entries(attrs).map(([k, v]) => attr(k, v)).join("")}></${tag}>`;
  }
  function hexMap(map, canMove) {
    const box = `viewBox="0 0 ${map.width} ${map.height}" width="${map.width}" height="${map.height}"`;
    const land = map.terrain.length === 0 ? `<rect class="hm-land" x="0" y="0" width="${map.width}" height="${map.height}"></rect>` : "";
    return '<div class="kit-hexmap">' + (canMove ? '<div class="hm-bar"><span class="hm-zoom"><button type="button" data-act="out" aria-label="D\xE9zoomer">\u2212</button><button type="button" data-act="reset">Ajuster</button><button type="button" data-act="in" aria-label="Zoomer">+</button></span></div>' : "") + `<div class="hm-view" tabindex="0" id="battlemap-view" aria-label="${esc(`Battlemap ${map.title}`)}"><div class="hm-stage" id="battlemap-stage" style="width:${map.width}px;height:${map.height}px"><svg class="hm-layer hm-map" ${box} aria-hidden="true">${land}${map.terrain.map((s) => shape(s.tag, s.attrs)).join("")}</svg><svg class="hm-layer hm-grid" ${box} aria-hidden="true"><path class="hm-grid-path" id="battlemap-grid" d=""></path></svg><svg class="hm-layer hm-over" id="battlemap-over" ${box}></svg><div class="kit-token bm-tokens" id="battlemap-tokens"${attr("data-writes", canMove ? MOVE_TOKEN : "")}></div></div><div class="hm-tip" id="battlemap-tip" hidden></div></div></div>`;
  }
  function tokenItem(t, grid2, p) {
    const player = p.view === "player";
    const c = center(grid2, t);
    const initials = p.character?.initials ?? t.initials;
    const color = p.character?.color || t.color;
    const cls = ["kit-token__item", !t.visible && "kit-token__item--hidden", t.dead && "kit-token__item--dead", p.selected && "bm-selected"].filter(Boolean).join(" ");
    const badges = t.states.slice(0, 3).map((s, i) => `<span class="kit-token__badge kit-token__badge--${["tl", "tr", "br"][i]}" title="${esc(s)}">${esc(CONDITIONS[s] ?? "\u2022")}</span>`).join("");
    let health = "";
    if (player) {
      if (t.kind === "pnj" && t.healthBand) health = `<div class="kit-token__hp"><span class="kit-token__hp-label">${esc(bandLabel(t.healthBand))}</span></div>`;
    } else if (t.kind === "pnj" && t.hp !== null && t.hpMax !== null) {
      const pct = t.hpMax > 0 ? Math.round(100 * Math.max(0, t.hp) / t.hpMax) : 0;
      const fill = pct <= 25 ? " kit-token__gauge-fill--low" : pct <= 50 ? " kit-token__gauge-fill--mid" : "";
      health = `<div class="kit-token__hp"><div class="kit-token__gauge" style="--kit-token-hp: ${pct}%"><div class="kit-token__gauge-fill${fill}"></div></div><span class="kit-token__hp-label">${t.hp}/${t.hpMax} PV</span></div>`;
    }
    const eye = '<div class="kit-token__eye"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-label="Cach\xE9"><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/><path d="M3 3l18 18"/></svg></div>';
    return `<div class="${cls}" id="token-${esc(t.id)}" data-token="${esc(t.id)}" style="left:${c.x.toFixed(1)}px;top:${c.y.toFixed(1)}px" title="${esc(t.name + (t.visible ? "" : " \u2014 cach\xE9 aux joueurs"))}"><div class="kit-token__disc" style="--kit-token-bg: var(--color-${tone(color)})"><div class="kit-token__clip"><span class="kit-token__initials">${esc(initials)}</span></div>${t.visible ? "" : eye}${t.dead ? '<span class="kit-token__strike"></span>' : ""}${badges}${t.num === null ? "" : `<span class="kit-token__num">${t.num}</span>`}</div><div class="kit-token__name">${esc(t.name)}</div>${health}</div>`;
  }
  function pathMarkup(grid2, hexes) {
    return hexes.map((h, i) => `<polygon class="${i === hexes.length - 1 ? "hm-end" : "hm-step"}" points="${polygon(grid2, h)}"></polygon>`).join("") + `<polyline class="hm-line" points="${trail(grid2, hexes)}"></polyline>`;
  }
  function inspector(t, siblings, character) {
    const chips = siblings.length > 1 ? `<div class="row" id="inspector-instances" role="group" aria-label="Instances de ${esc(character?.name ?? t.name)}">${siblings.map((x) => `<button type="button" class="chip${x.id === t.id ? " is-on" : ""}" data-select="${esc(x.id)}">${esc(x.name)}${x.dead ? " \u2020" : ""}</button>`).join("")}</div>` : "";
    const conditions = Object.entries(CONDITIONS).map(([name, glyph]) => `<button type="button" class="chip${t.states.includes(name) ? " is-on" : ""}" data-state="${esc(name)}" aria-pressed="${t.states.includes(name)}" data-writes="${SET_TOKEN_CONDITION}">${glyph} ${esc(name)}</button>`).join("");
    return `<div class="panel__head"><h3 id="inspector-title">${esc(t.name)}</h3><button type="button" class="icon-btn" data-act="close" aria-label="Fermer le menu du token">\xD7</button></div><p class="small muted">${esc(character?.name ?? "")}</p>` + chips + `<div class="kit-input is-wide"><label>Nom affich\xE9<input type="text" id="inspector-name" value="${esc(t.name)}" data-writes="${RENAME_TOKEN}"></label></div><div class="kit-toggle kit-toggle--bare"><label><button type="button" role="switch" id="inspector-visible" aria-checked="${t.visible}" data-writes="${SET_TOKEN_VISIBILITY}"><i></i></button><span>${t.visible ? "Visible des joueurs" : "Cach\xE9 aux joueurs"}</span></label></div><div id="inspector-states" class="stack"><h3>\xC9tats</h3><div class="row">${conditions}</div></div>` + (t.dead ? '<p class="bm-dead" id="inspector-dead">Mort \u2014 gris\xE9.</p>' : "") + `<div class="kit-button kit-button--bare kit-button--small">${button({ id: "inspector-remove", label: t.dead ? "Retirer le corps" : "Retirer de la carte", variant: "danger", writes: REMOVE_TOKEN })}</div>`;
  }
  function roster(characters) {
    return `<h3 id="battlemap-roster-title">Personnages de la sc\xE8ne</h3><p class="small muted">Glissez un personnage sur la carte, ou cliquez pour choisir le nombre d\u2019instances (\xD7 N).</p><ul class="list" id="battlemap-roster-list">${characters.map((c) => `<li><button type="button" class="chip chip--${c.kind}" draggable="true" data-writes="${PLACE_TOKENS}" data-char="${esc(c.id)}" id="roster-${esc(c.id)}">\u283F ${esc(c.name)}</button></li>`).join("")}</ul>`;
  }
  function dropForm(character, tokens, count) {
    return `<b>Placer ${esc(character.name)}</b>` + (character.kind === "pnj" ? `<label class="row small">\xD7 <input type="number" class="num-input" id="battlemap-drop-count" min="1" max="${MAX_COUNT}" value="${count}"> instance(s)</label><span class="small muted" id="battlemap-drop-numbers">${esc(numbering(tokens, character, count))}</span>` : '<span class="small muted">Un PJ n\u2019a qu\u2019un token sur la carte.</span>') + `<div class="kit-button kit-button--bare kit-button--small">${button({ id: "battlemap-drop-ok", label: "Placer", variant: "primary", writes: PLACE_TOKENS })}${button({ id: "battlemap-drop-cancel", label: "Annuler", variant: "ghost" })}</div>`;
  }

  // apps/table/battlemap/src/style.ts
  var STYLE = `
.bm { box-sizing: border-box; display: flex; flex-direction: column; gap: var(--space-3); min-width: 0; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.bm *, .bm *::before, .bm *::after { box-sizing: inherit; }
.bm--fill { height: 100%; }
.bm h2, .bm h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.bm h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.bm h3 { font-size: var(--font-size-base); color: var(--color-text); }
.bm p { margin: 0; }
.bm .muted { color: var(--color-text-muted); }
.bm .small { font-size: var(--font-size-small); }
.bm .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.bm .stack { display: grid; gap: var(--space-3); align-content: start; }
.bm .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.bm .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.bm .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.bm .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.bm .chip.is-on { background: var(--color-primary); border-color: var(--color-primary); color: var(--color-primary-contrast); font-weight: var(--font-weight-heading); }
.bm .chip--pj { border-color: var(--color-success); }
.bm .chip--pnj { border-color: var(--color-secondary); }
.bm .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.bm .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.bm .num-input { width: calc(var(--space-5) * 2); padding: var(--space-1) var(--space-2); background: var(--color-surface); color: var(--color-text); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-small); font: inherit; }
.bm .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.bm .kit-button--small button { font-size: var(--font-size-small); padding: var(--space-1) var(--space-3); }
.bm .kit-button .danger { background: var(--color-danger); color: var(--color-primary-contrast); }
.bm .kit-input.is-wide { max-width: none; }
.bm .kit-toggle.kit-toggle--bare { padding: 0; background: transparent; border: 0; box-shadow: none; }
.bm-toolbar { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.bm-toolbar:empty { display: none; }
.bm-title { display: grid; gap: var(--space-1); }
.bm-body { display: flex; gap: var(--space-3); min-height: 0; flex: 1; }
.bm-map { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.bm .kit-hexmap { flex: 1; display: flex; flex-direction: column; min-height: 0; padding: var(--space-2); }
.bm .kit-hexmap .hm-bar { margin-bottom: var(--space-2); }
.bm .kit-hexmap .hm-view { flex: 1; height: auto; min-height: calc(var(--space-5) * 13); }
.bm--fill .kit-hexmap { padding: 0; border: 0; border-radius: 0; background: transparent; box-shadow: none; }
.bm--fill .kit-hexmap .hm-view { min-height: 0; border: 0; border-radius: 0; }
.bm--fill .kit-hexmap .hm-bar { position: absolute; right: var(--space-3); bottom: var(--space-3); z-index: 4; margin: 0; }
.bm-side { flex: none; width: calc(var(--space-5) * 7); display: grid; gap: var(--space-2); align-content: start; padding: var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); overflow-y: auto; }
.bm-side[hidden] { display: none; }
.bm-dead { color: var(--color-danger); font-weight: var(--font-weight-heading); }
.bm-status { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.bm-status[hidden] { display: none; }
.bm-status.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.bm-status.is-ok { border-color: var(--color-success); color: var(--color-success); }
.bm-tokens.kit-token { position: absolute; inset: 0; display: block; padding: 0; background: transparent; pointer-events: none; }
.bm-tokens .kit-token__item { position: absolute; transform: translate(-50%, calc((var(--space-4) + var(--space-2)) * -1)) scale(var(--bm-scale, 0.8)); transform-origin: 50% calc(var(--space-4) + var(--space-2)); pointer-events: auto; cursor: grab; touch-action: none; }
.bm-tokens .kit-token__item--carried { transform: translate(-50%, calc((var(--space-4) + var(--space-2)) * -1)) scale(calc(var(--bm-scale, 0.8) * 1.2)); z-index: 3; cursor: grabbing; transition: none; }
.bm-tokens .kit-token__name { text-shadow: var(--shadow-low); }
.bm--readonly .bm-tokens .kit-token__item { pointer-events: none; cursor: default; }
.bm .bm-selected .kit-token__disc { outline: calc(var(--border-width) * 3) solid var(--color-primary); outline-offset: calc(var(--border-width) * 3); }
.bm-drop { position: absolute; z-index: 5; display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-primary); border-radius: var(--radius-medium); box-shadow: var(--shadow-high); cursor: default; }
@media (max-width: 960px) { .bm-body { flex-direction: column; } .bm-side { width: auto; } }
`;

  // apps/table/battlemap/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 500 };
  var DRAG_THRESHOLD = 4;
  var CHARACTER_TYPE = "text/fdm-character";
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="Battlemap"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "Battlemap";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, raw, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    let props2 = parseProps(raw);
    let instance = null;
    let map = null;
    let characters = {};
    let selected = null;
    let frame = { zoom: 1, panX: 0, panY: 0 };
    let fitted = false;
    let drag = null;
    let pan = null;
    let stale = false;
    let generation = 0;
    let readNo = 0;
    let stopWatching = () => void 0;
    let observer = null;
    const part = (id) => host2.querySelector(`#${id}`);
    const canMove = () => props2.view !== "player" || props2.pilot;
    const dm = () => props2.view !== "player";
    function say(kind, text2 = "") {
      const box = part("battlemap-status");
      if (!box) return;
      box.hidden = kind === "none";
      box.className = `bm-status${kind === "none" ? "" : ` is-${kind}`}`;
      box.textContent = text2;
    }
    function applyFrame() {
      const stage = part("battlemap-stage");
      if (stage) stage.style.transform = `translate(${frame.panX}px,${frame.panY}px) scale(${frame.zoom})`;
    }
    function fit() {
      const view = part("battlemap-view");
      if (!view || !map || !view.clientWidth || !view.clientHeight) return false;
      const zoom = Math.min(view.clientWidth / map.width, view.clientHeight / map.height);
      frame = { zoom, panX: (view.clientWidth - map.width * zoom) / 2, panY: (view.clientHeight - map.height * zoom) / 2 };
      fitted = true;
      applyFrame();
      return true;
    }
    function zoomAt(factor, cx, cy) {
      const zoom = Math.max(0.3, Math.min(4, frame.zoom * factor));
      const k = zoom / frame.zoom;
      frame = { zoom, panX: cx - (cx - frame.panX) * k, panY: cy - (cy - frame.panY) * k };
      fitted = true;
      applyFrame();
    }
    function world(e) {
      const box = part("battlemap-view")?.getBoundingClientRect();
      const lx = e.clientX - (box?.left ?? 0);
      const ly = e.clientY - (box?.top ?? 0);
      return { lx, ly, x: (lx - frame.panX) / frame.zoom, y: (ly - frame.panY) / frame.zoom };
    }
    function paintToolbar() {
      const box = part("battlemap-toolbar");
      if (box && map && instance) box.innerHTML = toolbar(map, instance, props2);
    }
    function paintTokens() {
      const layer = part("battlemap-tokens");
      if (!layer || !map || !instance) return;
      if (drag?.moved) {
        stale = true;
        return;
      }
      stale = false;
      const grid2 = map.grid;
      layer.style.setProperty("--bm-scale", (grid2.size * 1.7 / 64).toFixed(3));
      layer.innerHTML = shown(instance.tokens, props2.view).map((t) => tokenItem(t, grid2, { view: props2.view, selected: t.id === selected, character: characters[t.char] })).join("");
    }
    function paintInspector() {
      const box = part("token-inspector");
      if (!box) return;
      const t = dm() ? instance?.tokens.find((x) => x.id === selected) : void 0;
      if (!t || !instance) {
        box.hidden = true;
        box.innerHTML = "";
        return;
      }
      if (!box.hidden && host2.ownerDocument.activeElement?.id === "inspector-name" && box.dataset.token === t.id) return;
      box.hidden = false;
      box.dataset.token = t.id;
      box.innerHTML = inspector(t, instance.tokens.filter((x) => x.char === t.char), characters[t.char]);
    }
    function paintRoster() {
      const box = part("battlemap-roster");
      if (!box) return;
      box.hidden = props2.view !== "prep" || !instance;
      if (box.hidden || !instance) return;
      box.innerHTML = roster(instance.roster.map((id) => characters[id]).filter((c) => c !== void 0));
    }
    function paintGrid() {
      if (!map) return;
      part("battlemap-grid")?.setAttribute("d", gridPath(map.grid, map.width, map.height));
      const path = part("battlemap-grid");
      if (path) path.style.opacity = String(map.grid.opacity);
    }
    function paintMap() {
      const box = part("battlemap-map");
      if (!box) return;
      if (!map) {
        box.innerHTML = "";
        observer?.disconnect();
        observer = null;
        return;
      }
      box.innerHTML = hexMap(map, canMove());
      paintGrid();
      fitted = false;
      frame = { zoom: 1, panX: 0, panY: 0 };
      applyFrame();
      observer?.disconnect();
      observer = null;
      if (!fit() && typeof ResizeObserver !== "undefined") {
        const view = part("battlemap-view");
        if (view) {
          observer = new ResizeObserver(() => {
            if (!fitted) fit();
          });
          observer.observe(view);
        }
      }
    }
    function paintAll() {
      paintToolbar();
      paintTokens();
      paintInspector();
      paintRoster();
    }
    async function loadMap(id) {
      return parseMap((await shell2.capability(GET_BATTLEMAP, void 0, { id })).data);
    }
    async function loadCharacters() {
      if (!dm()) return {};
      try {
        return parseCharacters((await shell2.capability(LIST_CHARACTERS, void 0, props2.campaign === "" ? {} : { campaign: props2.campaign })).data);
      } catch {
        return {};
      }
    }
    async function refresh() {
      const mine = generation;
      const no = readNo += 1;
      try {
        const read = await shell2.capability(GET_BATTLEMAP_INSTANCE, void 0, { instance: props2.instance, view: dm() ? "dm" : "player" });
        if (mine !== generation || no < readNo) return true;
        const next = parseInstance(read.data);
        if (next === null) {
          say("error", "Cette battlemap est introuvable.");
          return false;
        }
        next.id ||= props2.instance;
        const changed = next.battlemap !== instance?.battlemap || map === null;
        const loaded = changed ? await loadMap(next.battlemap) : map;
        if (mine !== generation || no < readNo) return true;
        instance = next;
        if (loaded === null) say("error", "La carte de cette instance est introuvable.");
        if (loaded !== map) {
          map = loaded;
          paintMap();
        }
        if (selected !== null && !next.tokens.some((t) => t.id === selected)) selected = null;
        paintAll();
        return true;
      } catch {
        say("error", "Impossible de lire la battlemap.");
        return false;
      }
    }
    async function start() {
      generation += 1;
      const mine = generation;
      stopWatching();
      instance = null;
      map = null;
      selected = null;
      drag = null;
      pan = null;
      stale = false;
      observer?.disconnect();
      observer = null;
      host2.innerHTML = skeleton(props2);
      if (props2.instance === "") return;
      const listed = await loadCharacters();
      if (mine !== generation) return;
      characters = listed;
      await refresh();
      if (mine !== generation) return;
      let first = true;
      stopWatching = shell2.onDataVersion(() => {
        if (first) first = false;
        else void refresh();
      }, timing.watchMs);
    }
    async function send(body) {
      const answer = await shell2.command(body);
      const id = commandId(answer);
      let state = settled(answer);
      for (let asked = 0; !isFinal(state.status) && id !== "" && asked < timing.polls; asked += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    async function act(failure, dataCapability, payload, apply) {
      if (!instance) return;
      readNo += 1;
      apply?.();
      paintAll();
      try {
        const state = await send(tableCommand(dataCapability, instance.id, payload));
        if (state.status === "rejected") say("error", `${failure} refus\xE9 : ${reasons(state.violations)}.`);
        else if (state.status !== "applied") say("error", `${failure} n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`);
        else say("none");
      } catch {
        say("error", `${failure} a \xE9chou\xE9.`);
      }
      await refresh();
    }
    const moveTo = (t, hex) => act("Le d\xE9placement", MOVE_TOKEN, { token: t.id, q: hex.q, r: hex.r }, () => {
      t.q = hex.q;
      t.r = hex.r;
    });
    async function place(character, hex, count) {
      if (!instance) return;
      const there = existingToken(instance.tokens, character);
      if (there) return moveTo(there, hex);
      return act("Le placement", PLACE_TOKENS, { character: character.id, q: hex.q, r: hex.r, count: character.kind === "pnj" ? count : 1 });
    }
    function askPlace(character, hex, at) {
      const view = part("battlemap-view");
      if (!view || !instance) return;
      part("battlemap-drop")?.remove();
      const pop = host2.ownerDocument.createElement("div");
      pop.className = "bm-drop";
      pop.id = "battlemap-drop";
      pop.dataset.noPan = "";
      pop.dataset.char = character.id;
      pop.style.left = `${at.lx}px`;
      pop.style.top = `${at.ly}px`;
      pop.innerHTML = dropForm(character, instance.tokens, 1);
      view.append(pop);
      pop.addEventListener("input", (event) => {
        const field = event.target;
        const note = pop.querySelector("#battlemap-drop-numbers");
        if (field?.id === "battlemap-drop-count" && note && instance) note.textContent = numbering(instance.tokens, character, countOf(field.value ?? ""));
      });
      pop.addEventListener("click", (event) => {
        const clicked = event.target?.closest("button");
        if (!clicked) return;
        const count = countOf(pop.querySelector("#battlemap-drop-count")?.value ?? "1");
        pop.remove();
        if (clicked.id === "battlemap-drop-ok") void place(character, hex, count);
      });
      pop.querySelector("#battlemap-drop-count")?.focus();
    }
    const showOnPlayerView = async () => {
      if (!instance) return;
      try {
        const body = commandBody(SHOW_ON_PLAYER_VIEW, "Diffusion", instance.session === "" ? void 0 : instance.session, { kind: "battlemap", instance: instance.id, zoom: Number(frame.zoom.toFixed(3)), panX: Math.round(frame.panX), panY: Math.round(frame.panY) });
        const state = await send(body);
        if (state.status === "rejected") say("error", `L\u2019envoi aux joueurs est refus\xE9 : ${reasons(state.violations)}.`);
        else if (state.status !== "applied") say("error", `L\u2019envoi aux joueurs n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`);
        else say("ok", "Carte envoy\xE9e aux joueurs.");
      } catch {
        say("error", "L\u2019envoi aux joueurs a \xE9chou\xE9.");
      }
    };
    const tokenOf = (el) => {
      const id = el?.closest(".kit-token__item")?.dataset.token;
      return instance?.tokens.find((t) => t.id === id);
    };
    function drawPath(from, to) {
      const over = part("battlemap-over");
      if (over && map) over.innerHTML = pathMarkup(map.grid, line(from, to));
    }
    function clearCarry() {
      const over = part("battlemap-over");
      if (over) over.innerHTML = "";
      const tip = part("battlemap-tip");
      if (tip) tip.hidden = true;
    }
    host2.addEventListener("pointerdown", (event) => {
      const e = event;
      if (e.button !== 0 || !canMove() || !map) return;
      const target = e.target;
      const el = target.closest(".kit-token__item");
      if (el && part("battlemap-tokens")?.contains(el)) {
        const t = tokenOf(el);
        if (!t) return;
        e.preventDefault();
        drag = { token: t, el, from: { q: t.q, r: t.r }, x: e.clientX, y: e.clientY, moved: false, to: null };
        el.setPointerCapture?.(e.pointerId);
      } else if (target.closest(".hm-view") && !target.closest("[data-no-pan]")) {
        pan = { x: e.clientX, y: e.clientY, panX: frame.panX, panY: frame.panY, moved: false };
        part("battlemap-view")?.setPointerCapture?.(e.pointerId);
      }
    });
    host2.addEventListener("pointermove", (event) => {
      const e = event;
      if (drag && map) {
        if (!drag.moved && Math.abs(e.clientX - drag.x) + Math.abs(e.clientY - drag.y) < DRAG_THRESHOLD) return;
        drag.moved = true;
        drag.el.classList.add("kit-token__item--carried");
        const w = world(e);
        drag.el.style.left = `${w.x}px`;
        drag.el.style.top = `${w.y}px`;
        drag.to = toHex(map.grid, w);
        drawPath(drag.from, drag.to);
        const tip = part("battlemap-tip");
        if (tip) {
          tip.textContent = label(distance(drag.from, drag.to));
          tip.style.left = `${w.lx + 16}px`;
          tip.style.top = `${w.ly + 16}px`;
          tip.hidden = false;
        }
      } else if (pan) {
        const dx = e.clientX - pan.x;
        const dy = e.clientY - pan.y;
        if (!pan.moved && Math.abs(dx) + Math.abs(dy) > DRAG_THRESHOLD) pan.moved = true;
        if (pan.moved) {
          frame = { ...frame, panX: pan.panX + dx, panY: pan.panY + dy };
          fitted = true;
          applyFrame();
        }
      }
    });
    const release = () => {
      pan = null;
      if (!drag) return;
      const d = drag;
      drag = null;
      clearCarry();
      if (d.moved && d.to && map && !onMap(map.grid, d.to, map.width, map.height)) {
        say("error", "Ce hex est hors de la carte.");
        paintTokens();
      } else if (d.moved && d.to && (d.to.q !== d.from.q || d.to.r !== d.from.r)) void moveTo(d.token, d.to);
      else if (!d.moved && dm()) {
        selected = d.token.id;
        paintAll();
      } else paintTokens();
      if (stale) paintTokens();
    };
    host2.addEventListener("pointerup", release);
    host2.addEventListener("pointercancel", release);
    host2.addEventListener("wheel", (event) => {
      const e = event;
      if (!canMove() || !e.target.closest(".hm-view")) return;
      e.preventDefault();
      const box = part("battlemap-view")?.getBoundingClientRect();
      zoomAt(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientX - (box?.left ?? 0), e.clientY - (box?.top ?? 0));
    });
    host2.addEventListener("dragstart", (event) => {
      const e = event;
      const chip = e.target.closest("[data-char]");
      if (!chip || !e.dataTransfer) return;
      e.dataTransfer.setData(CHARACTER_TYPE, chip.dataset.char ?? "");
      e.dataTransfer.effectAllowed = "copy";
    });
    host2.addEventListener("dragover", (event) => {
      const e = event;
      if (dm() && e.target.closest(".hm-view") && Array.from(e.dataTransfer?.types ?? []).includes(CHARACTER_TYPE)) {
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
      }
    });
    host2.addEventListener("drop", (event) => {
      const e = event;
      const character = characters[e.dataTransfer?.getData(CHARACTER_TYPE) ?? ""];
      if (!dm() || !character || !map || !e.target.closest(".hm-view")) return;
      e.preventDefault();
      const w = world(e);
      const hex = toHex(map.grid, w);
      if (onMap(map.grid, hex, map.width, map.height)) askPlace(character, hex, w);
      else say("error", "Ce hex est hors de la carte.");
    });
    host2.addEventListener("click", (event) => {
      const button2 = event.target?.closest("button");
      if (!button2) return;
      const t = instance?.tokens.find((x) => x.id === selected);
      const zoomBox = part("battlemap-view");
      const [cx, cy] = [(zoomBox?.clientWidth ?? 0) / 2, (zoomBox?.clientHeight ?? 0) / 2];
      const gesture = button2.dataset.act;
      if (gesture === "in") zoomAt(1.25, cx, cy);
      else if (gesture === "out") zoomAt(1 / 1.25, cx, cy);
      else if (gesture === "reset") fit();
      else if (gesture === "close") {
        selected = null;
        paintAll();
      } else if (button2.dataset.select) {
        selected = button2.dataset.select;
        paintAll();
      } else if (button2.dataset.char && button2.closest("#battlemap-roster")) {
        const character = characters[button2.dataset.char];
        if (character && map) {
          const box = part("battlemap-view");
          const w = world({ clientX: (box?.getBoundingClientRect().left ?? 0) + (box?.clientWidth ?? 0) / 2, clientY: (box?.getBoundingClientRect().top ?? 0) + (box?.clientHeight ?? 0) / 2 });
          const hex = toHex(map.grid, w);
          askPlace(character, onMap(map.grid, hex, map.width, map.height) ? hex : toHex(map.grid, { x: map.width / 2, y: map.height / 2 }), w);
        }
      } else if (button2.dataset.writes === SHOW_ON_PLAYER_VIEW) void showOnPlayerView();
      else if (t && button2.dataset.state) {
        const state = button2.dataset.state;
        void act("Le changement d\u2019\xE9tat", SET_TOKEN_CONDITION, { token: t.id, conditions: withCondition(t.states, state) }, () => {
          t.states = withCondition(t.states, state);
        });
      } else if (t && button2.id === "inspector-visible") {
        void act(t.visible ? "Le fait de cacher" : "Le fait de r\xE9v\xE9ler", SET_TOKEN_VISIBILITY, { token: t.id, visible: !t.visible }, () => {
          t.visible = !t.visible;
        });
      } else if (t && button2.dataset.writes === REMOVE_TOKEN) {
        void act("Le retrait", REMOVE_TOKEN, { token: t.id }, () => {
          if (instance) instance.tokens = instance.tokens.filter((x) => x.id !== t.id);
          selected = null;
        });
      }
    });
    host2.addEventListener("change", (event) => {
      const field = event.target;
      const t = instance?.tokens.find((x) => x.id === selected);
      if (!t || field?.id !== "inspector-name") return;
      const name = field.value.trim();
      if (name === "") {
        field.value = t.name;
        say("error", "Le nom d\u2019un token ne peut pas \xEAtre vide.");
      } else if (name !== t.name) {
        void act("Le renommage", RENAME_TOKEN, { token: t.id, name }, () => {
          t.name = name;
        });
      }
    });
    host2.addEventListener("submit", (event) => event.preventDefault());
    void start();
    return {
      update(next) {
        const parsed = parseProps(next);
        const same2 = parsed.instance === props2.instance && parsed.view === props2.view && parsed.pilot === props2.pilot && parsed.fill === props2.fill && parsed.campaign === props2.campaign;
        props2 = parsed;
        if (!same2) void start();
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
  async function instanceOf(server, id) {
    const data = await server.load("table.getBattlemapInstance");
    if (!data || id && data.id !== id) return void 0;
    const store = window.Mock && window.Mock.store;
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
      const dressedTokens = await dressed(server, data.state.tokens);
      if (view !== "player") return { ...data, state: { ...data.state, tokens: dressedTokens } };
      const tokens = dressedTokens.filter((t) => t.visible).map((t) => ({ id: t.id, char: t.char, kind: t.kind, name: t.name, num: t.num, q: t.q, r: t.r, visible: true, states: t.states, dead: t.dead, initials: t.initials, color: t.color, portrait: t.portrait, healthBand: t.kind === "pnj" ? healthBand(t.hp, t.hpMax) : null }));
      return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat: null } };
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
      const { opacity, ...grid2 } = battlemap.grid;
      return { ...battlemap, grid: { ...grid2, opacity: opacity / 100, feetPerHex: 5 } };
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
  async function onToken(server, target, token2, change) {
    const data = await instanceOf(server, target && target.id);
    const found = data && data.state.tokens.find((t) => t.id === token2);
    if (!found) return ["instance-isolated"];
    const violations = change(found, data.state);
    if (violations) return violations;
    shareInstance(data);
    return void 0;
  }
  var COMMANDS = {
    // table.moveToken (crates/table/data-capabilities/move-token): (q, r) is the
    // one position both views read.
    "table.moveToken": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      t.q = payload.q;
      t.r = payload.r;
    }),
    // table.renameToken: only that token's name; an empty name is refused.
    "table.renameToken": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      if (!String(payload.name || "").trim()) return ["name-required"];
      t.name = String(payload.name).trim();
      return void 0;
    }),
    // table.setTokenVisibility: hidden tokens keep their place; the players do not read them.
    "table.setTokenVisibility": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      t.visible = payload.visible === true;
    }),
    // table.setTokenCondition: the badges of that token alone.
    "table.setTokenCondition": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
      t.states = [...payload.conditions || []];
    }),
    // table.removeToken: the token leaves the map.
    "table.removeToken": (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t, state) => {
      state.tokens = state.tokens.filter((x) => x !== t);
    }),
    // table.placeTokens: one character dropped, once or « × n »; the new tokens
    // are numbered after those already taken for it (multi-drop-numbering), a
    // monster takes its hit points from its sheet, a PC has one token.
    "table.placeTokens": async (server, { payload, targetId }) => {
      const data = await instanceOf(server, targetId);
      const character = (await server.load("campaign.listCharacters") || []).find((c) => c.id === payload.character);
      if (!data || !character) return ["instance-isolated"];
      const { tokens } = data.state;
      const mine = tokens.filter((t) => t.char === character.id);
      const pnj = character.kind === "pnj";
      if (!pnj && (mine.length || !(data.roster || []).includes(character.id))) return ["token-roster"];
      const count = pnj ? Math.max(1, Math.min(12, Number(payload.count) || 1)) : 1;
      const base2 = character.tokenName || String(character.name).split(" ")[0];
      let num2 = Math.max(0, ...mine.map((t) => t.num || 0));
      const numbered = pnj && (count > 1 || mine.length > 0);
      for (const hex of freeHexes({ q: payload.q, r: payload.r }, tokens, count)) {
        num2 += 1;
        tokens.push({
          id: `t-${character.id}${pnj ? `-${num2}` : ""}`,
          char: character.id,
          kind: character.kind,
          name: numbered ? `${base2} ${num2}` : base2,
          num: pnj ? num2 : null,
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
    // a battlemap instance of the session, or an illustration of the campaign,
    // becomes the player view's base, framed as the DM framed it (1 and 0, 0
    // when no framing is sent); the floating window stays as it is. Any other
    // base would leave it incoherent (`base-coherent`).
    "live.showOnPlayerView": async (server, { payload }) => {
      let base2;
      if (payload.kind === "battlemap") {
        const data = await instanceOf(server, payload.instance);
        if (!data) return ["base-coherent"];
        base2 = { kind: "battlemap", id: data.id };
      } else if (payload.kind === "illustration") {
        const listed = [await server.load("media"), await server.load("media.listMedia")];
        if (!listed.some((media) => media && media.illustrations.some((i) => i.id === payload.media))) return ["base-coherent"];
        base2 = { kind: "illustration", id: payload.media };
      } else return ["base-coherent"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.base = base2;
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
    "live.showWindowOnPlayerView": async (server, { payload }) => {
      const id = payload.media ?? payload.id;
      if (!id) return payload.kind && payload.kind !== "illustration" ? ["window-proportional"] : void 0;
      const listed = [await server.load("media"), await server.load("media.listMedia")];
      if (!listed.some((media) => media && media.illustrations.some((i) => i.id === id))) return ["window-proportional"];
      const store = window.Mock && window.Mock.store;
      if (store) {
        const diffusion = store.get("diffusion", { base: { kind: "cover" }, window: null, view: null });
        diffusion.window = { id, x: 0.6, y: 0.12, w: 0.3 };
        store.set("diffusion", diffusion);
      }
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
      const base2 = { id: server.uuid(), title: String(payload.title || "").trim() || "Sans titre", file: payload.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      if (kind === "battlemap") media.battlemaps.push({ ...base2, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 } });
      else media.illustrations.push(base2);
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
    "campaign.restoreEntityVersion": async (server, { payload, targetId }) => {
      const history = (await server.load("campaign.listEntityVersions"))[targetId];
      const target = history && history.versions.find((v) => v.v === payload.version);
      if (!target) return ["not-found"];
      const sheet = (upTo) => history.versions.slice().sort((a, b) => a.v - b.v).filter((v) => v.v <= upTo).reduce((fields, v) => {
        for (const c of v.changes) fields[c.field] = c.new;
        return fields;
      }, {});
      const before = sheet(history.versions[0].v);
      let after = sheet(target.v);
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
      const { proposals } = await server.load("NoteImport");
      server.noteText = text2;
      server.noteProposals = proposals.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
      return void 0;
    },
    // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
    // one element per accepted proposal, the raw note kept as an import listed
    // newest first, the proposals no longer awaiting.
    "campaign.createEntitiesFromNote": async (server, { payload }) => {
      const proposals = server.noteProposals || [];
      const accepted = Array.isArray(payload.accepted) ? payload.accepted : [];
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
    "campaign.saveEntity": async (server, { payload }) => {
      const given = payload.entity || payload;
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

  // mock/src/Battlemap.js
  var { host, props } = mountPoint();
  var base = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var shell = {
    ...base,
    onDataVersion(listener) {
      const server = mockServer();
      const source = server.eventSource();
      let watching = true;
      source.addEventListener("dataVersion", (message) => {
        if (watching) listener(JSON.parse(message.data).dataVersion);
      });
      listener(server.dataVersion);
      return () => {
        watching = false;
        source.close();
      };
    }
  };
  compose(props, (resolved) => mount(host, resolved, shell));
})();
