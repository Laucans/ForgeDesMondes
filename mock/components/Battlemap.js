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

  // apps/table/battlemap/src/ids.ts
  var GET_BATTLEMAP_INSTANCE = "table.getBattlemapInstance";
  var GET_BATTLEMAP = "media.getBattlemap";
  var LIST_CHARACTERS = "campaign.listCharacters";
  var GET_DIFFUSION = "live.getDiffusion";
  var MOVE_TOKEN = "table.moveToken";
  var PLACE_TOKENS = "table.placeTokens";
  var REMOVE_TOKEN = "table.removeToken";
  var RENAME_TOKEN = "table.renameToken";
  var SET_TOKEN_VISIBILITY = "table.setTokenVisibility";
  var SET_TOKEN_CONDITION = "table.setTokenCondition";
  var CHANGE_HIT_POINTS = "table.changeHitPoints";
  var START_COMBAT = "table.startCombat";
  var NEXT_TURN = "table.nextTurn";
  var REORDER_INITIATIVE = "table.reorderInitiative";
  var ADD_PARTICIPANT = "table.addParticipant";
  var REMOVE_PARTICIPANT = "table.removeParticipant";
  var END_COMBAT = "table.endCombat";
  var RESET_BATTLEMAP = "table.resetBattlemap";
  var SHOW_ON_PLAYER_VIEW = "live.showOnPlayerView";
  var AGGREGATE = "BattlemapInstance";
  var DATA_CAPABILITY_VERSION = 1;
  var APPROVER = "role:approver";

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
  var numbers = (value) => isRecord(value) ? Object.fromEntries(Object.entries(value).filter((e) => typeof e[1] === "number" && Number.isFinite(e[1]))) : {};
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
  function parseProps(raw, search = "") {
    const view = raw.view === void 0 || raw.view === "dm" ? "dm" : raw.view === "prep" ? "prep" : "player";
    return { instance: text(raw.instance), view, pilot: raw.pilot === true || new URLSearchParams(search).get("pilote") === "1", fill: raw.fill === true, campaign: text(raw.campaign) };
  }
  function token(value) {
    const name = text(value.name);
    const kind = value.kind === "pnj" ? "pnj" : "pj";
    const hp = orNull(value.hp);
    return {
      id: text(value.id),
      char: text(value.char),
      kind,
      name,
      num: orNull(value.num),
      q: num(value.q, 0),
      r: num(value.r, 0),
      visible: value.visible !== false,
      hp,
      hpMax: orNull(value.hpMax),
      healthBand: text(value.healthBand ?? value.health_band ?? value.band) || null,
      states: texts(value.states),
      // dead-derived: an NPC is dead iff its current HP is 0, whatever else the answer says.
      dead: kind === "pnj" && hp !== null ? hp <= 0 : value.dead === true,
      initials: text(value.initials) || name.slice(0, 2).toUpperCase(),
      color: text(value.color),
      portrait: value.portrait === true
    };
  }
  function participant(value) {
    return { id: text(value.id), init: num(value.initiative, num(value.init, 0)), dex: num(value.dex, 0) };
  }
  function parseCombat(value) {
    if (!isRecord(value) || !Array.isArray(value.order)) return null;
    const order = value.order.filter(isRecord).map(participant).filter((p) => p.id !== "");
    return order.length === 0 ? null : { round: Math.max(1, num(value.round, 1)), current: typeof value.current === "string" ? value.current : order[0]?.id ?? "", order };
  }
  function parseInstance(data) {
    if (!isRecord(data)) return null;
    const state = isRecord(data.state) ? data.state : data;
    const tokens = Array.isArray(state.tokens) ? state.tokens.filter(isRecord).map(token) : [];
    return { id: text(data.id), battlemap: text(data.battlemap), session: text(data.session), label: text(data.label), roster: texts(data.roster), tokens, combat: parseCombat(state.combat), pjInitiative: numbers(data.pjInitiative) };
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
  var modifier = (score) => typeof score === "number" && Number.isFinite(score) ? Math.floor((score - 10) / 2) : 0;
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
        portrait: entry.portrait === true,
        dexMod: modifier(isRecord(entry.stats) ? entry.stats.DEX : void 0)
      };
    }
    return out;
  }
  var shown = (tokens, view) => view === "player" ? tokens.filter((t) => t.visible) : [...tokens];
  function visibleOrder(combat, tokens, view) {
    return combat.order.flatMap((participant2) => {
      const found = tokens.find((t) => t.id === participant2.id);
      return found && (view !== "player" || found.visible) ? [{ participant: participant2, token: found }] : [];
    });
  }
  function turnAfter(combat, tokens) {
    const { order } = combat;
    const from = Math.max(0, order.findIndex((p) => p.id === combat.current));
    for (let step = 1; step <= order.length; step += 1) {
      const at = (from + step) % order.length;
      const next = order[at];
      if (next && !tokens.find((t) => t.id === next.id)?.dead) return { current: next.id, round: at <= from ? combat.round + 1 : combat.round };
    }
    return { current: combat.current, round: combat.round };
  }
  var hitPointsAfter = (hp, hpMax, delta) => Math.max(0, Math.min(hpMax, hp + delta));
  function reordered(order, id, position) {
    const from = order.findIndex((p) => p.id === id);
    const moved = order[from];
    if (!moved) return [...order];
    const rest = order.filter((p) => p.id !== id);
    rest.splice(Math.max(0, Math.min(rest.length, position)), 0, moved);
    return rest;
  }
  function inserted(order, added) {
    const at = order.findIndex((p) => p.init < added.init || p.init === added.init && p.dex < added.dex);
    return [...order.slice(0, at < 0 ? order.length : at), added, ...at < 0 ? [] : order.slice(at)];
  }
  function without(combat, tokens, id) {
    const order = combat.order.filter((p) => p.id !== id);
    if (order.length === 0) return null;
    if (combat.current !== id) return { ...combat, order };
    const next = turnAfter(combat, tokens.map((t) => t.id === id ? { ...t, dead: true } : t));
    return next.current === id ? { ...combat, current: order[0]?.id ?? "", order } : { round: next.round, current: next.current, order };
  }
  var initiativeOf = (roll, dexMod) => roll + dexMod;
  function bandOf(hp, hpMax) {
    const current = Math.max(0, Math.min(hpMax, hp));
    return current === 0 ? "mort" : current === hpMax ? "indemne" : 2 * current > hpMax ? "bless\xE9" : "en sang";
  }
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
    return { dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`, target: id === void 0 ? { aggregate } : { aggregate, id }, payload, caller: { by: "dm", from: "Battlemap", roles: [APPROVER] } };
  }
  var tableCommand = (dataCapability, instance, payload) => commandBody(dataCapability, AGGREGATE, instance, payload);
  var VIOLATIONS = {
    "token-roster": "un PJ absent de la s\xE9ance, ou d\xE9j\xE0 sur la carte, ne peut pas \xEAtre plac\xE9",
    "instance-isolated": "ce token n\u2019est pas sur cette carte",
    "base-coherent": "cette carte n\u2019est pas celle de la s\xE9ance",
    "pj-hp-not-tracked": "les PV d\u2019un PJ ne sont pas suivis",
    "one-active-combat": "il n\u2019y a pas de combat en cours, ou il y en a d\xE9j\xE0 un",
    "npc-initiative-bounds": "l\u2019initiative saisie ou tir\xE9e est hors limites",
    "name-required": "le nom ne peut pas \xEAtre vide",
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
  var roundFrame = (frame) => ({ zoom: Number(frame.zoom.toFixed(3)), panX: Math.round(frame.panX), panY: Math.round(frame.panY) });
  function parseShownFrame(data, instance) {
    if (!isRecord(data) || !isRecord(data.base) || data.base.kind !== "battlemap" || data.base.id !== instance || !isRecord(data.view)) return null;
    const { zoom, panX, panY } = data.view;
    const finite = (value) => typeof value === "number" && Number.isFinite(value);
    return finite(zoom) && zoom > 0 && finite(panX) && finite(panY) ? { zoom, panX, panY } : null;
  }

  // apps/table/battlemap/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var attr = (name, value) => value === "" ? "" : ` ${name}="${esc(value)}"`;
  var tone = (color) => /^[a-z][a-z-]*$/.test(color) ? color : "secondary";
  function skeleton(props2) {
    const modes = `${props2.fill ? " bm--fill" : ""}${props2.view === "player" && !props2.pilot ? " bm--readonly" : ""}${props2.view === "prep" ? " bm--prep" : ""}`;
    return `<div class="bm${modes}" id="battlemap" data-reads="${GET_BATTLEMAP_INSTANCE}"><div class="bm-toolbar" id="battlemap-toolbar"></div><div class="bm-body" id="battlemap-body"><aside class="bm-side" id="battlemap-roster" aria-labelledby="battlemap-roster-title" data-reads="${LIST_CHARACTERS}"${props2.view === "prep" ? "" : " hidden"}></aside><div class="bm-map" id="battlemap-map" data-reads="${GET_BATTLEMAP} ${GET_DIFFUSION}"></div><aside class="bm-side" id="token-inspector" aria-labelledby="inspector-title" aria-live="polite" hidden></aside></div><div class="bm-carousel" id="initiative-carousel" aria-label="Ordre d\u2019initiative" hidden></div><p class="bm-status" id="battlemap-status" role="status" aria-live="polite" hidden></p><div id="battlemap-dialog"></div></div>`;
  }
  function button(p) {
    return `<button type="button" class="${p.variant}" id="${p.id}"${attr("data-writes", p.writes ?? "")}${p.disabled ? " disabled" : ""}>${esc(p.label)}</button>`;
  }
  function toolbar(map, instance, props2) {
    if (props2.view === "player") return "";
    const hidden = instance.tokens.filter((t) => !t.visible).length;
    const count = `${instance.tokens.length} token${instance.tokens.length > 1 ? "s" : ""}${hidden > 0 ? ` \xB7 ${hidden} cach\xE9${hidden > 1 ? "s" : ""}` : ""}`;
    return `<div class="bm-title"><h2 id="battlemap-title">${esc(map.title)}</h2><span class="small muted">${esc(instance.label)} \xB7 ${count} \xB7 ${SCALE}</span></div>` + group(
      (props2.view === "dm" ? instance.combat ? button({ id: "bm-end-combat", label: "Terminer le combat", variant: "secondary", writes: END_COMBAT }) : button({ id: "bm-start-combat", label: "Lancer le combat", variant: "primary", writes: START_COMBAT }) : "") + button({ id: "bm-send", label: "Envoyer aux joueurs", variant: "secondary", writes: SHOW_ON_PLAYER_VIEW }) + button({ id: "bm-reset", label: "R\xE9initialiser la carte", variant: "ghost", writes: RESET_BATTLEMAP })
    );
  }
  function shape(tag, attrs) {
    return `<${tag}${Object.entries(attrs).map(([k, v]) => attr(k, v)).join("")}></${tag}>`;
  }
  function hexMap(map, canMove) {
    const box = `viewBox="0 0 ${map.width} ${map.height}" width="${map.width}" height="${map.height}"`;
    const land = map.terrain.length === 0 ? `<rect class="hm-land" x="0" y="0" width="${map.width}" height="${map.height}"></rect>` : "";
    return '<div class="kit-hexmap">' + (canMove ? '<div class="hm-bar"><span class="hm-zoom"><button type="button" data-act="out" aria-label="R\xE9duire le zoom">\u2212</button><button type="button" data-act="reset">Ajuster</button><button type="button" data-act="in" aria-label="Zoomer">+</button></span></div>' : "") + `<div class="hm-view" tabindex="0" id="battlemap-view" aria-label="${esc(`Battlemap ${map.title}`)}"><div class="hm-stage" id="battlemap-stage" style="width:${map.width}px;height:${map.height}px"><svg class="hm-layer hm-map" ${box} aria-hidden="true">${land}${map.terrain.map((s) => shape(s.tag, s.attrs)).join("")}</svg><svg class="hm-layer hm-grid" ${box} aria-hidden="true"><path class="hm-grid-path" id="battlemap-grid" d=""></path></svg><svg class="hm-layer hm-over" id="battlemap-over" ${box}></svg><div class="kit-token bm-tokens" id="battlemap-tokens"${attr("data-writes", canMove ? MOVE_TOKEN : "")}></div></div><div class="hm-tip" id="battlemap-tip" hidden></div></div></div>`;
  }
  function tokenItem(t, grid2, p) {
    const c = center(grid2, t);
    return figure(t, p, { id: `token-${esc(t.id)}`, style: ` style="left:${c.x.toFixed(1)}px;top:${c.y.toFixed(1)}px"`, cls: [], extra: "", attrs: "" });
  }
  function figure(t, p, place) {
    const player = p.view === "player";
    const initials = p.character?.initials ?? t.initials;
    const color = p.character?.color || t.color;
    const cls = ["kit-token__item", ...place.cls, !t.visible && "kit-token__item--hidden", t.dead && "kit-token__item--dead", p.selected && "bm-selected"].filter(Boolean).join(" ");
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
    return `<div class="${cls}" id="${place.id}" data-token="${esc(t.id)}"${place.style}${place.attrs} title="${esc(t.name + (t.visible ? "" : " \u2014 cach\xE9 aux joueurs"))}"><div class="kit-token__disc" style="--kit-token-bg: var(--color-${tone(color)})"><div class="kit-token__clip"><span class="kit-token__initials">${esc(initials)}</span></div>${t.visible ? "" : eye}${t.dead ? '<span class="kit-token__strike"></span>' : ""}${badges}${t.num === null ? "" : `<span class="kit-token__num">${t.num}</span>`}</div><div class="kit-token__name">${esc(t.name)}</div>${health}${place.extra}</div>`;
  }
  var group = (inner) => `<div class="kit-button kit-button--bare kit-button--small">${inner}</div>`;
  function carousel(combat, tokens, p) {
    const dm = p.view !== "player";
    const items = visibleOrder(combat, tokens, p.view).map(
      ({ participant: participant2, token: token2 }) => figure(token2, { view: p.view, selected: token2.id === p.selected, character: p.characters[token2.char] }, {
        id: `carousel-${esc(token2.id)}`,
        style: "",
        cls: token2.id === combat.current ? ["kit-token__item--current"] : [],
        extra: `<span class="bm-init" title="Initiative">${participant2.init}</span>`,
        attrs: dm ? ` draggable="true" data-participant="${esc(token2.id)}"` : ""
      })
    ).join("");
    const listed = new Set(combat.order.map((o) => o.id));
    const outside = tokens.filter((t) => !listed.has(t.id) && !t.dead);
    const chosen = p.selected !== null && listed.has(p.selected);
    const controls = dm ? '<div class="bm-carousel__controls" id="carousel-controls">' + group(
      button({ id: "combat-next", label: "Suivant \u25B8", variant: "primary", writes: NEXT_TURN }) + button({ id: "combat-earlier", label: "\u25C2 Avancer", variant: "ghost", writes: REORDER_INITIATIVE, disabled: !chosen }) + button({ id: "combat-later", label: "Reculer \u25B8", variant: "ghost", writes: REORDER_INITIATIVE, disabled: !chosen }) + button({ id: "combat-drop", label: "Retirer du combat", variant: "ghost", writes: REMOVE_PARTICIPANT, disabled: !chosen })
    ) + (outside.length > 0 ? `<span class="row"><select class="num-input bm-add" id="combat-add-select" aria-label="Participant \xE0 ajouter">${outside.map((t) => `<option value="${esc(t.id)}">${esc(t.name)}</option>`).join("")}</select><input type="number" class="num-input" id="combat-add-init" min="1" max="40" aria-label="Initiative du PJ ajout\xE9" value="${p.pjInitiative[outside[0]?.char ?? ""] ?? 10}">${group(button({ id: "combat-add", label: "Ajouter", variant: "secondary", writes: ADD_PARTICIPANT }))}</span>` : "") + "</div>" : "";
    return `<div class="bm-carousel__round" id="combat-round"><span class="small muted">Round</span><b>${combat.round}</b></div><div class="bm-carousel__track" id="carousel-track"><div class="kit-token kit-token--row">${items}</div></div>${controls}`;
  }
  function dialog(p) {
    return `<div class="bm-modal" id="${p.id}"><div class="kit-dialog"><div class="kit-dialog__veil"><div class="kit-dialog__window" role="dialog" aria-modal="true" aria-label="${esc(p.title)}"><header class="kit-dialog__header"><h2 class="kit-dialog__title">${esc(p.title)}</h2><button class="kit-dialog__close" type="button" data-act="dialog-close" aria-label="Fermer">\xD7</button></header><div class="kit-dialog__body">${p.text ? `<p class="kit-dialog__text">${esc(p.text)}</p>` : ""}${p.body ?? ""}</div><footer class="kit-dialog__footer">${p.actions.map((a) => `<button type="button" class="kit-dialog__btn kit-dialog__btn--${a.variant}" id="${a.id}"${attr("data-writes", a.writes ?? "")}>${esc(a.label)}</button>`).join("")}</footer></div></div></div></div>`;
  }
  var signed = (n) => n < 0 ? `\u2212 ${Math.abs(n)}` : `+ ${n}`;
  function startDialog(starters) {
    const rows = starters.map(
      (s) => `<li class="kit-dialog__row"><span>${esc(s.token.name)}${s.token.visible ? "" : ' <em class="muted">(cach\xE9)</em>'}</span>` + (s.token.kind === "pnj" ? `<strong>d20 (${s.roll}) ${signed(s.dexMod)} = ${initiativeOf(s.roll, s.dexMod)}</strong>` : `<input type="number" class="num-input" data-init="${esc(s.token.id)}" min="1" max="40" value="${s.entered ?? ""}" aria-label="${esc(`Initiative de ${s.token.name}`)}">`) + "</li>"
    ).join("");
    return dialog({
      id: "combat-start-dialog",
      title: "Lancer le combat",
      body: `<p class="kit-dialog__text">Les PNJ tirent d20 + modificateur de DEX ; les \xE9galit\xE9s se d\xE9partagent par la DEX, puis au hasard. Saisissez l\u2019initiative des PJ.</p><ul class="kit-dialog__list" id="combat-setup">${rows}</ul><p class="bm-status is-error" id="combat-start-error" role="alert" hidden></p>`,
      actions: [
        { id: "combat-start-cancel", label: "Annuler", variant: "ghost" },
        { id: "combat-start-submit", label: "Commencer le combat", variant: "primary", writes: START_COMBAT }
      ]
    });
  }
  var resetDialog = () => dialog({
    id: "battlemap-reset-dialog",
    title: "R\xE9initialiser la carte",
    text: "Les tokens, le combat et l\u2019initiative de cette instance sont effac\xE9s ; la carte repart comme \xE0 son ouverture.",
    actions: [
      { id: "battlemap-reset-cancel", label: "Annuler", variant: "ghost" },
      { id: "battlemap-reset-submit", label: "R\xE9initialiser", variant: "primary", writes: RESET_BATTLEMAP }
    ]
  });
  function pathMarkup(grid2, hexes) {
    return hexes.map((h, i) => `<polygon class="${i === hexes.length - 1 ? "hm-end" : "hm-step"}" points="${polygon(grid2, h)}"></polygon>`).join("") + `<polyline class="hm-line" points="${trail(grid2, hexes)}"></polyline>`;
  }
  function inspector(t, siblings, character, initiative = null) {
    const chips = siblings.length > 1 ? `<div class="row" id="inspector-instances" role="group" aria-label="Instances de ${esc(character?.name ?? t.name)}">${siblings.map((x) => `<button type="button" class="chip${x.id === t.id ? " is-on" : ""}" data-select="${esc(x.id)}">${esc(x.name)}${x.dead ? " \u2020" : ""}</button>`).join("")}</div>` : "";
    const conditions = Object.entries(CONDITIONS).map(([name, glyph]) => `<button type="button" class="chip${t.states.includes(name) ? " is-on" : ""}" data-state="${esc(name)}" aria-pressed="${t.states.includes(name)}" data-writes="${SET_TOKEN_CONDITION}">${glyph} ${esc(name)}</button>`).join("");
    return `<div class="panel__head"><h3 id="inspector-title">${esc(t.name)}</h3><button type="button" class="icon-btn" data-act="close" aria-label="Fermer le menu du token">\xD7</button></div><p class="small muted">${esc(character?.name ?? "")}${initiative === null ? "" : ` \xB7 initiative ${initiative}`}</p>` + chips + hitPoints(t) + `<div class="kit-input is-wide"><label>Nom affich\xE9<input type="text" id="inspector-name" value="${esc(t.name)}" data-writes="${RENAME_TOKEN}"></label></div><div class="kit-toggle kit-toggle--bare"><label><button type="button" role="switch" id="inspector-visible" aria-checked="${t.visible}" data-writes="${SET_TOKEN_VISIBILITY}"><i></i></button><span>${t.visible ? "Visible des joueurs" : "Cach\xE9 aux joueurs"}</span></label></div><div id="inspector-states" class="stack"><h3>\xC9tats</h3><div class="row">${conditions}</div></div>` + (t.dead ? '<p class="bm-dead" id="inspector-dead">Mort \u2014 gris\xE9.</p>' : "") + `<div class="kit-button kit-button--bare kit-button--small">${button({ id: "inspector-remove", label: t.dead ? "Retirer le corps" : "Retirer de la carte", variant: "danger", writes: REMOVE_TOKEN })}</div>`;
  }
  function hitPoints(t) {
    if (t.kind !== "pnj" || t.hp === null || t.hpMax === null) return '<p class="small muted" id="inspector-pj">Les PV des PJ ne sont pas suivis : les joueurs les g\xE8rent sur leur fiche.</p>';
    return `<div class="bm-hp" id="inspector-hp"><div class="bm-hp__value"><b>${t.hp}</b> / ${t.hpMax} PV</div><span class="small muted">Vu par les joueurs : ${esc(bandLabel(bandOf(t.hp, t.hpMax)))}</span><div class="row"><input type="number" class="num-input" id="inspector-hp-amount" min="1" max="999" value="7" aria-label="Points de vie"><div class="kit-button kit-button--bare kit-button--small">${button({ id: "inspector-damage", label: "\u2212 D\xE9g\xE2ts", variant: "danger", writes: CHANGE_HIT_POINTS })}${button({ id: "inspector-heal", label: "+ Soins", variant: "secondary", writes: CHANGE_HIT_POINTS })}</div></div></div>`;
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
.bm-toolbar { order: -2; display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
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
.bm--prep .bm-body { position: relative; }
.bm--prep #token-inspector { position: absolute; top: 0; right: 0; bottom: 0; z-index: 4; box-shadow: var(--shadow-high); }
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
.bm .kit-token.kit-token--row { flex-wrap: nowrap; padding: 0; background: transparent; gap: var(--space-3); }
.bm-carousel { order: -1; display: flex; align-items: center; gap: var(--space-3); padding: var(--space-2) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-secondary); border-radius: var(--radius-large); box-shadow: var(--shadow-high); }
.bm-carousel[hidden] { display: none; }
.bm-carousel__round { display: grid; justify-items: center; }
.bm-carousel__round b { font-family: var(--font-family-heading); font-size: var(--font-size-heading); color: var(--color-accent); line-height: 1; }
.bm-carousel__track { flex: 1; min-width: 0; overflow-x: auto; padding: var(--space-2) var(--space-1); }
.bm-carousel .kit-token__item { cursor: pointer; flex: none; }
.bm-carousel .kit-token__item[draggable="true"] { cursor: grab; }
.bm-carousel .kit-token__item.bm-drag-over .kit-token__disc { outline: calc(var(--border-width) * 3) dashed var(--color-accent); outline-offset: calc(var(--border-width) * 3); }
.bm-carousel__controls { display: flex; flex-wrap: wrap; gap: var(--space-2); align-items: center; justify-content: flex-end; max-width: calc(var(--space-5) * 9); }
.bm-init { font-size: var(--font-size-small); font-weight: var(--font-weight-heading); color: var(--color-accent); }
.bm-add { width: auto; max-width: calc(var(--space-5) * 4); }
.bm-hp { display: grid; gap: var(--space-1); padding: var(--space-2) var(--space-3); background: var(--color-surface); border-radius: var(--radius-medium); }
.bm-hp__value { font-size: var(--font-size-large); }
.bm-hp__value b { font-family: var(--font-family-heading); font-size: var(--font-size-heading); color: var(--color-success); }
.bm-modal { position: fixed; inset: 0; z-index: 100; }
.bm-modal .kit-dialog, .bm-modal .kit-dialog__veil { height: 100%; }
.bm-modal .kit-dialog__window { max-height: 88vh; }
.bm-drop { position: absolute; z-index: 5; display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-primary); border-radius: var(--radius-medium); box-shadow: var(--shadow-high); cursor: default; }
@media (max-width: 960px) { .bm-body { flex-direction: column; } .bm-side { width: auto; } .bm--prep #token-inspector { position: static; } }
`;

  // apps/table/battlemap/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 500, roll: () => 1 + Math.floor(Math.random() * 20) };
  var DRAG_THRESHOLD = 4;
  var CHARACTER_TYPE = "text/fdm-character";
  var PARTICIPANT_TYPE = "text/fdm-participant";
  var DM_ONLY = /* @__PURE__ */ new Set(["bm-start-combat", "bm-end-combat", "bm-reset", "combat-start-submit", "battlemap-reset-submit", "combat-next", "combat-earlier", "combat-later", "combat-drop", "combat-add", "inspector-damage", "inspector-heal"]);
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="Battlemap"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "Battlemap";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, raw, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    const addressSearch = () => host2.ownerDocument.defaultView?.location.search ?? "";
    let props2 = parseProps(raw, addressSearch());
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
    let followed = false;
    let frameTimer;
    const part = (id) => host2.querySelector(`#${id}`);
    const canMove = () => props2.view !== "player" || props2.pilot;
    const dm = () => props2.view !== "player";
    const piloting = () => props2.view === "player" && props2.pilot;
    function say(kind, text2 = "") {
      const box = part("battlemap-status");
      if (!box) return;
      box.hidden = kind === "none";
      box.className = `bm-status${kind === "none" ? "" : ` is-${kind}`}`;
      box.textContent = text2;
    }
    function applyFrame() {
      const stage = part("battlemap-stage");
      if (stage) stage.style.transform = `translate(${frame.panX}px, ${frame.panY}px) scale(${frame.zoom})`;
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
    function fitWhenSized() {
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
    function follow(framed) {
      if (!map || props2.view !== "player" || props2.pilot) return;
      if (framed) {
        frame = framed;
        fitted = true;
        followed = true;
        applyFrame();
      } else if (followed) {
        followed = false;
        fitWhenSized();
      }
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
    function paintCarousel() {
      const box = part("initiative-carousel");
      if (!box) return;
      const combat = instance?.combat ?? null;
      box.hidden = combat === null || !instance;
      if (box.hidden || !combat || !instance) {
        box.innerHTML = "";
        return;
      }
      box.innerHTML = carousel(combat, instance.tokens, { view: props2.view, selected, characters, pjInitiative: instance.pjInitiative });
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
      const typing = host2.ownerDocument.activeElement?.id;
      const same2 = !box.hidden && box.dataset.token === t.id;
      if (same2 && (typing === "inspector-name" || typing === "inspector-hp-amount")) return;
      const amount = same2 ? box.querySelector("#inspector-hp-amount")?.value : void 0;
      box.hidden = false;
      box.dataset.token = t.id;
      const rank = instance.combat?.order.find((o) => o.id === t.id);
      box.innerHTML = inspector(t, instance.tokens.filter((x) => x.char === t.char), characters[t.char], rank?.init ?? null);
      const field = box.querySelector("#inspector-hp-amount");
      if (field && amount !== void 0) field.value = amount;
    }
    function paintRoster() {
      const box = part("battlemap-roster");
      if (!box) return;
      box.hidden = props2.view !== "prep";
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
      followed = false;
      fitWhenSized();
    }
    function paintAll() {
      paintToolbar();
      paintTokens();
      paintCarousel();
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
    async function readFrame() {
      if (dm() || props2.pilot) return void 0;
      try {
        return parseShownFrame((await shell2.capability(GET_DIFFUSION, void 0, { view: "player" })).data, props2.instance);
      } catch {
        return void 0;
      }
    }
    async function refresh() {
      const mine = generation;
      const no = readNo += 1;
      try {
        const [read, framed] = await Promise.all([shell2.capability(GET_BATTLEMAP_INSTANCE, void 0, { instance: props2.instance, view: dm() ? "dm" : "player" }), readFrame()]);
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
        if (framed !== void 0) follow(framed);
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
      clearTimeout(frameTimer);
      followed = false;
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
    const changeHitPoints = (t, delta) => act("Le changement de PV", CHANGE_HIT_POINTS, { token: t.id, delta }, () => {
      if (t.kind !== "pnj" || t.hp === null || t.hpMax === null) return;
      t.hp = hitPointsAfter(t.hp, t.hpMax, delta);
      t.dead = t.hp === 0;
    });
    const nextTurn = () => {
      const current = instance;
      if (!current?.combat) return Promise.resolve();
      const combat = current.combat;
      return act("Le tour suivant", NEXT_TURN, {}, () => Object.assign(combat, turnAfter(combat, current.tokens)));
    };
    const reorder = (id, position) => {
      const combat = instance?.combat;
      if (!combat || position < 0 || position >= combat.order.length || combat.order[position]?.id === id) return Promise.resolve();
      const order = reordered(combat.order, id, position);
      return act("Le r\xE9ordonnancement", REORDER_INITIATIVE, { order: order.map((p) => p.id) }, () => {
        combat.order = order;
      });
    };
    const yieldTurn = async (id) => {
      if (instance?.combat?.current === id) await nextTurn();
    };
    const removeParticipant = async (id) => {
      await yieldTurn(id);
      const current = instance;
      const combat = current?.combat;
      if (!current || !combat) return;
      await act("Le retrait du combat", REMOVE_PARTICIPANT, { participant: id }, () => {
        current.combat = without(combat, current.tokens, id);
      });
    };
    function addParticipant(id, entered) {
      const combat = instance?.combat;
      const added = instance?.tokens.find((t) => t.id === id);
      if (!combat || !added) return Promise.resolve();
      const dexMod = characters[added.char]?.dexMod ?? 0;
      const roll = (timing.roll ?? (() => 10))();
      const init = added.kind === "pnj" ? initiativeOf(roll, dexMod) : entered;
      return act("L\u2019ajout au combat", ADD_PARTICIPANT, added.kind === "pnj" ? { token: id } : { token: id, initiative: entered }, () => {
        combat.order = inserted(combat.order, { id, init, dex: dexMod });
      });
    }
    const endCombat = () => act("La fin du combat", END_COMBAT, {}, () => {
      if (instance) instance.combat = null;
    });
    let starters = [];
    const closeDialog = () => {
      const box = part("battlemap-dialog");
      if (box) box.innerHTML = "";
      starters = [];
    };
    function askStart() {
      const current = instance;
      const box = part("battlemap-dialog");
      if (!current || !box || current.combat) return;
      const roll = timing.roll ?? (() => 10);
      starters = current.tokens.filter((t) => !t.dead).map((token2) => ({ token: token2, roll: token2.kind === "pnj" ? roll() : 0, dexMod: characters[token2.char]?.dexMod ?? 0, entered: token2.kind === "pj" ? current.pjInitiative[token2.char] ?? null : null }));
      box.innerHTML = startDialog(starters);
      box.querySelector("[data-init]")?.focus();
    }
    function startCombat() {
      const box = part("battlemap-dialog");
      const rolled = starters;
      if (!box || rolled.length === 0) return Promise.resolve();
      const blank = Array.from(box.querySelectorAll("[data-init]")).find((f) => !(Math.floor(Number(f.value)) >= 1));
      if (blank) {
        const error = part("combat-start-error");
        if (error) {
          error.hidden = false;
          error.textContent = `Saisissez l\u2019initiative de ${rolled.find((s) => s.token.id === blank.dataset.init)?.token.name ?? "ce PJ"} (1 ou plus).`;
        }
        blank.focus();
        return Promise.resolve();
      }
      const fields = new Map(Array.from(box.querySelectorAll("[data-init]")).map((f) => [f.dataset.init ?? "", Math.floor(Number(f.value))]));
      const initiative = {};
      const rolls = {};
      const lines = rolled.map((s, rank) => {
        const init = s.token.kind === "pnj" ? initiativeOf(s.roll, s.dexMod) : fields.get(s.token.id) ?? 1;
        if (s.token.kind === "pnj") rolls[s.token.id] = s.roll;
        else initiative[s.token.char] = init;
        return { id: s.token.id, init, dex: s.dexMod, rank };
      });
      closeDialog();
      const order = lines.sort((a, b) => b.init - a.init || b.dex - a.dex || a.rank - b.rank).map(({ id, init, dex }) => ({ id, init, dex }));
      return act("Le lancement du combat", START_COMBAT, { initiative, rolls }, () => {
        if (instance && order[0]) instance.combat = { round: 1, current: order[0].id, order };
      });
    }
    function askReset() {
      const box = part("battlemap-dialog");
      if (box) box.innerHTML = resetDialog();
    }
    function resetMap() {
      closeDialog();
      selected = null;
      return act("La r\xE9initialisation", RESET_BATTLEMAP, {});
    }
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
    const showOnPlayerView = async (quiet = false) => {
      if (!instance) return;
      try {
        const body = commandBody(SHOW_ON_PLAYER_VIEW, "Diffusion", instance.session === "" ? void 0 : instance.session, { kind: "battlemap", instance: instance.id, ...roundFrame(frame) });
        const state = await send(body);
        if (state.status === "rejected") say("error", `L\u2019envoi aux joueurs est refus\xE9 : ${reasons(state.violations)}.`);
        else if (state.status !== "applied") say("error", `L\u2019envoi aux joueurs n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).`);
        else if (!quiet) say("ok", "Carte envoy\xE9e aux joueurs.");
      } catch {
        say("error", "L\u2019envoi aux joueurs a \xE9chou\xE9.");
      }
    };
    function frameGestureEnded() {
      if (!piloting()) return;
      clearTimeout(frameTimer);
      frameTimer = setTimeout(() => {
        frame = roundFrame(frame);
        applyFrame();
        void showOnPlayerView(true);
      }, timing.frameMs ?? 250);
    }
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
      if (pan?.moved) frameGestureEnded();
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
      frameGestureEnded();
    });
    host2.addEventListener("dragstart", (event) => {
      const e = event;
      const carried = e.target.closest("[data-participant]");
      if (carried && e.dataTransfer) {
        e.dataTransfer.setData(PARTICIPANT_TYPE, carried.dataset.participant ?? "");
        e.dataTransfer.effectAllowed = "move";
        return;
      }
      const chip = e.target.closest("[data-char]");
      if (!chip || !e.dataTransfer) return;
      e.dataTransfer.setData(CHARACTER_TYPE, chip.dataset.char ?? "");
      e.dataTransfer.effectAllowed = "copy";
    });
    host2.addEventListener("dragover", (event) => {
      const e = event;
      if (dm() && e.target.closest("[data-participant]") && Array.from(e.dataTransfer?.types ?? []).includes(PARTICIPANT_TYPE)) {
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
        const over = e.target.closest("[data-participant]");
        for (const item of host2.querySelectorAll(".bm-drag-over")) if (item !== over) item.classList.remove("bm-drag-over");
        over?.classList.add("bm-drag-over");
        return;
      }
      if (dm() && e.target.closest(".hm-view") && Array.from(e.dataTransfer?.types ?? []).includes(CHARACTER_TYPE)) {
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
      }
    });
    host2.addEventListener("dragend", () => {
      for (const item of host2.querySelectorAll(".bm-drag-over")) item.classList.remove("bm-drag-over");
    });
    host2.addEventListener("drop", (event) => {
      const e = event;
      for (const item of host2.querySelectorAll(".bm-drag-over")) item.classList.remove("bm-drag-over");
      const carried = e.dataTransfer?.getData(PARTICIPANT_TYPE) ?? "";
      const over = e.target.closest("[data-participant]");
      if (dm() && carried !== "" && over) {
        e.preventDefault();
        const position = instance?.combat?.order.findIndex((o) => o.id === over.dataset.participant) ?? -1;
        if (position >= 0) void reorder(carried, position);
        return;
      }
      const character = characters[e.dataTransfer?.getData(CHARACTER_TYPE) ?? ""];
      if (!dm() || !character || !map || !e.target.closest(".hm-view")) return;
      e.preventDefault();
      const w = world(e);
      const hex = toHex(map.grid, w);
      if (onMap(map.grid, hex, map.width, map.height)) askPlace(character, hex, w);
      else say("error", "Ce hex est hors de la carte.");
    });
    host2.addEventListener("click", (event) => {
      const target = event.target;
      const listed = target?.closest("#initiative-carousel .kit-token__item");
      if (listed && dm()) {
        selected = listed.dataset.token ?? null;
        paintAll();
        return;
      }
      const button2 = target?.closest("button");
      if (!button2) return;
      if (!dm() && DM_ONLY.has(button2.id)) return;
      const t = instance?.tokens.find((x) => x.id === selected);
      const zoomBox = part("battlemap-view");
      const [cx, cy] = [(zoomBox?.clientWidth ?? 0) / 2, (zoomBox?.clientHeight ?? 0) / 2];
      const gesture = button2.dataset.act;
      if (gesture === "in" || gesture === "out" || gesture === "reset") {
        if (gesture === "reset") fit();
        else zoomAt(gesture === "in" ? 1.25 : 1 / 1.25, cx, cy);
        frameGestureEnded();
      } else if (gesture === "dialog-close") closeDialog();
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
      } else if (button2.id === "bm-start-combat") askStart();
      else if (button2.id === "combat-start-submit") void startCombat();
      else if (button2.id === "combat-start-cancel" || button2.id === "battlemap-reset-cancel") closeDialog();
      else if (button2.id === "bm-reset") askReset();
      else if (button2.id === "battlemap-reset-submit") void resetMap();
      else if (button2.id === "bm-end-combat") void endCombat();
      else if (button2.id === "combat-next") void nextTurn();
      else if ((button2.id === "combat-earlier" || button2.id === "combat-later") && selected !== null) {
        const at = instance?.combat?.order.findIndex((o) => o.id === selected) ?? -1;
        if (at >= 0 && selected !== null) void reorder(selected, at + (button2.id === "combat-earlier" ? -1 : 1));
      } else if (button2.id === "combat-drop" && selected !== null) void removeParticipant(selected);
      else if (button2.id === "combat-add") {
        const id = part("combat-add-select")?.value ?? "";
        const entered = Math.max(1, Math.floor(Number(part("combat-add-init")?.value)) || 10);
        if (id !== "") void addParticipant(id, entered);
      } else if (t && t.kind === "pnj" && (button2.id === "inspector-damage" || button2.id === "inspector-heal")) {
        const amount = Math.max(0, Math.floor(Number(part("inspector-hp-amount")?.value)) || 0);
        if (amount > 0) void changeHitPoints(t, button2.id === "inspector-damage" ? -amount : amount);
      } else if (button2.dataset.writes === SHOW_ON_PLAYER_VIEW) void showOnPlayerView();
      else if (t && button2.dataset.state) {
        const state = button2.dataset.state;
        void act("Le changement d\u2019\xE9tat", SET_TOKEN_CONDITION, { token: t.id, state, on: !t.states.includes(state) }, () => {
          t.states = withCondition(t.states, state);
        });
      } else if (t && button2.id === "inspector-visible") {
        void act(t.visible ? "Le fait de cacher" : "Le fait de r\xE9v\xE9ler", SET_TOKEN_VISIBILITY, { token: t.id, visible: !t.visible }, () => {
          t.visible = !t.visible;
        });
      } else if (t && button2.dataset.writes === REMOVE_TOKEN) {
        void yieldTurn(t.id).then(
          () => act("Le retrait", REMOVE_TOKEN, { token: t.id }, () => {
            if (instance) instance.tokens = instance.tokens.filter((x) => x.id !== t.id);
            selected = null;
          })
        );
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
    host2.addEventListener("keydown", (event) => {
      if (event.key === "Escape") closeDialog();
    });
    void start();
    return {
      update(next) {
        const parsed = parseProps(next, addressSearch());
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
      if (own && kept) throw new HandlerError(`${id} is defined twice: mock/handlers/${id}.js and ${shown2(LEGACY[kind][0])} \u2014 keep one`);
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
        const at = (base2) => fetch(new URL(base2 + path, location.href)).then((r) => r.ok ? r.json() : void 0).catch(() => void 0);
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
