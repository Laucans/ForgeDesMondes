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

  // apps/soundbox/live-soundbox/src/audio.ts
  var soundUrl = (file) => `/sounds/${file.split("/").map(encodeURIComponent).join("/")}`;
  var browserPlayer = (sound, gain, onEnded, onBlocked) => {
    const element = document.createElement("audio");
    element.src = soundUrl(sound.file);
    element.hidden = true;
    document.body.append(element);
    let over = false;
    let silent;
    const end = () => {
      if (over) return;
      over = true;
      clearTimeout(silent);
      element.remove();
      onEnded();
    };
    const fallBack = () => {
      if (over || silent !== void 0) return;
      silent = setTimeout(end, (sound.duration ?? 0) * 1e3);
    };
    element.volume = gain;
    element.addEventListener("ended", end);
    element.addEventListener("error", fallBack);
    element.play().catch((error) => {
      if (error instanceof DOMException && error.name === "NotAllowedError") {
        over = true;
        element.remove();
        onBlocked();
      } else fallBack();
    });
    return {
      setGain(next) {
        element.volume = next;
      },
      stop() {
        over = true;
        clearTimeout(silent);
        element.pause();
        element.remove();
      }
    };
  };

  // apps/soundbox/live-soundbox/src/ids.ts
  var LIST_SOUNDS = "soundbox.listSounds";
  var PLAY_SOUND = "soundbox.playSound";
  var STOP_ALL = "soundbox.stopAll";
  var SET_SOUND_VOLUME = "soundbox.setSoundVolume";
  var SET_MASTER_VOLUME = "soundbox.setMasterVolume";
  var DATA_CAPABILITY_VERSION = 1;
  var CAMPAIGN_CONTEXT = "campaign";

  // apps/soundbox/live-soundbox/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var num = (value, fallback) => typeof value === "number" && Number.isFinite(value) ? value : fallback;
  var clamp = (value, min, max) => Math.min(max, Math.max(min, value));
  function parseSound(value) {
    const id = text(value.id);
    if (id === "") return null;
    return {
      id,
      name: text(value.name),
      tags: texts(value.tags),
      duration: typeof value.duration === "number" && value.duration > 0 ? value.duration : null,
      volume: clamp(num(value.volume, 0), 0, 100),
      file: text(value.file),
      // A sound whose gain the Capability did not give plays silent: the gain is never worked out here.
      gain: clamp(num(value.gain, 0), 0, 1),
      playing: value.playing === true
    };
  }
  function parseLibrary(data) {
    const answer = isRecord(data) ? data : {};
    const sounds = Array.isArray(answer.sounds) ? answer.sounds.filter(isRecord).map(parseSound).filter((sound) => sound !== null) : [];
    return { masterVolume: clamp(num(answer.masterVolume, 0), 0, 100), sounds };
  }
  var fold = (value) => value.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase().trim();
  var tagKey = (tag) => tag.trim().toLowerCase();
  function allTags(sounds) {
    return Array.from(new Set(sounds.flatMap((sound) => sound.tags.map(tagKey)).filter((tag) => tag !== ""))).sort((a, b) => a.localeCompare(b, "fr"));
  }
  function visible(sounds, query, tags) {
    const needle = fold(query);
    return sounds.filter(
      (sound) => (needle === "" || [sound.name, ...sound.tags].some((field) => fold(field).includes(needle))) && (tags.size === 0 || sound.tags.some((tag) => tags.has(tagKey(tag))))
    );
  }
  function commandBody(dataCapability, payload, targetId) {
    return {
      dataCapability: `${dataCapability}@${DATA_CAPABILITY_VERSION}`,
      payload,
      ...targetId === void 0 ? {} : { target: { id: targetId } },
      caller: { by: "dm", from: "LiveSoundbox", roles: [] }
    };
  }
  var VIOLATIONS = {
    "dm-side-only": "seule la DM view joue un son",
    "sound-volume-range": "le volume d\u2019un son reste entre 0 et 100 %",
    "master-volume-range": "le volume global reste entre 0 et 100 %",
    "stop-all": "impossible d\u2019arr\xEAter les sons"
  };
  var reasons = (violations) => violations.map((v) => VIOLATIONS[v] ?? v).join(", ");
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
  function settled(answer) {
    const result = isRecord(answer) && isRecord(answer.result) ? answer.result : {};
    return { status: text(result.status), violations: texts(result.violations) };
  }
  var isFinal = (status) => FINAL.has(status);
  function commandId(answer) {
    return isRecord(answer) ? text(answer.commandId) || (isRecord(answer.result) ? text(answer.result.commandId) : "") : "";
  }
  var seconds = (duration) => duration === null ? "" : ` \xB7 ${String(duration).replace(".", ",")} s`;

  // apps/soundbox/live-soundbox/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  function masterSlider() {
    return '<div class="kit-slider kit-slider--bare"><div class="kit-slider__field" data-name="master"><div class="kit-slider__head"><label class="kit-slider__label" for="soundbox-master-volume">Volume global</label><span class="kit-slider__value"><output class="kit-slider__out" for="soundbox-master-volume">0</output><span class="kit-slider__unit">%</span></span></div><input class="kit-slider__input" id="soundbox-master-volume" type="range" name="master" min="0" max="100" value="0" style="--fill: 0%" disabled><div class="kit-slider__bounds"><span>0 %</span><span>100 %</span></div></div></div>';
  }
  function setSlider(slider, value) {
    slider.value = String(value);
    slider.style.setProperty("--fill", `${value}%`);
    const out = slider.closest(".kit-slider__field")?.querySelector(".kit-slider__out");
    if (out) out.textContent = String(value);
  }
  function skeleton() {
    const reads = `data-reads="${LIST_SOUNDS}"`;
    return `<section class="mf-live-sb" id="live-soundbox" aria-labelledby="soundbox-title"><div class="panel__head"><h2 id="soundbox-title">Soundbox</h2><span class="small muted" id="soundbox-playing-count" ${reads} aria-live="polite"></span></div><div class="mf-live-sb__bar" id="soundbox-controls"><div id="soundbox-search"><div class="kit-input"><label>Rechercher un son<input type="search" id="soundbox-search-input" placeholder="porte, foule, corbeau\u2026"></label></div></div><div id="soundbox-master" ${reads} data-writes="${SET_MASTER_VOLUME}">${masterSlider()}</div><div class="kit-button kit-button--bare"><button type="button" class="danger" id="soundbox-stop-all" data-writes="${STOP_ALL}">Tout arr\xEAter</button></div></div><div class="row" id="soundbox-tags" role="group" aria-label="Filtrer par tag" ${reads}></div><p class="small mf-live-sb__message" id="soundbox-message" role="status"></p><ul class="mf-live-sb__grid" id="soundbox-grid" ${reads}></ul></section>`;
  }
  function playingCount(count) {
    const only = "le son sort uniquement de cette vue";
    return count === 0 ? "Le son sort uniquement de cette vue" : `${count} son${count > 1 ? "s" : ""} en cours \xB7 ${only}`;
  }
  var tagId = (tag) => Array.from(tag).map((c) => /[a-z0-9]/.test(c) ? c : `_${c.codePointAt(0)?.toString(16) ?? ""}_`).join("");
  function tagChips(tags, chosen) {
    return tags.map((tag) => `<button type="button" class="chip${chosen.has(tag) ? " is-on" : ""}" aria-pressed="${chosen.has(tag)}" data-tag="${esc(tag)}" id="soundbox-tag-${tagId(tag)}">#${esc(tag)}</button>`).join("");
  }
  function box(sound, playing) {
    const id = esc(sound.id);
    return `<li class="mf-live-sb__box${playing ? " is-playing" : ""}" id="soundbox-${id}" data-id="${id}"><button type="button" class="mf-live-sb__play" data-writes="${PLAY_SOUND}" data-play="${id}"><b>${esc(sound.name)}</b><small>${esc(sound.tags.join(" \xB7 "))}${seconds(sound.duration)}</small></button><label class="mf-live-sb__vol">Vol. <input type="range" min="0" max="100" value="${sound.volume}" data-writes="${SET_SOUND_VOLUME}" data-volume="${id}" aria-label="Volume de ${esc(sound.name)}"><span>${sound.volume}</span></label><span class="mf-live-sb__progress"></span></li>`;
  }
  function grid(sounds, playing) {
    return sounds.map((sound) => box(sound, playing.has(sound.id))).join("") || '<li class="muted">Aucun son ne correspond.</li>';
  }

  // apps/soundbox/live-soundbox/src/style.ts
  var STYLE = `
.mf-live-sb { box-sizing: border-box; display: grid; gap: var(--space-3); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-live-sb *, .mf-live-sb *::before, .mf-live-sb *::after { box-sizing: inherit; }
.mf-live-sb h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; font-size: var(--font-size-large); color: var(--color-accent); }
.mf-live-sb p { margin: 0; }
.mf-live-sb .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-live-sb .muted { color: var(--color-text-muted); }
.mf-live-sb .small { font-size: var(--font-size-small); }
.mf-live-sb .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-live-sb .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-live-sb .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-live-sb .chip.is-on { background: var(--color-primary); border-color: var(--color-primary); color: var(--color-primary-contrast); font-weight: var(--font-weight-heading); }
.mf-live-sb .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-live-sb .kit-slider.kit-slider--bare { padding: 0; background: transparent; border: 0; box-shadow: none; max-width: none; }
.mf-live-sb .kit-input.is-wide { max-width: none; }
.mf-live-sb__bar { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 2fr) auto; gap: var(--space-3); align-items: end; }
.mf-live-sb__bar .kit-input { max-width: none; }
.mf-live-sb__message:empty { display: none; }
.mf-live-sb__message { color: var(--color-danger); }
.mf-live-sb__grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 4.5), 1fr)); gap: var(--space-3); }
.mf-live-sb__box { position: relative; display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); overflow: hidden; }
.mf-live-sb__box.is-playing { border-color: var(--color-success); box-shadow: var(--shadow-high); }
.mf-live-sb__play { display: grid; gap: var(--space-1); text-align: left; background: none; border: 0; padding: 0; color: var(--color-text); font: inherit; cursor: pointer; }
.mf-live-sb__play b { font-size: var(--font-size-base); }
.mf-live-sb__play small { color: var(--color-text-muted); }
.mf-live-sb__box.is-playing .mf-live-sb__play b::before { content: "\u25B6 "; color: var(--color-success); }
.mf-live-sb__vol { display: flex; align-items: center; gap: var(--space-2); font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-live-sb__vol input { flex: 1; accent-color: var(--color-primary); }
.mf-live-sb__progress { position: absolute; left: 0; bottom: 0; height: var(--space-1); width: 0; background: var(--color-success); }
@media (max-width: 900px) { .mf-live-sb__bar { grid-template-columns: minmax(0, 1fr); } }
`;

  // apps/soundbox/live-soundbox/src/index.ts
  var WAITING = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 30 };
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="LiveSoundbox"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "LiveSoundbox";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function mount(host2, _props, shell2, player = browserPlayer, timing = WAITING) {
    ensureStyle(host2.ownerDocument);
    host2.innerHTML = skeleton();
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`LiveSoundbox: #${id} is not in the panel`);
      return found;
    };
    const boxOf = (id) => [...host2.querySelectorAll("[data-id]")].find((box2) => box2.dataset.id === id);
    let library = parseLibrary(void 0);
    let query = "";
    const chosen = /* @__PURE__ */ new Set();
    const heard = /* @__PURE__ */ new Map();
    const asked = /* @__PURE__ */ new Map();
    let running = /* @__PURE__ */ new Set();
    let firstRead = true;
    let unreadable = false;
    let over = false;
    let dragging = false;
    let repaintLater = false;
    let loads = 0;
    let seen = -1;
    const campaign = () => {
      const id = shell2.context.get(CAMPAIGN_CONTEXT);
      return typeof id === "string" && id !== "" ? { campaign: id } : {};
    };
    const say = (message) => {
      part("soundbox-message").textContent = message;
    };
    const progress = (id) => {
      const box2 = boxOf(id);
      const bar = box2?.querySelector(".mf-live-sb__progress");
      if (!bar || typeof bar.animate !== "function") return;
      for (const animation of bar.getAnimations()) animation.cancel();
      const now = heard.get(id);
      const duration = library.sounds.find((sound) => sound.id === id)?.duration;
      if (!now || duration === null || duration === void 0) return;
      const done = Math.min(1, (Date.now() - now.startedAt) / (duration * 1e3));
      bar.animate([{ width: `${done * 100}%` }, { width: "100%" }], { duration: duration * 1e3 * (1 - done), easing: "linear" });
    };
    const keepingFocus = (region, draw) => {
      const active = host2.ownerDocument.activeElement;
      const key = active && region.contains(active) ? ["play", "volume", "tag"].map((name) => [name, active.dataset[name]]).find(([, value]) => value !== void 0) : void 0;
      draw();
      if (key) [...region.querySelectorAll(`[data-${key[0]}]`)].find((element) => element.dataset[key[0]] === key[1])?.focus();
    };
    const paintCount = () => {
      part("soundbox-playing-count").textContent = playingCount(heard.size);
    };
    const paintGrid = () => {
      if (dragging) {
        repaintLater = true;
        return;
      }
      repaintLater = false;
      const region = part("soundbox-grid");
      keepingFocus(region, () => {
        region.innerHTML = grid(visible(library.sounds, query, chosen), new Set(heard.keys()));
      });
      for (const id of heard.keys()) progress(id);
      paintCount();
    };
    const paintBox = (id) => {
      boxOf(id)?.classList.toggle("is-playing", heard.has(id));
      progress(id);
      paintCount();
    };
    const paintTags = () => {
      const tags = allTags(library.sounds);
      for (const tag of [...chosen]) if (!tags.includes(tag)) chosen.delete(tag);
      const region = part("soundbox-tags");
      keepingFocus(region, () => {
        region.innerHTML = tagChips(tags, chosen);
      });
    };
    const paintMaster = () => {
      if (dragging) return;
      const slider = part("soundbox-master-volume");
      slider.disabled = false;
      setSlider(slider, library.masterVolume);
    };
    const hush = (id) => {
      heard.get(id)?.voice.stop();
      heard.delete(id);
    };
    function listen(id) {
      const sound = library.sounds.find((candidate) => candidate.id === id);
      if (!sound) return void 0;
      hush(id);
      const now = {
        startedAt: Date.now(),
        voice: player(
          sound,
          sound.gain,
          () => {
            if (heard.get(id) !== now) return;
            heard.delete(id);
            paintBox(id);
          },
          () => {
            if (heard.get(id) !== now) return;
            heard.delete(id);
            paintBox(id);
            say("Le navigateur bloque le son : un clic sur la page l\u2019autorise.");
          }
        )
      };
      heard.set(id, now);
      paintBox(id);
      return now;
    }
    const reconcile = () => {
      for (const [id, now] of [...heard]) {
        const sound = library.sounds.find((candidate) => candidate.id === id);
        if (!sound || !sound.playing && (asked.get(id) ?? 0) === 0) hush(id);
        else now.voice.setGain(sound.gain);
      }
      const playing = new Set(library.sounds.filter((sound) => sound.playing).map((sound) => sound.id));
      if (!firstRead) {
        for (const id of playing) if (!running.has(id) && !heard.has(id) && (asked.get(id) ?? 0) === 0) listen(id);
      }
      running = playing;
      firstRead = false;
    };
    async function load() {
      loads += 1;
      const mine = loads;
      try {
        const read = await shell2.capability(LIST_SOUNDS, void 0, campaign());
        if (mine !== loads || over) return true;
        library = parseLibrary(read.data);
        seen = read.asOf;
        reconcile();
        paintTags();
        paintGrid();
        paintMaster();
        if (unreadable) {
          unreadable = false;
          say("");
        }
        return true;
      } catch {
        unreadable = true;
        say("Impossible de lire les sons.");
        return false;
      }
    }
    const lines = /* @__PURE__ */ new Map();
    function inLine(key, run) {
      const result = (lines.get(key) ?? Promise.resolve()).then(run, run);
      lines.set(key, result.catch(() => void 0));
      return result;
    }
    async function send(dataCapability, payload, targetId) {
      const answer = await shell2.command(commandBody(dataCapability, payload, targetId));
      const id = commandId(answer);
      let state = settled(answer);
      for (let polled = 0; !isFinal(state.status) && id !== "" && polled < timing.polls; polled += 1) {
        await timing.sleep(timing.everyMs);
        state = settled(await shell2.commandResult(id));
      }
      return state;
    }
    async function act(key, wrong, dataCapability, payload, targetId) {
      let outcome = "refused";
      try {
        const state = await inLine(key, () => send(dataCapability, payload, targetId));
        if (state.status === "applied") {
          outcome = "applied";
          say("");
        } else if (isFinal(state.status)) say(`${wrong} : ${state.violations.length > 0 ? reasons(state.violations) : state.status}.`);
        else {
          outcome = "waiting";
          say(`${wrong} : le serveur ne l\u2019a pas encore pris en compte (\xE9tat : ${state.status || "inconnu"}).`);
        }
      } catch {
        say(`${wrong}.`);
      }
      await load();
      return outcome;
    }
    function play(id) {
      const now = listen(id);
      if (!now) return;
      asked.set(id, (asked.get(id) ?? 0) + 1);
      void act("playback", "Le son n\u2019a pas pu \xEAtre lanc\xE9", PLAY_SOUND, {}, id).then((outcome) => {
        if (outcome === "refused" && heard.get(id) === now) {
          hush(id);
          paintBox(id);
        }
      }).finally(() => {
        asked.set(id, (asked.get(id) ?? 1) - 1);
      });
    }
    function stopAll() {
      for (const id of [...heard.keys()]) hush(id);
      paintGrid();
      void act("playback", "Impossible de tout arr\xEAter", STOP_ALL, campaign());
    }
    host2.addEventListener("input", (event) => {
      const target = event.target;
      if (target?.id === "soundbox-search-input") {
        query = target.value ?? "";
        paintGrid();
      } else if (target?.type === "range") {
        dragging = true;
        const slider = event.target;
        if (target.dataset?.volume !== void 0) {
          const sound = library.sounds.find((candidate) => candidate.id === target.dataset?.volume);
          if (sound) sound.volume = Number(slider.value);
          const readout = slider.nextElementSibling;
          if (readout) readout.textContent = slider.value;
        } else {
          slider.style.setProperty("--fill", `${slider.value}%`);
          const out = slider.closest(".kit-slider__field")?.querySelector(".kit-slider__out");
          if (out) out.textContent = slider.value;
        }
      }
    });
    host2.addEventListener("change", (event) => {
      const slider = event.target;
      if (slider?.type !== "range") return;
      dragging = false;
      const volume = Number(slider.value);
      const id = slider.dataset.volume;
      if (id !== void 0) void act(`volume:${id}`, "Le volume du son n\u2019a pas pu \xEAtre chang\xE9", SET_SOUND_VOLUME, { volume }, id);
      else if (slider.name === "master") void act("master", "Le volume global n\u2019a pas pu \xEAtre chang\xE9", SET_MASTER_VOLUME, { ...campaign(), volume });
      if (repaintLater) paintGrid();
    });
    host2.addEventListener("click", (event) => {
      const button = event.target?.closest("button");
      if (!button) return;
      if (button.dataset.writes === PLAY_SOUND && button.dataset.play !== void 0) play(button.dataset.play);
      else if (button.dataset.writes === STOP_ALL) stopAll();
      else if (button.dataset.tag !== void 0) {
        const tag = tagKey(button.dataset.tag);
        if (!chosen.delete(tag)) chosen.add(tag);
        paintTags();
        paintGrid();
      }
    });
    paintCount();
    void load();
    const stopWatching = [];
    stopWatching.push(
      shell2.onDataVersion((dataVersion) => {
        if (!host2.isConnected) return end();
        if (dataVersion !== seen) void load();
      })
    );
    stopWatching.push(
      shell2.context.on((name) => {
        if (!host2.isConnected) return end();
        if (name !== CAMPAIGN_CONTEXT) return;
        for (const id of [...heard.keys()]) hush(id);
        chosen.clear();
        running = /* @__PURE__ */ new Set();
        firstRead = true;
        void load();
      })
    );
    function end() {
      over = true;
      for (const stop of stopWatching) stop();
      for (const id of [...heard.keys()]) hush(id);
    }
    const release = () => {
      dragging = false;
    };
    host2.addEventListener("pointerup", release);
    host2.addEventListener("pointercancel", release);
    host2.addEventListener("keyup", release);
    host2.addEventListener("focusout", () => {
      release();
      if (repaintLater) paintGrid();
    });
    return { update: () => void load() };
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
        s.tell("CommandRejected", by, commandId2, { violations: entry.violations, reason: `${dataCapability} has no handler: write mock/handlers/${dataCapability}.js` });
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

  // mock/src/LiveSoundbox.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  async function start() {
    if (shell.context.get("campaign") === void 0) {
      const library = await mockServer().load("sounds");
      if (library && library.campaign) shell.context.set("campaign", library.campaign);
    }
    const mounted = compose(props, (resolved) => mount(host, resolved, shell));
    document.addEventListener("fdm:play-sound", (event) => {
      const body = { dataCapability: "soundbox.playSound@1", payload: { source: "suggestion" }, target: { id: event.detail.id }, caller: { by: "dm", from: "SuggestionPill", roles: [] } };
      void shell.command(body).then(() => mounted?.update?.({}));
    });
  }
  void start();
})();
