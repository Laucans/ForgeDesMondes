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
    resolve(raw2) {
      const out = {};
      for (const [key, value] of Object.entries(raw2)) {
        out[key] = typeof value === "string" && value.startsWith(REFERENCE) ? this.get(value.slice(REFERENCE.length)) : value;
      }
      return out;
    }
    /** The context names the raw props follow. */
    follows(raw2) {
      return Object.values(raw2).filter((value) => typeof value === "string" && value.startsWith(REFERENCE)).map((value) => value.slice(REFERENCE.length));
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

  // apps/media/grid-settings/src/ids.ts
  var GET_BATTLEMAP = "media.getBattlemap";
  var SAVE_GRID_SETTINGS = "media.saveGridSettings";
  var DATA_CAPABILITY_VERSION = 1;
  var AGGREGATE = "Media";

  // apps/media/grid-settings/src/hex.ts
  var SQRT3 = Math.sqrt(3);
  var FEET_PER_HEX = 5;
  var finite = (value) => typeof value === "number" && Number.isFinite(value);
  function problems(grid) {
    const out = [];
    if (!finite(grid.size) || grid.size <= 0) out.push("size");
    if (!finite(grid.ox)) out.push("ox");
    if (!finite(grid.oy)) out.push("oy");
    if (grid.orient !== "pointy" && grid.orient !== "flat") out.push("orient");
    if (typeof grid.color !== "string" || grid.color === "") out.push("color");
    if (!finite(grid.opacity) || grid.opacity < 0 || grid.opacity > 1) out.push("opacity");
    return out;
  }
  function feet(hexes) {
    return typeof hexes === "number" && Number.isInteger(hexes) && hexes >= 0 ? hexes * FEET_PER_HEX : null;
  }
  function center(grid, hex) {
    const s = grid.size;
    const x = grid.orient === "pointy" ? s * SQRT3 * (hex.q + hex.r / 2) : s * 1.5 * hex.q;
    const y = grid.orient === "pointy" ? s * 1.5 * hex.r : s * SQRT3 * (hex.r + hex.q / 2);
    return { x: x + grid.ox + s, y: y + grid.oy + s };
  }
  function axial(grid, px, py) {
    const s = grid.size;
    const x = px - (grid.ox + s);
    const y = py - (grid.oy + s);
    return grid.orient === "pointy" ? { q: (SQRT3 / 3 * x - y / 3) / s, r: 2 / 3 * y / s } : { q: 2 / 3 * x / s, r: (-x / 3 + SQRT3 / 3 * y) / s };
  }
  function round(q, r) {
    const y = -q - r;
    let rx = Math.round(q);
    let ry = Math.round(y);
    let rz = Math.round(r);
    const dx = Math.abs(rx - q);
    const dy = Math.abs(ry - y);
    const dz = Math.abs(rz - r);
    if (dx > dy && dx > dz) rx = -ry - rz;
    else if (dy > dz) ry = -rx - rz;
    else rz = -rx - ry;
    return { q: rx + 0, r: rz + 0 };
  }
  function toHex(grid, px, py) {
    const { q, r } = axial(grid, px, py);
    return round(q, r);
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
  function corners(grid, c) {
    const offset = grid.orient === "pointy" ? -30 : 0;
    return [0, 1, 2, 3, 4, 5].map((i) => {
      const angle = (60 * i + offset) * Math.PI / 180;
      return [c.x + grid.size * Math.cos(angle), c.y + grid.size * Math.sin(angle)];
    });
  }
  var pair = (p) => `${p[0].toFixed(1)} ${p[1].toFixed(1)}`;
  var points = (grid, hex) => corners(grid, center(grid, hex)).map((p) => `${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(" ");
  var MAX_HEXES = 4e4;
  function gridPath(grid, width, height) {
    if (!finite(grid.size) || grid.size <= 0 || !finite(grid.ox) || !finite(grid.oy)) return "";
    const s = grid.size;
    if (width * height / (2.598 * s * s) > MAX_HEXES) return "";
    const edges = [axial(grid, -s, -s), axial(grid, width + s, -s), axial(grid, -s, height + s), axial(grid, width + s, height + s)];
    const qs = edges.map((e) => e.q);
    const rs = edges.map((e) => e.r);
    let d = "";
    for (let q = Math.floor(Math.min(...qs)) - 1; q <= Math.ceil(Math.max(...qs)) + 1; q += 1) {
      for (let r = Math.floor(Math.min(...rs)) - 1; r <= Math.ceil(Math.max(...rs)) + 1; r += 1) {
        const c = center(grid, { q, r });
        if (c.x < -s || c.x > width + s || c.y < -s || c.y > height + s) continue;
        d += `M${corners(grid, c).map(pair).join("L")}Z`;
      }
    }
    return d;
  }

  // apps/media/grid-settings/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var COLORS = [
    { value: "text", label: "Ivoire" },
    { value: "accent", label: "Ambre" },
    { value: "primary", label: "Orange" },
    { value: "secondary", label: "Violet" },
    { value: "success", label: "Vert" }
  ];
  var cssColor = (color) => `var(--color-${COLORS.some((c) => c.value === color) ? color : "text"})`;
  var TAGS = /* @__PURE__ */ new Set(["rect", "ellipse", "circle", "path", "polygon", "polyline", "line"]);
  var ATTRIBUTES = /* @__PURE__ */ new Set(["class", "x", "y", "width", "height", "d", "points", "cx", "cy", "r", "rx", "ry", "x1", "y1", "x2", "y2", "transform", "opacity"]);
  function terrain(value) {
    if (!Array.isArray(value)) return [];
    const shapes = [];
    for (const entry of value) {
      if (!isRecord(entry) || typeof entry.tag !== "string" || !TAGS.has(entry.tag)) continue;
      const attrs = {};
      for (const [name, attribute] of Object.entries(entry)) {
        if (name !== "tag" && ATTRIBUTES.has(name) && (typeof attribute === "string" || typeof attribute === "number")) attrs[name] = String(attribute);
      }
      shapes.push({ tag: entry.tag, attrs });
    }
    return shapes;
  }
  function required(value, what) {
    if (value === void 0) throw new Error(`media.getBattlemap: ${what} is not readable`);
    return value;
  }
  var finite2 = (value) => typeof value === "number" && Number.isFinite(value) ? value : void 0;
  function parseBattlemap(data) {
    if (data === null || data === void 0) return null;
    if (!isRecord(data) || typeof data.id !== "string" || data.id === "" || !isRecord(data.grid)) throw new Error("media.getBattlemap: the answer is not a battlemap");
    const grid = data.grid;
    const width = required(finite2(data.width), "width");
    const height = required(finite2(data.height), "height");
    if (width <= 0 || height <= 0) throw new Error("media.getBattlemap: the map has no size");
    const orient = grid.orient === "flat" || grid.orient === "pointy" ? grid.orient : required(void 0, "orient");
    return {
      id: data.id,
      title: text(data.title),
      width,
      height,
      grid: {
        size: required(finite2(grid.size), "size"),
        ox: required(finite2(grid.ox), "ox"),
        oy: required(finite2(grid.oy), "oy"),
        orient,
        color: required(text(grid.color) || void 0, "color"),
        opacity: required(finite2(grid.opacity), "opacity")
      },
      terrain: terrain(data.terrain)
    };
  }
  function initialForm(map) {
    return { ...map.grid, opacity: Math.round(map.grid.opacity * 1e4) / 100 };
  }
  var gridOf = (form) => ({ ...form, opacity: form.opacity / 100 });
  var WORDS = {
    size: "La taille d\u2019un hex doit \xEAtre sup\xE9rieure \xE0 0.",
    ox: "Le d\xE9calage horizontal doit \xEAtre un nombre.",
    oy: "Le d\xE9calage vertical doit \xEAtre un nombre.",
    orient: "L\u2019orientation doit \xEAtre \xAB pointe en haut \xBB ou \xAB c\xF4t\xE9 plat en haut \xBB.",
    color: "La couleur de la grille est obligatoire.",
    opacity: "L\u2019opacit\xE9 doit \xEAtre comprise entre 0 et 100 %."
  };
  function problem(form) {
    return problems(gridOf(form)).map((field) => WORDS[field] ?? field).join(" ");
  }
  function payload(form) {
    const { size, ox, oy, orient, color } = form;
    return { size, ox, oy, orient, color, opacity: form.opacity / 100 };
  }
  function commandBody(battlemap, body) {
    return {
      dataCapability: `${SAVE_GRID_SETTINGS}@${DATA_CAPABILITY_VERSION}`,
      target: { aggregate: AGGREGATE, id: battlemap },
      payload: body,
      caller: { by: "dm", from: "GridSettings", roles: [] }
    };
  }
  var VIOLATIONS = {
    "grid-values-range": "la taille d\u2019un hex doit \xEAtre sup\xE9rieure \xE0 0 et l\u2019opacit\xE9 comprise entre 0 et 100 %",
    "grid-only-for-battlemap": "seule une battlemap a une grille",
    "hex-is-five-feet": "un hex vaut toujours 5 pieds"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result2 = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    const violations = Array.isArray(result2.violations) ? result2.violations.filter((v) => typeof v === "string") : [];
    return { status: text(result2.status), violations };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }

  // apps/media/grid-settings/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  function skeleton() {
    const reads = `data-reads="${GET_BATTLEMAP}"`;
    return `<section class="panel mf-grid" id="grid-settings" aria-labelledby="grid-title"><div class="panel__head"><h2 id="grid-title">R\xE9glage de la grille</h2><span class="small muted" id="grid-scale">1 hex = 5 pieds \xB7 cliquez deux hex de l\u2019aper\xE7u pour v\xE9rifier une distance</span></div><div class="mf-grid__layout"><form class="stack" id="grid-form"><div class="stack" id="grid-fields" ${reads}></div><div id="grid-actions"><div class="kit-button kit-button--bare"><button type="button" class="primary" id="grid-save" data-writes="${SAVE_GRID_SETTINGS}" disabled>Enregistrer le r\xE9glage</button><button type="button" class="ghost" id="grid-reset" disabled>Annuler les changements</button></div></div><div class="mf-grid__result" id="grid-result" aria-live="polite"></div></form><div id="grid-preview" ${reads}></div></div></section>`;
  }
  var notice = (words) => `<p class="muted" id="grid-notice">${esc(words)}</p>`;
  var bounds = (value, min, max) => ({ min: Math.min(min, Math.floor(value)), max: Math.max(max, Math.ceil(value)) });
  function slider(f) {
    const { min, max } = bounds(f.value, f.min, f.max);
    const fill = max === min ? 0 : (f.value - min) / (max - min) * 100;
    const step = f.step ? ` step="${f.step}"` : "";
    return `<div class="kit-slider__field" data-name="${f.name}"><div class="kit-slider__head"><label class="kit-slider__label" for="${f.id}">${esc(f.label)}</label><span class="kit-slider__value"><output class="kit-slider__out" for="${f.id}">${f.value}</output><span class="kit-slider__unit">${f.unit}</span></span></div><input class="kit-slider__input" id="${f.id}" type="range" name="${f.name}" min="${min}" max="${max}" value="${f.value}"${step} style="--fill: ${fill}%"><div class="kit-slider__bounds"><span>${min} ${f.unit}</span><span>${max} ${f.unit}</span></div></div>`;
  }
  function fields(form) {
    const radio = (value, label) => `<label><input type="radio" name="grid-orient" value="${value}"${form.orient === value ? " checked" : ""}><span>${label}</span></label>`;
    const known = COLORS.some((c) => c.value === form.color);
    const colors = known ? COLORS : [...COLORS, { value: form.color, label: form.color }];
    return '<div class="kit-slider kit-slider--bare">' + slider({ id: "grid-size", name: "size", label: "Taille d\u2019un hex", value: form.size, min: 16, max: 60, unit: "px", step: "any" }) + slider({ id: "grid-ox", name: "ox", label: "D\xE9calage horizontal", value: form.ox, min: -30, max: 60, unit: "px", step: "any" }) + slider({ id: "grid-oy", name: "oy", label: "D\xE9calage vertical", value: form.oy, min: -30, max: 60, unit: "px", step: "any" }) + slider({ id: "grid-opacity", name: "opacity", label: "Opacit\xE9", value: form.opacity, min: 5, max: 100, unit: "%", step: "any" }) + `</div><div class="kit-radio"><fieldset id="grid-orient"><legend>Orientation</legend>${radio("pointy", "Pointe en haut")}${radio("flat", "C\xF4t\xE9 plat en haut")}</fieldset></div><div class="kit-select is-wide"><span id="grid-color-label">Couleur de la grille</span><select id="grid-color" name="color" aria-labelledby="grid-color-label">${colors.map((c) => `<option value="${esc(c.value)}"${c.value === form.color ? " selected" : ""}>${esc(c.label)}</option>`).join("")}</select></div>`;
  }
  var title = (map) => map === null ? "R\xE9glage de la grille" : `R\xE9glage de la grille \u2014 ${map.title}`;
  function result(outcome) {
    switch (outcome.kind) {
      case "none":
        return "";
      case "busy":
        return `<p class="muted">${esc(outcome.text)}</p>`;
      case "ok":
      case "error":
        return `<p class="${outcome.kind === "ok" ? "is-ok" : "is-error"}" id="grid-message">${esc(outcome.text)}</p>`;
    }
  }

  // apps/media/grid-settings/src/preview.ts
  var NS = "http://www.w3.org/2000/svg";
  function markup(map) {
    const view = `viewBox="0 0 ${map.width} ${map.height}" width="${map.width}" height="${map.height}"`;
    const shape = (s) => `<${s.tag}${Object.entries(s.attrs).map(([name, value]) => ` ${name}="${esc(value)}"`).join("")}></${s.tag}>`;
    const land = map.terrain.some((s) => s.attrs.class === "hm-land") ? "" : `<rect class="hm-land" x="0" y="0" width="${map.width}" height="${map.height}"></rect>`;
    return `<div class="kit-hexmap"><div class="hm-bar"><span class="hm-zoom"><button type="button" data-act="out" aria-label="D\xE9zoomer">\u2212</button><button type="button" data-act="reset">Ajuster</button><button type="button" data-act="in" aria-label="Zoomer">+</button></span></div><div class="hm-view" tabindex="0" id="grid-preview-map" aria-label="Aper\xE7u de la battlemap avec sa grille"><div class="hm-stage"><svg class="hm-layer hm-map" ${view} aria-hidden="true">${land}${map.terrain.map(shape).join("")}</svg><svg class="hm-layer hm-grid" ${view} aria-hidden="true"><path class="hm-grid-path" d=""></path></svg><svg class="hm-layer hm-over" ${view}></svg></div><div class="hm-tip" hidden></div></div><p class="hm-hint">Glisser pour d\xE9placer \xB7 molette pour zoomer \xB7 cliquer deux hex pour mesurer</p></div>`;
  }
  var MIN_ZOOM = 0.3;
  var MAX_ZOOM = 4;
  function mountPreview(host2, map, grid) {
    host2.innerHTML = markup(map);
    const part = (selector) => {
      const found = host2.querySelector(selector);
      if (!found) throw new Error(`GridSettings: ${selector} is not in the preview`);
      return found;
    };
    const root = part(".kit-hexmap");
    const view = part(".hm-view");
    const stage = part(".hm-stage");
    const path = part(".hm-grid-path");
    const over = part(".hm-over");
    const tip = part(".hm-tip");
    stage.style.width = `${map.width}px`;
    stage.style.height = `${map.height}px`;
    let current = grid;
    let zoom = 1;
    let px = 0;
    let py = 0;
    let fitted = false;
    const applyView = () => {
      stage.style.transform = `translate(${px}px,${py}px) scale(${zoom})`;
    };
    const zoomAt = (factor, cx, cy) => {
      const next = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, zoom * factor));
      const k = next / zoom;
      px = cx - (cx - px) * k;
      py = cy - (cy - py) * k;
      zoom = next;
      applyView();
    };
    const fit = () => {
      const w = view.clientWidth;
      const h = view.clientHeight;
      if (!w || !h) return false;
      zoom = Math.min(w / map.width, h / map.height);
      px = (w - map.width * zoom) / 2;
      py = (h - map.height * zoom) / 2;
      fitted = true;
      applyView();
      return true;
    };
    const svg = (name, attrs, cls) => {
      const element = view.ownerDocument.createElementNS(NS, name);
      element.setAttribute("class", cls);
      for (const [key, value] of Object.entries(attrs)) element.setAttribute(key, value);
      return element;
    };
    const poly = (hex, cls) => svg("polygon", { points: points(current, hex) }, cls);
    const clearOver = () => over.replaceChildren();
    const showTip = (words, lx, ly) => {
      tip.textContent = words;
      tip.style.left = `${lx + 16}px`;
      tip.style.top = `${ly + 16}px`;
      tip.hidden = false;
    };
    const world = (e) => {
      const box = view.getBoundingClientRect();
      const lx = e.clientX - box.left;
      const ly = e.clientY - box.top;
      return { lx, ly, x: (lx - px) / zoom, y: (ly - py) / zoom };
    };
    const drawPath = (a, b) => {
      clearOver();
      const cells = line(a, b);
      cells.forEach((h, i) => over.appendChild(poly(h, i === cells.length - 1 ? "hm-end" : "hm-step")));
      over.appendChild(
        svg(
          "polyline",
          {
            points: cells.map((h) => {
              const c = center(current, h);
              return `${c.x.toFixed(1)},${c.y.toFixed(1)}`;
            }).join(" ")
          },
          "hm-line"
        )
      );
      return distance(a, b);
    };
    const label = (hexes) => `${hexes} hex \xB7 ${feet(hexes)} pi`;
    let start = null;
    let end = null;
    const forget = () => {
      start = null;
      end = null;
      clearOver();
      tip.hidden = true;
    };
    let down = null;
    view.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      down = { x: e.clientX, y: e.clientY, px, py, moved: false };
      view.setPointerCapture?.(e.pointerId);
    });
    view.addEventListener("pointermove", (e) => {
      if (down) {
        const dx = e.clientX - down.x;
        const dy = e.clientY - down.y;
        if (!down.moved && Math.abs(dx) + Math.abs(dy) > 4) {
          down.moved = true;
          view.classList.add("hm-drag");
        }
        if (down.moved) {
          px = down.px + dx;
          py = down.py + dy;
          applyView();
        }
        return;
      }
      if (!start || end) return;
      const w = world(e);
      showTip(label(drawPath(start, toHex(current, w.x, w.y))), w.lx, w.ly);
    });
    const release = () => {
      down = null;
      view.classList.remove("hm-drag");
    };
    view.addEventListener("pointercancel", release);
    view.addEventListener("pointerup", (e) => {
      const click = down !== null && !down.moved;
      down = null;
      view.classList.remove("hm-drag");
      if (!click) return;
      const w = world(e);
      const hex = toHex(current, w.x, w.y);
      if (!start || end) {
        start = hex;
        end = null;
        clearOver();
        over.appendChild(poly(hex, "hm-end"));
        tip.hidden = true;
      } else {
        end = hex;
        showTip(label(drawPath(start, end)), w.lx, w.ly);
      }
    });
    view.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        const box = view.getBoundingClientRect();
        zoomAt(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientX - box.left, e.clientY - box.top);
      },
      { passive: false }
    );
    root.querySelectorAll("[data-act]").forEach(
      (button) => button.addEventListener("click", () => {
        const act = button.dataset.act;
        if (act === "in") zoomAt(1.25, view.clientWidth / 2, view.clientHeight / 2);
        else if (act === "out") zoomAt(1 / 1.25, view.clientWidth / 2, view.clientHeight / 2);
        else fit();
      })
    );
    if (typeof ResizeObserver !== "undefined") {
      new ResizeObserver(() => {
        if (!fitted) fit();
      }).observe(view);
    }
    const draw = (next) => {
      if (problems(next).length > 0) return;
      const d = gridPath(next, map.width, map.height);
      if (d === "" && path.getAttribute("d")) return;
      current = next;
      root.style.setProperty("--hm-grid", cssColor(next.color));
      path.style.opacity = String(next.opacity);
      path.setAttribute("d", d);
      forget();
    };
    draw(grid);
    applyView();
    return { setGrid: draw };
  }

  // apps/media/grid-settings/src/style.ts
  var STYLE = `
.mf-grid { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-grid *, .mf-grid *::before, .mf-grid *::after { box-sizing: inherit; }
.mf-grid.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-grid h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; font-size: var(--font-size-large); color: var(--color-accent); }
.mf-grid p { margin: 0; }
.mf-grid .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-grid .muted { color: var(--color-text-muted); }
.mf-grid .small { font-size: var(--font-size-small); }
.mf-grid .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-grid .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-grid .kit-slider.kit-slider--bare { padding: 0; background: transparent; border: 0; box-shadow: none; max-width: none; }
.mf-grid .kit-select.is-wide { max-width: none; }
.mf-grid .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-grid .kit-select select:hover { border-color: var(--color-primary); }
.mf-grid .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-grid__layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); gap: var(--space-4); align-items: start; }
.mf-grid .kit-hexmap .hm-view { height: auto; min-height: calc(var(--space-5) * 11); }
.mf-grid__result { min-height: var(--space-4); }
.mf-grid__result p { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-grid__result .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-grid__result .is-error { border-color: var(--color-danger); color: var(--color-danger); }
@media (max-width: 960px) { .mf-grid__layout { grid-template-columns: minmax(0, 1fr); } }
`;

  // apps/media/grid-settings/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="GridSettings"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "GridSettings";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  var battlemapOf = (props2) => typeof props2.battlemap === "string" ? props2.battlemap : "";
  function mount(host2, props2, shell2, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`GridSettings: #${id} is not in the panel`);
      return found;
    };
    let map = null;
    let form = null;
    let preview = null;
    let outcome = { kind: "none" };
    let busy = false;
    let asked = 0;
    const paintResult = () => {
      part("grid-result").innerHTML = result(outcome);
    };
    const say = (next) => {
      outcome = next;
      paintResult();
    };
    const lock = (locked) => {
      for (const control of host2.querySelectorAll("#grid-form input, #grid-form select, #grid-form button")) {
        control.disabled = locked || form === null;
      }
    };
    const paintForm = () => {
      part("grid-fields").innerHTML = form === null ? "" : fields(form);
      lock(busy);
    };
    const paintMap = () => {
      part("grid-title").textContent = title(map);
      paintForm();
      if (map !== null && form !== null) preview = mountPreview(part("grid-preview"), map, gridOf(form));
    };
    const show = (words) => {
      map = null;
      form = null;
      preview = null;
      part("grid-title").textContent = title(null);
      paintForm();
      part("grid-preview").innerHTML = notice(words);
    };
    const read = async (id) => parseBattlemap((await shell2.capability(GET_BATTLEMAP, void 0, { id })).data);
    async function load(id) {
      const mine = ++asked;
      say({ kind: "none" });
      if (id === "") return show("Choisissez une battlemap \xE0 r\xE9gler.");
      show("Chargement de la battlemap\u2026");
      try {
        const found = await read(id);
        if (mine !== asked) return;
        if (found === null) return show("Cette battlemap n\u2019existe pas : seule une battlemap a une grille.");
        map = found;
        form = initialForm(found);
        paintMap();
      } catch {
        if (mine === asked) show("Impossible de lire la battlemap.");
      }
    }
    async function send(id, body) {
      const answer = await shell2.command(commandBody(id, payload(body)));
      const command = commandId(answer);
      let state = settled(answer);
      for (let tries = 0; !isFinal(state.status) && command !== "" && tries < timing.polls; tries += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(command));
      }
      return state;
    }
    async function save() {
      if (busy || map === null || form === null) return;
      const why = problem(form);
      if (why) return say({ kind: "error", text: why });
      const id = map.id;
      const sent = form;
      const mine = asked;
      busy = true;
      lock(true);
      say({ kind: "busy", text: "Enregistrement\u2026" });
      try {
        const state = await send(id, sent);
        if (mine !== asked) return;
        if (state.status === "rejected") return say({ kind: "error", text: `Enregistrement refus\xE9 : ${reasons(state.violations)}.` });
        if (state.status !== "applied" && !isFinal(state.status) && state.status !== "") return say({ kind: "busy", text: `Enregistrement en attente (\xE9tat : ${state.status}) : il s\u2019appliquera d\xE8s que la file le permettra.` });
        if (state.status !== "applied") return say({ kind: "error", text: `L\u2019enregistrement n\u2019a pas abouti (\xE9tat : ${state.status || "inconnu"}).` });
        try {
          const saved = await read(id);
          if (mine !== asked) return;
          if (saved !== null && map !== null) map = saved;
        } catch {
          if (mine !== asked) return;
          if (map !== null) map = { ...map, grid: gridOf(sent) };
          return say({ kind: "ok", text: "R\xE9glage enregistr\xE9, mais il n\u2019a pas pu \xEAtre relu." });
        }
        say({ kind: "ok", text: "R\xE9glage enregistr\xE9." });
      } catch {
        if (mine === asked) say({ kind: "error", text: "L\u2019enregistrement a \xE9chou\xE9." });
      } finally {
        busy = false;
        lock(false);
      }
    }
    host2.addEventListener("submit", (event) => event.preventDefault());
    const follow = (change) => {
      if (form === null) return;
      form = { ...form, ...change };
      preview?.setGrid(gridOf(form));
      if (outcome.kind !== "busy") say({ kind: "none" });
    };
    host2.addEventListener("input", (event) => {
      const input = event.target;
      const name = input?.name;
      if (!input || name !== "size" && name !== "ox" && name !== "oy" && name !== "opacity") return;
      const out = input.closest(".kit-slider__field")?.querySelector(".kit-slider__out");
      if (out) out.textContent = input.value;
      const min = Number(input.min);
      const max = Number(input.max);
      input.style.setProperty("--fill", `${max === min ? 0 : (Number(input.value) - min) / (max - min) * 100}%`);
      follow({ [name]: Number(input.value) });
    });
    host2.addEventListener("change", (event) => {
      const target = event.target;
      if (target?.name === "grid-orient") follow({ orient: target.value === "flat" ? "flat" : "pointy" });
      else if (target?.name === "color") follow({ color: target.value ?? "text" });
    });
    host2.addEventListener("click", (event) => {
      const button = event.target?.closest("button");
      if (!button) return;
      if (button.dataset.writes === SAVE_GRID_SETTINGS) void save();
      else if (button.id === "grid-reset" && map !== null && !busy) {
        form = initialForm(map);
        say({ kind: "none" });
        paintForm();
        preview?.setGrid(gridOf(form));
      }
    });
    let shown2 = battlemapOf(props2);
    void load(shown2);
    return {
      update(next) {
        const id = battlemapOf(next);
        if (id === shown2) return;
        shown2 = id;
        void load(id);
      }
    };
  }

  // mock/src/screen.js
  var context = () => window.__mockContext = window.__mockContext || {};
  function setContext(name, value) {
    const ctx = context();
    if (ctx[name] === value) return;
    ctx[name] = value;
    document.dispatchEvent(new CustomEvent("mock:context", { detail: { name, value } }));
  }
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
  function resolveProps(raw2) {
    const out = {};
    for (const [key, value] of Object.entries(raw2 || {})) {
      out[key] = typeof value === "string" && value.startsWith("$ctx.") ? getContext(value.slice(5)) : value;
    }
    return out;
  }
  function compose(raw2, mount2) {
    const follows = Object.values(raw2 || {}).filter((v) => typeof v === "string" && v.startsWith("$ctx.")).map((v) => v.slice(5));
    const mounted = mount2(resolveProps(raw2));
    if (follows.length) onContext((name) => {
      if (follows.includes(name) && mounted && mounted.update) mounted.update(resolveProps(raw2));
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
    const [, field, op, raw2] = m;
    if (!(field in payload2)) return true;
    const left = payload2[field];
    let right = raw2;
    if (/^-?\d+(\.\d+)?$/.test(raw2)) right = Number(raw2);
    else if (raw2 === "true" || raw2 === "false") right = raw2 === "true";
    else right = raw2.replace(/^['"]|['"]$/g, "");
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
      const payload2 = body.payload || {};
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
      const derived = Object.keys(payload2).filter((f) => declared[f] && declared[f].kind === "derived");
      if (derived.length) return json({ error: `${dataCapability} writes the derived field(s) ${derived.join(", ")}: ${aggregate} recomputes them` }, 403);
      const commandId2 = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId2}`;
      const entry = { commandId: commandId2, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload: payload2, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId2, entry);
      s.commands.set(commandId2, entry);
      const known = (await s.manifests())[dataCapability];
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

  // mock/src/GridSettings.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  var raw = props.battlemap ? props : { ...props, battlemap: "$ctx.battlemap" };
  compose(raw, (resolved) => mount(host, resolved, shell));
  if (!props.battlemap) {
    window.Mock?.on("grid-edit", (d) => {
      if (d && d.id) setContext("battlemap", d.id);
    });
    if (getContext("battlemap") === void 0) {
      mockServer().load("media").then((media) => {
        if (media && media.battlemaps.length) setContext("battlemap", media.battlemaps[0].id);
      });
    }
  }
})();
