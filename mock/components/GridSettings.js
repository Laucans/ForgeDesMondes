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
    let shown = battlemapOf(props2);
    void load(shown);
    return {
      update(next) {
        const id = battlemapOf(next);
        if (id === shown) return;
        shown = id;
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
  var soundTags = (tags) => [...new Set((Array.isArray(tags) ? tags : []).map((t) => [...String(t).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "")].map((c) => c.toLowerCase()).join("")).filter(Boolean))];
  var COMMANDS = {
    // live.showCover: the players'
    // view goes back to the campaign cover, whatever its base was.
    "live.showCover": async (server) => {
      const diffusion = await server.load("live.getDiffusion");
      Object.assign(diffusion, { base: { kind: "cover", id: "ill-cover" }, view: null });
      return void 0;
    },
    // live.closeWindow: the floating
    // illustration over the players' view is closed; their base stays.
    "live.closeWindow": async (server) => {
      (await server.load("live.getDiffusion")).window = null;
      return void 0;
    },
    // soundbox.addSound (crates/soundbox/data-capabilities/add-sound): a sound
    // added to the library. One with no name or no file is refused before
    // anything is queued, as the server refuses it. Its tags, a list of texts,
    // are normalised (`tags-normalized`); its own volume is a percentage from
    // 0 to 100 — a number or the text of one — full when none is given
    // (`sound-volume-range`).
    "soundbox.addSound": async (server, { payload: payload2 }) => {
      const name = typeof payload2.name === "string" ? payload2.name.trim() : "";
      const file = typeof payload2.file === "string" ? payload2.file.trim() : "";
      if (!name || !file) return ["unprocessable"];
      const tags = payload2.tags ?? [];
      if (!Array.isArray(tags) || !tags.every((t) => typeof t === "string")) return ["tags-normalized"];
      const given = payload2.volume ?? 100;
      const volume = typeof given === "number" || typeof given === "string" && given.trim() !== "" ? Number(given) : NaN;
      if (!(volume >= 0 && volume <= 100)) return ["sound-volume-range"];
      const library = await server.load("soundbox.listSounds");
      library.sounds.unshift({ id: server.uuid(), name, tags: soundTags(tags), duration: 0, volume: Math.round(volume), file });
      return void 0;
    },
    // session.createSession (crates/session/data-capabilities/create-session):
    // the next session of the campaign, at the position after the last, in
    // preparation, with the PCs ticked present. `present-only-pj` refuses a
    // character that is not a PC of the fixtures; `oneshot-single-session` a
    // second session of a oneshot.
    "session.createSession": async (server, { payload: payload2 }) => {
      const sessions = await server.load("session.listSessions");
      const characters = await server.load("campaign.listCharacters");
      const campaign = (await server.load("campaign.listCampaigns") || []).find((c) => c.id === payload2.campaign);
      const present = Array.isArray(payload2.present) ? payload2.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
      if (campaign && campaign.type === "oneshot" && (campaign.sessions ?? 0) > 0) return ["oneshot-single-session"];
      if (campaign) campaign.sessions = (campaign.sessions ?? 0) + 1;
      sessions.push({
        id: server.uuid(),
        order: sessions.reduce((last, s) => Math.max(last, s.order), 0) + 1,
        title: String(payload2.title || ""),
        date: payload2.plannedOn || null,
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
    "session.addPresentCharacter": async (server, { payload: payload2, targetId }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === (targetId || payload2.session));
      if (!session) return ["not-found"];
      const characters = await server.load("campaign.listCharacters");
      if (!characters.some((c) => c.kind === "pj" && c.id === payload2.character)) return ["present-only-pj"];
      if (!session.present.includes(payload2.character)) session.present.push(payload2.character);
      return void 0;
    },
    // session.reorderScenes (crates/session/data-capabilities/reorder-scenes):
    // the session's scenes in the order the DM gives, every rank rewritten at
    // once, 1 to n. An order that omits a scene of the fixtures' session,
    // repeats one or names one it does not hold is `scene-order-contiguous`,
    // and no rank changes.
    "session.reorderScenes": async (server, { payload: payload2 }) => {
      const scenes = await server.load("session.listScenes");
      const order = Array.isArray(payload2.order) ? payload2.order : [];
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
    "session.removePresentCharacter": async (server, { payload: payload2, targetId }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === (targetId || payload2.session));
      if (!session) return ["not-found"];
      session.present = session.present.filter((pc) => pc !== payload2.character);
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
      const base = { id: server.uuid(), title: String(payload2.title || "").trim() || "Sans titre", file: payload2.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      for (const name of ["media", "media.listMedia"]) {
        const media = await server.load(name);
        if (!media) continue;
        if (kind === "battlemap") media.battlemaps.push({ ...base, width: payload2.width || 0, height: payload2.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: "pointy", color: "text", opacity: 45 } });
        else media.illustrations.push({ ...base, ratio: payload2.width > 0 && payload2.height > 0 ? Math.round(payload2.width / payload2.height * 100) / 100 : null });
      }
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
    // session.addScene (crates/session/data-capabilities/add-scene): a new
    // scene, ranked after the session's last (`scene-order-contiguous`), with
    // its title, its notes and the places it links — each a place of the
    // fixtures' campaign, else `scene-links-same-campaign` — whose media
    // follow it to the session (`session-media-by-place`, derived, never
    // stored). Its notes are kept as written. A blank title is
    // `scene-title-required`: the mock answers violation ids, where the
    // server answers 422.
    "session.addScene": async (server, { payload: payload2 }) => {
      const title2 = String(payload2.title || "").trim();
      if (!title2) return ["scene-title-required"];
      const known = new Set((await server.load("campaign.listPlaces")).map((p) => p.id));
      const places = [...new Set(Array.isArray(payload2.places) ? payload2.places : [])];
      if (places.some((id) => !known.has(id))) return ["scene-links-same-campaign"];
      const library = await server.load("media.listMedia");
      const media = library.battlemaps.concat(library.illustrations).filter((m) => places.includes(m.place)).map((m) => m.id);
      const scenes = await server.load("session.listScenes");
      scenes.push({
        id: server.uuid(),
        order: scenes.reduce((last, sc) => Math.max(last, sc.order), 0) + 1,
        title: title2,
        places,
        characters: [],
        events: [],
        media,
        toCreate: [],
        battlemap: null,
        notes: typeof payload2.notes === "string" ? payload2.notes : null
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
    "session.addPlaceToSession": async (server, { payload: payload2 }) => {
      const place = payload2.place;
      const scenes = await server.load("session.listScenes") || [];
      const first = scenes.reduce((low, sc) => !low || sc.order < low.order ? sc : low, null);
      const scene = payload2.scene ? scenes.find((sc) => sc.id === payload2.scene) : first;
      if (!scene) return ["not-found"];
      if (!(await server.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["scene-links-same-campaign"];
      if (!scene.places.includes(place)) scene.places.push(place);
      const library = await server.load("media.listMedia") || { battlemaps: [], illustrations: [] };
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
    // soundbox.updateSound (crates/soundbox/data-capabilities/update-sound): the
    // name, tags and volume of one sound, tags normalised and volume ranged as
    // for an addition; a sound that is not there is `not-found`.
    "soundbox.updateSound": async (server, { payload: payload2, targetId }) => {
      const sound = (await server.load("soundbox.listSounds")).sounds.find((s) => s.id === targetId);
      if (!sound) return ["not-found"];
      if ("volume" in payload2 && !(Number(payload2.volume) >= 0 && Number(payload2.volume) <= 100)) return ["sound-volume-range"];
      if ("name" in payload2) sound.name = String(payload2.name);
      if ("tags" in payload2) sound.tags = soundTags(payload2.tags);
      if ("volume" in payload2) sound.volume = Number(payload2.volume);
      if ("file" in payload2) sound.file = String(payload2.file);
      return void 0;
    },
    // soundbox.removeSound (crates/soundbox/data-capabilities/remove-sound): the
    // sound leaves the library's readers (a soft delete, its tombstone is the
    // server's).
    "soundbox.removeSound": async (server, { targetId }) => {
      const library = await server.load("soundbox.listSounds");
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
    "soundbox.setMasterVolume": async (server, { payload: payload2, targetId }) => {
      if (!String(targetId || payload2.campaign || "").trim()) return ["not-found"];
      const { volume } = payload2;
      if (typeof volume !== "number" || !(volume >= 0 && volume <= 100)) return ["master-volume-range"];
      for (const name of ["soundbox.listSounds", "sounds"]) {
        const fixture = await server.load(name);
        if (fixture) fixture.masterVolume = Math.round(volume);
      }
      return void 0;
    },
    // soundbox.stopAll (crates/soundbox/data-capabilities/stop-all): every
    // running playback gets its stop (`stop-all`); with none running it still
    // succeeds and changes nothing.
    "soundbox.stopAll": async (server) => {
      for (const sound of (await server.load("soundbox.listSounds")).sounds) sound.playing = false;
      return void 0;
    },
    // soundbox.setSoundVolume (crates/soundbox/data-capabilities/set-sound-volume):
    // one sound's own volume, in percent, from 0 to 100; out of range it is
    // refused by `sound-volume-range`, never clamped. Nothing else of the sound
    // changes; its effective gain follows (EffectiveVolume@1).
    "soundbox.setSoundVolume": async (server, { payload: payload2, targetId }) => {
      const id = targetId || payload2?.sound;
      const fixtures = (await Promise.all(["soundbox.listSounds", "sounds"].map((name) => server.load(name)))).filter(Boolean);
      const found = fixtures.map((fixture) => fixture.sounds.find((candidate) => candidate.id === id)).filter(Boolean);
      if (!found.length) return ["not-found"];
      const volume = payload2?.volume;
      if (typeof volume !== "number" || !(volume >= 0 && volume <= 100)) return ["sound-volume-range"];
      for (const sound of found) sound.volume = volume;
      return void 0;
    },
    // campaign.createCampaign (crates/campaign/data-capabilities/create-campaign):
    // a new campaign at the top of the home page, its master volume at full
    // (`master-volume-range`); a oneshot comes with its single session, in
    // preparation (`oneshot-single-session`), a campagne with none. A campaign
    // with no name is refused, as one of a kind there is not.
    "campaign.createCampaign": async (server, { payload: payload2 }) => {
      const name = String(payload2.name ?? "").trim();
      if (!name) return ["campaign-name-required"];
      if (!["campagne", "oneshot"].includes(payload2.type)) return ["campaign-type-unknown"];
      const text2 = (field) => typeof payload2[field] === "string" ? payload2[field] : null;
      (await server.load("campaign.listCampaigns")).unshift({
        id: server.uuid(),
        name,
        type: payload2.type,
        art: text2("art"),
        glyph: text2("glyph"),
        pitch: text2("pitch"),
        sessions: payload2.type === "oneshot" ? 1 : 0,
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
    "ai.saveSettings": async (server, { payload: payload2 }) => {
      const settings = await server.load("ai.getSettings");
      const models = { generationModel: payload2.generationModel, summaryModel: payload2.summaryModel };
      if (!String(models.generationModel || "") || !String(models.summaryModel || "")) return ["single-settings-row"];
      if (payload2.mode === "api") {
        const url = String(payload2.url || "");
        const type = apiType(settings, url);
        if (!type) return ["api-type-recognized"];
        const key = String(payload2.key || "");
        settings.providers[type] = { ...settings.providers[type], url, ...models, key: key ? `\u2022\u2022\u2022\u2022${key.slice(-4)}` : "" };
        Object.assign(settings, { mode: "api", provider: type, binary: null });
        return void 0;
      }
      if (payload2.mode === "binary") {
        const binary = { claude: "claude-code", "claude-code": "claude-code", opencode: "opencode" }[payload2.binary];
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
    // session.closeSession (crates/session/data-capabilities/close-session): the
    // live session closed in one action (`close-session`) — its microphone off,
    // its close dated, its journal kept as it was — and a summary of its journal
    // written by the active provider's summary model, the journal's text alone
    // sent, as a Journal request (`text-only-no-audio`). The summary is the one
    // the next preparation shows. A session that is not live is refused by
    // `status-lifecycle`; one that is not the live session is `not-found`.
    "session.closeSession": async (server, { payload: payload2, targetId }) => {
      const live = await server.load("session.getLiveSession");
      if (!live || live.id !== (targetId || payload2.id)) return ["not-found"];
      if ((live.status || "live") !== "live") return ["status-lifecycle"];
      const settings = await server.load("ai.getSettings");
      const binary = settings.mode === "binary" && settings.binary;
      const model = (binary || settings.providers?.[settings.provider] || {}).summaryModel || "";
      const provider = binary ? `${binary.binary}-binary` : settings.provider === "opencode" ? "opencode-server" : "cloud";
      const { journal = [] } = await server.load("session.getSessionJournal") || {};
      const lines = journal.length ? journal.map((line2) => `${line2.t} ${line2.text}`) : ["(aucune ligne transcrite)"];
      const text2 = [`Journal de la s\xE9ance \xAB ${live.title} \xBB`, ...lines].join("\n");
      const requests = (await server.load("ai.listProviderRequests"))?.requests;
      if (requests) requests.push({ sequence: requests.length + 1, kind: "journal", provider, text: text2, modalities: ["text"] });
      const summary = `R\xE9sum\xE9 de la s\xE9ance ${live.order} g\xE9n\xE9r\xE9 par ${model} \xE0 partir de ${journal.length} lignes du journal.`;
      Object.assign(live, { status: "closed", closedAt: (/* @__PURE__ */ new Date()).toISOString(), microphone: { on: false }, summary, summaryModel: model });
      const previous = await server.load("session.getPreviousSessionSummary");
      if (previous) Object.assign(previous, { session: { id: live.id, order: live.order, title: live.title, date: live.date }, summary, summaryMeta: `G\xE9n\xE9r\xE9 \xE0 la cl\xF4ture par ${model}` });
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
      const before = order.findLastIndex((o) => o.init > init || o.init === init && (dexScore === null || o.dex >= dex));
      order.splice(before + 1, 0, { id: token.id, init, dex });
      combat.order = order;
      return void 0;
    },
    // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative):
    // the open combat of the instance `targetId` (or `payload.instance`)
    // reordered as `payload.order` gives it, every participant once, named by
    // its token — the ranks 1 to n in that order, a hidden participant like any
    // other (`hidden-keeps-rank`); the turn and the round stay. An unknown
    // instance is `not-found`; one with no open combat, or an order that is no
    // list, omits a participant, repeats one or names one not in the combat, is
    // refused by `initiative-tiebreak`, as the server refuses them (422). Ids
    // are compared in any case and without the blanks around them, as the
    // server does.
    "table.reorderInitiative": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload2.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      const combat = instance.state.combat;
      const held = combat && combat.order || [];
      const named = Array.isArray(payload2.order) ? payload2.order : [];
      if (!combat || !named.length || named.length !== held.length) return ["initiative-tiebreak"];
      const order = [];
      for (const n of named) {
        const at = held.findIndex((o) => same2(o.id, n) && !order.includes(o));
        if (at < 0) return ["initiative-tiebreak"];
        order.push(held[at]);
      }
      combat.order = order;
      return void 0;
    },
    // table.removeParticipant (crates/table/data-capabilities/remove-participant):
    // one participant taken out of the open combat, `payload.participant` — named
    // by its token, as the carousel shows it — on the instance `targetId`, or the
    // participant itself as the target when `payload.participant` is absent or
    // null. Its entry is tombstoned and nothing else is written: its token stays
    // on the map, and the order closes up. The participant holding the turn is
    // refused by `next-turn`: the Battlemap sends table.nextTurn first. A
    // participant in no open combat, or in one of another instance than the one
    // named, is `not-found`. Ids are compared in any case and without the blanks
    // around them, as the server does.
    "table.removeParticipant": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const named = payload2?.participant != null;
      const participant = named ? payload2.participant : targetId;
      if (typeof participant !== "string") return ["not-found"];
      const instance = named && targetId && !same2(targetId, participant) ? targetId : null;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => (instance == null || same2(i.id, instance)) && (i.state.combat?.order || []).some((o) => same2(o.id, participant)));
      if (!holder) return ["not-found"];
      const combat = holder.state.combat;
      if (same2(combat.current, participant)) return ["next-turn"];
      combat.order = combat.order.filter((o) => !same2(o.id, participant));
      return void 0;
    },
    // table.endCombat (crates/table/data-capabilities/end-combat): the open
    // combat of the instance `targetId` (or `payload.instance`) ends — no turn
    // is current and the carousel goes; its tokens, their HP and their
    // conditions stay as they are, and a new combat may start
    // (`one-active-combat`). An unknown instance, or one with no open combat,
    // is refused by `one-active-combat`, as the server refuses it (422). Ids
    // are compared in any case and without the blanks around them, as the
    // server does.
    "table.endCombat": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload2?.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance?.state.combat) return ["one-active-combat"];
      instance.state.combat = null;
      return void 0;
    },
    // table.resetBattlemap (crates/table/data-capabilities/reset-battlemap,
    // « Réinitialiser la carte »): the instance `targetId` — or
    // `payload.instance` — loses every token, its combat and its initiative;
    // the instance itself, its battlemap and its grid stay, and no other
    // instance is touched (`instance-isolated`). When it was reset is the
    // command journal's, which no read shows. An unknown instance is
    // `not-found`; one with nothing on its map is refused by
    // `state-persisted-reset`, as the server refuses it (422). Ids are compared
    // in any case and without the blanks around them, as the server does.
    "table.resetBattlemap": async (server, { payload: payload2, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload2?.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      if (!instance.state.tokens.length && !instance.state.combat) return ["state-persisted-reset"];
      instance.state.tokens = [];
      instance.state.combat = null;
      return void 0;
    },
    // soundbox.playSound (crates/soundbox/data-capabilities/play-sound): a
    // playback of the sound starts, whatever else plays (`simultaneous-playback`),
    // from the soundbox (the default) or a suggestion's pastille; any other
    // source, the player view's, is refused (`dm-side-only`). The sound plays
    // as soundbox.listSounds says it: `playing`.
    "soundbox.playSound": async (server, { payload: payload2, targetId }) => {
      const id = targetId || payload2?.sound;
      const sound = (await server.load("soundbox.listSounds")).sounds.find((candidate) => candidate.id === id);
      if (!sound) return ["not-found"];
      const source = payload2?.source ?? "soundbox";
      if (!["soundbox", "suggestion"].includes(source)) return ["dm-side-only"];
      sound.playing = true;
      return void 0;
    }
  };

  // mock/src/reads.js
  async function closedBySession(server, variables) {
    const bar = await server.load("LiveSessionBar");
    return bar && variables.session === bar.session.id ? bar : null;
  }
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e" };
  var READS = {
    // session.getLiveSession (crates/session/capabilities/get-live-session): the running session as
    // `{ session, microphone }` — its header and the PCs ticked present (those of the session in
    // `session.listSessions`), the microphone listening until the session is closed — and, beside
    // them, the scenes of its screen (fixtures/LiveSessionBar.json) the bar offers and marks the
    // current one of.
    "session.getLiveSession": async (server) => {
      const live = await server.load("session.getLiveSession");
      const bar = await server.load("LiveSessionBar");
      if (!live) return live;
      const listed = (await server.load("session.listSessions")).find((s) => s.id === live.id);
      const status = live.status || "live";
      return {
        session: { ...live, status, statusLabel: STATUS_LABELS[status] ?? null, present: listed?.present ?? [] },
        microphone: { on: status !== "closed" },
        ...bar ? { scenes: bar.scenes, currentScene: bar.currentScene } : {}
      };
    },
    "session.getSessionJournal": async (server, variables) => {
      const bar = await closedBySession(server, variables);
      if (!bar) return server.load("session.getSessionJournal");
      return { journal: Array.from({ length: bar.closing.journalLines }, (_, i) => ({ t: bar.session.startedAt, text: `Ligne ${i + 1} du journal transcrit` })) };
    },
    "session.getPreviousSessionSummary": async (server, variables) => {
      const bar = await closedBySession(server, variables);
      if (!bar) return server.load("session.getPreviousSessionSummary");
      const { id, order, title: title2, date } = bar.session;
      return {
        session: { id, order, title: title2, date },
        summary: bar.closing.summary,
        summaryMeta: `G\xE9n\xE9r\xE9 \xE0 la cl\xF4ture par ${bar.closing.summaryModel}`,
        mentions: bar.closing.mentions.map((name) => ({ id: name, type: "pnj", name })),
        threads: bar.closing.threads
      };
    },
    // table.getBattlemapInstance: the fixture's instance, or one a command made.
    "table.getBattlemapInstance": async (server, { instance }) => {
      const fixture = await server.load("table.getBattlemapInstance");
      if (!instance || instance === fixture.id) return fixture;
      return server.instances && server.instances.get(instance);
    },
    // session.getSession (crates/session/capabilities/get-session): one session's
    // header and the PCs present, a session of `session.listSessions` (the ones
    // created since included), its status labelled as SessionLifecycle@1 says. An
    // unknown id is no session: `null`, as the server answers it.
    "session.getSession": async (server, { id }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === id);
      if (!session) return null;
      const status = session.status === "played" ? "closed" : session.status;
      return { id: session.id, order: session.order, title: session.title, date: session.date, status, statusLabel: STATUS_LABELS[status] ?? null, present: session.present };
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
    // soundbox.listSounds (crates/soundbox/capabilities/list-sounds): the
    // campaign's sounds with their effective gain — the own volume times the
    // master volume, both percents (EffectiveVolume@1) — and whether one plays
    // now (the fixture's `playing`, which soundbox.playSound and soundbox.stopAll move). The optional `search` keeps the
    // names that hold it and `tag` the sounds that carry it, whatever the case
    // (SoundTag@1); the fixture's `duration` is kept, the store has none yet.
    "soundbox.listSounds": async (server, { search, tag }) => {
      const { masterVolume, sounds } = await server.load("soundbox.listSounds");
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
    "media.getBattlemap": async (server, { id }) => {
      const battlemap = (await server.load("media")).battlemaps.find((b) => b.id === id);
      if (!battlemap) return null;
      const { opacity, ...grid } = battlemap.grid;
      return { ...battlemap, grid: { ...grid, opacity: opacity / 100, feetPerHex: 5 } };
    },
    // session.listScenes (crates/session/capabilities/list-scenes): a session's scenes by rank. The fixture
    // is the scenes of session 4, the one in preparation; another session has none yet.
    "session.listScenes": async (server, { session }) => session === "s4" ? await server.load("session.listScenes") : [],
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
