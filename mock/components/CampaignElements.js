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
      commandResult: (commandId) => call(fetcher, "GET", `/commands/${encodeURIComponent(commandId)}`),
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

  // apps/campaign/campaign-elements/src/ids.ts
  var LIST_PLAYERS = "campaign.listPlayers";
  var LIST_CHARACTERS = "campaign.listCharacters";
  var LIST_PLACES = "campaign.listPlaces";
  var LIST_EVENTS = "campaign.listEvents";
  var LIST_MEDIA = "media.listMedia";
  var LIST_SOUNDS = "soundbox.listSounds";

  // apps/campaign/campaign-elements/src/model.ts
  var isRecord = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
  var text = (value) => typeof value === "string" ? value : "";
  var texts = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  var records = (value) => Array.isArray(value) ? value.filter(isRecord) : [];
  function normalize(term) {
    return term.normalize("NFD").replace(/[\u0300-\u036f]/g, "").normalize("NFC").toLowerCase().trim();
  }
  function parsePlayers(data) {
    return records(data).map((row) => ({ id: text(row.id), name: text(row.name) }));
  }
  function parseCharacters(data) {
    return records(data).map((row) => {
      const ddb = isRecord(row.ddb) ? row.ddb : null;
      return {
        id: text(row.id),
        name: text(row.name),
        aliases: texts(row.aliases),
        kind: row.kind === "pj" ? "pj" : "pnj",
        player: typeof row.player === "string" ? row.player : null,
        initials: text(row.initials),
        type: text(row.type),
        art: text(row.art),
        ddbSyncedAt: ddb && typeof ddb.syncedAt === "string" ? ddb.syncedAt : null,
        hasDdb: ddb !== null
      };
    });
  }
  function parseElements(data) {
    return records(data).map((row) => ({ id: text(row.id), name: text(row.name), aliases: texts(row.aliases), art: text(row.art) }));
  }
  function parseMedia(data) {
    const library = isRecord(data) ? data : {};
    const size = (row) => {
      if (typeof row.size === "string") return row.size;
      return typeof row.width === "number" && typeof row.height === "number" ? `${row.width}\xD7${row.height}` : "";
    };
    return [
      ...records(library.battlemaps).map((row) => ({ id: text(row.id), title: text(row.title), kind: "battlemap", size: size(row), art: "forest", glyph: "\u2B21" })),
      ...records(library.illustrations).map((row) => {
        const title = text(row.title);
        return { id: text(row.id), title, kind: "illustration", size: size(row), art: text(row.art) || "night", glyph: text(row.glyph) || title.charAt(0) };
      })
    ];
  }
  function parseSounds(data) {
    return records(isRecord(data) ? data.sounds : null).map((row) => ({ id: text(row.id), name: text(row.name), tags: texts(row.tags) }));
  }
  function level(type) {
    return /niveau (\d+)/.exec(type)?.[1] ?? "\u2014";
  }
  function synced(character, format) {
    return character.ddbSyncedAt === null ? "\u2014" : `Synchronis\xE9 le ${format(character.ddbSyncedAt)}`;
  }
  function placeGlyph(name) {
    return name.replace(/^(La |Le |L’)/, "").charAt(0);
  }
  var GROUPS = [
    ["pj", "PJ"],
    ["pnj", "PNJ"],
    ["place", "Lieux"],
    ["event", "\xC9v\xE9nements"]
  ];
  function railIndex(characters, places, events) {
    return [
      ...characters.map((c) => ({ id: c.id, group: c.kind, name: c.name, aliases: c.aliases, sub: c.aliases.join(", ") || c.type, art: c.art || "night", glyph: c.initials })),
      ...places.map((p) => ({ id: p.id, group: "place", name: p.name, aliases: p.aliases, sub: p.aliases.join(", "), art: p.art || "night", glyph: placeGlyph(p.name) })),
      ...events.map((v) => ({ id: v.id, group: "event", name: v.name, aliases: v.aliases, sub: v.aliases.join(", "), art: "blood", glyph: "!" }))
    ];
  }
  function distance(a, b) {
    const row = Array.from({ length: b.length + 1 }, (_, i) => i);
    for (let i = 1; i <= a.length; i += 1) {
      let previous = row[0] ?? 0;
      row[0] = i;
      for (let j = 1; j <= b.length; j += 1) {
        const above = row[j] ?? 0;
        row[j] = Math.min(above + 1, (row[j - 1] ?? 0) + 1, previous + (a[i - 1] === b[j - 1] ? 0 : 1));
        previous = above;
      }
    }
    return row[b.length] ?? 0;
  }
  var words = (term) => term.split(/[^\p{L}\p{N}]+/u).filter((word) => word.length > 2);
  function score(entry2, query) {
    const wanted = normalize(query);
    if (wanted === "") return 0;
    const names = [entry2.name, ...entry2.aliases].map(normalize);
    if (names.some((name) => name.includes(wanted))) return 2;
    const known = words(names.join(" "));
    const asked = words(wanted);
    return asked.length > 0 && asked.every((word) => known.some((other) => distance(word, other) <= (word.length > 5 ? 2 : 1))) ? 1 : 0;
  }
  function search(index, query) {
    if (query.trim() === "") return index;
    return index.map((entry2) => ({ entry: entry2, score: score(entry2, query) })).filter((hit) => hit.score > 0).sort((a, b) => b.score - a.score).map((hit) => hit.entry);
  }
  function countText(query, found) {
    const q = query.trim();
    if (q === "") return "";
    return found > 0 ? `${found} r\xE9sultat${found > 1 ? "s" : ""} pour \xAB ${q} \xBB` : `Rien pour \xAB ${q} \xBB`;
  }
  var FOLD_KEY = "CampaignElements:rail";
  function readFolded(memory) {
    try {
      const kept = memory?.getItem(FOLD_KEY);
      const parsed = kept ? JSON.parse(kept) : null;
      return isRecord(parsed) && parsed.collapsed === true;
    } catch {
      return false;
    }
  }
  function writeFolded(memory, folded) {
    try {
      memory?.setItem(FOLD_KEY, JSON.stringify({ collapsed: folded }));
    } catch {
    }
  }

  // apps/campaign/campaign-elements/src/render.ts
  var esc = (value) => value.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  var reads = (...ids) => `data-reads="${ids.join(" ")}"`;
  function art(name, glyph) {
    const tone = /^[a-z]+$/.test(name) ? name : "ember";
    return `<div class="art art--${tone}" role="img">${glyph === "" ? "" : `<span class="art__glyph">${esc(glyph)}</span>`}</div>`;
  }
  var sheetHref = (id) => `fiche-entite.html?id=${encodeURIComponent(id)}`;
  var NEW_KINDS = { pnj: "pnj", place: "lieu", event: "evenement" };
  var NEW_LABELS = [
    ["pnj", "Nouveau PNJ", "secondary"],
    ["place", "Nouveau lieu", "ghost"],
    ["event", "Nouvel \xE9v\xE9nement", "ghost"]
  ];
  var smallButtons = (buttons) => `<div class="kit-button kit-button--bare kit-button--small">${buttons}</div>`;
  var TABS = [
    ["players", "Joueurs", LIST_PLAYERS],
    ["characters", "Personnages", LIST_CHARACTERS],
    ["places", "Lieux", LIST_PLACES],
    ["events", "\xC9v\xE9nements", LIST_EVENTS],
    ["media", "M\xE9dias", LIST_MEDIA],
    ["sounds", "Sons", LIST_SOUNDS]
  ];
  var FIRST_TAB = 1;
  function tabsSkeleton() {
    const newButtons = NEW_LABELS.map(([kind, label, variant]) => `<button type="button" class="${variant}" id="elements-new-${kind}" data-href="fiche-entite.html?new=${NEW_KINDS[kind] ?? ""}">${esc(label)}</button>`).join("");
    const tabs = TABS.map(
      ([key, label], i) => `<button type="button" role="tab" id="elements-tabs-tab-${i}" aria-controls="elements-tabs-panel-${i}" aria-selected="${i === FIRST_TAB}" data-tab="${key}">${esc(label)}</button>`
    ).join("");
    const panels = TABS.map(
      ([key, , read], i) => `<div role="tabpanel" id="elements-tabs-panel-${i}" aria-labelledby="elements-tabs-tab-${i}" data-panel="${key}" ${reads(read)}${i === FIRST_TAB ? "" : " hidden"}><p class="small muted">Chargement\u2026</p></div>`
    ).join("");
    return `<section class="mf-elements" id="campaign-elements" aria-labelledby="campaign-elements-title"><div class="panel__head"><h2 id="campaign-elements-title">\xC9l\xE9ments de la campagne</h2>${smallButtons(newButtons)}</div><div id="elements-tabs-host" ${reads(LIST_PLAYERS, LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS, LIST_MEDIA, LIST_SOUNDS)}><div class="kit-tabs"><div role="tablist" aria-label="\xC9l\xE9ments par type">${tabs}</div>${panels}</div></div></section>`;
  }
  var tabLabel = (label, count) => `${label} (${count})`;
  var unreadable = (what) => `<p class="small muted is-error" role="alert">Impossible de lire ${esc(what)}.</p>`;
  function entry(href, id, thumb, title, sub) {
    return `<li><a class="mf-elements__entry" id="${esc(id)}" href="${esc(href)}"><span class="mf-elements__thumb">${thumb}</span><span><b>${esc(title)}</b><small>${esc(sub)}</small></span></a></li>`;
  }
  var grid = (items) => `<ul class="mf-elements__grid">${items.join("")}</ul>`;
  var when = (at) => {
    const date = new Date(at);
    return Number.isNaN(date.getTime()) ? at : date.toLocaleDateString("fr-FR");
  };
  function playersPanel(players, characters, charactersUnreadable = false) {
    const rows = players.map((player) => {
      const played = characters.filter((c) => c.kind === "pj" && c.player === player.id);
      const cells = [
        esc(player.name),
        played.map((c) => `<a href="${esc(sheetHref(c.id))}">${esc(c.name)}</a>`).join(", "),
        played.map((c) => esc(synced(c, when))).join(", "),
        played.map((c) => esc(level(c.type))).join(", ")
      ];
      return `<tr id="player-${esc(player.id)}">${cells.map((cell, i) => `<td${i === 3 ? ' class="num"' : ""}>${cell}</td>`).join("")}</tr>`;
    }).join("");
    const note = charactersUnreadable ? unreadable("les personnages") : "";
    return `${note}<table class="kit-table" id="elements-players-table"><thead><tr><th>Joueur</th><th>Personnage</th><th>D&amp;D Beyond</th><th class="num">Niveau</th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  var characterEntry = (c) => entry(sheetHref(c.id), `elements-char-${c.id}`, art(c.art || "night", c.initials), c.name, `${c.kind === "pj" ? "PJ" : "PNJ"} \xB7 ${c.aliases.join(", ") || c.type}`);
  function charactersPanel(characters) {
    return `${grid(characters.filter((c) => c.kind === "pj").map(characterEntry))}<h3 class="mf-elements__links">PNJ</h3>${grid(characters.filter((c) => c.kind === "pnj").map(characterEntry))}`;
  }
  var placesPanel = (places) => grid(places.map((p) => entry(sheetHref(p.id), `elements-place-${p.id}`, art(p.art || "night", placeGlyph(p.name)), p.name, p.aliases.join(", "))));
  var eventsPanel = (events) => grid(events.map((v) => entry(sheetHref(v.id), `elements-event-${v.id}`, art("blood", "!"), v.name, v.aliases.join(", "))));
  var link = (label, href) => smallButtons(`<button type="button" class="secondary" data-href="${esc(href)}">${esc(label)}</button>`);
  var mediaPanel = (media) => grid(media.map((m) => entry("medias.html", `elements-media-${m.id}`, art(m.art, m.glyph), m.title, `${m.kind === "battlemap" ? "Battlemap" : "Illustration"} \xB7 ${m.size}`))) + `<div class="mf-elements__links">${link("G\xE9rer les m\xE9dias et les grilles", "medias.html")}</div>`;
  var soundsPanel = (sounds) => grid(sounds.map((s) => `<li class="item" id="elements-sound-${esc(s.id)}"><span>\u266A ${esc(s.name)}</span><small class="muted">${esc(s.tags.join(" \xB7 "))}</small></li>`)) + `<div class="mf-elements__links">${link("Ouvrir la biblioth\xE8que de sons", "soundbox.html")}</div>`;
  function railSkeleton() {
    const newButtons = NEW_LABELS.map(([kind, label, variant]) => `<button type="button" class="${variant}" id="rail-new-${kind}">${esc(label)}</button>`).join("");
    return `<div class="mf-rail" id="elements-rail" data-collapsed="false" data-open="false"><button type="button" class="chip mf-rail__open" id="rail-open" aria-controls="rail-panel" aria-expanded="false">\u2630 \xC9l\xE9ments</button><div class="mf-rail__veil" id="rail-veil" hidden></div><nav class="mf-rail__panel" id="rail-panel" aria-label="\xC9l\xE9ments de la campagne"><div class="mf-rail__bar"><button type="button" class="icon-btn" id="rail-toggle" aria-controls="rail-body" aria-expanded="true" title="Replier la liste">\xAB</button><h2 id="rail-title">\xC9l\xE9ments</h2></div><span class="mf-rail__strip" id="rail-strip" aria-hidden="true">\xC9l\xE9ments</span><div class="mf-rail__body" id="rail-body"><div id="rail-search"><div class="kit-input"><label>Rechercher<input type="search" id="rail-search-input" placeholder="Nom ou alias" autocomplete="off"></label></div></div><p class="small muted" id="rail-count" aria-live="polite"></p><div class="mf-rail__groups" id="rail-groups" ${reads(LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS)}></div><div class="mf-rail__new" id="rail-new">${smallButtons(newButtons)}</div></div></nav></div>`;
  }
  var GROUP_READS = { pj: LIST_CHARACTERS, pnj: LIST_CHARACTERS, place: LIST_PLACES, event: LIST_EVENTS };
  function railEntry(e, current) {
    const here = e.id === current;
    return `<li><a class="mf-rail__entry${here ? " is-current" : ""}" id="rail-entry-${esc(e.id)}" href="${esc(sheetHref(e.id))}" data-entity="${esc(e.id)}"${here ? ' aria-current="true"' : ""}><span class="mf-rail__thumb">${art(e.art, e.glyph)}</span><span class="mf-rail__text"><b>${esc(e.name)}</b><small>${esc(e.sub)}</small></span></a></li>`;
  }
  function railGroups(found, folded, current) {
    return GROUPS.map(([group, label]) => {
      const rows = found.filter((e) => e.group === group);
      if (rows.length === 0) return "";
      const closed = folded.has(group);
      return `<section class="mf-rail__group" id="rail-group-${group}" aria-label="${label}" ${reads(GROUP_READS[group])}><h3><button type="button" class="mf-rail__group-toggle" id="rail-group-toggle-${group}" data-group="${group}" aria-expanded="${!closed}" aria-controls="rail-list-${group}"><span class="mf-rail__caret" aria-hidden="true">${closed ? "\u25B8" : "\u25BE"}</span>${label}<small>${rows.length}</small></button></h3><ul class="mf-rail__list" id="rail-list-${group}"${closed ? " hidden" : ""}>${rows.map((e) => railEntry(e, current)).join("")}</ul></section>`;
    }).join("");
  }

  // apps/campaign/campaign-elements/src/style.ts
  var STYLE = `
.mf-elements, .mf-rail { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-elements *, .mf-elements *::before, .mf-elements *::after, .mf-rail *, .mf-rail *::before, .mf-rail *::after { box-sizing: inherit; }
.mf-elements [hidden], .mf-rail [hidden] { display: none !important; }
.mf-elements h2, .mf-elements h3, .mf-rail h2, .mf-rail h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-elements h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-elements h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-elements p, .mf-rail p { margin: 0; }
.mf-elements .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-elements .muted, .mf-rail .muted { color: var(--color-text-muted); }
.mf-elements .small, .mf-rail .small { font-size: var(--font-size-small); }
.mf-elements .is-error { color: var(--color-danger); }
.mf-elements .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-rail .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-rail .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-rail .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-rail .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-elements .kit-tabs, .mf-elements .kit-table { font-size: var(--font-size-base); }
.mf-elements .kit-table th.num, .mf-elements .kit-table td.num { text-align: right; }
.mf-elements .art, .mf-rail .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-elements .art::before, .mf-rail .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-elements .art::after, .mf-rail .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-elements .art__glyph, .mf-rail .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-elements .art--ember, .mf-rail .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-elements .art--dusk, .mf-rail .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-elements .art--forest, .mf-rail .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-elements .art--gold, .mf-rail .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-elements .art--blood, .mf-rail .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-elements .art--night, .mf-rail .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-elements { display: grid; gap: var(--space-3); }
.mf-elements__grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 6), 1fr)); gap: var(--space-2); }
.mf-elements__entry { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); align-items: center; padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); color: var(--color-text); text-decoration: none; }
.mf-elements__entry:hover { border-color: var(--color-primary); }
.mf-elements__thumb { width: calc(var(--space-5) + var(--space-2)); height: calc(var(--space-5) + var(--space-2)); border-radius: var(--radius-pill); overflow: hidden; }
.mf-elements__entry b { display: block; }
.mf-elements__entry small { display: block; color: var(--color-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-elements__links { margin-top: var(--space-3); }

/* Rail mode: a sticky list beside the sheet, folded to a thin strip, a drawer under 960 px. */
.mf-rail__panel { display: flex; flex-direction: column; gap: var(--space-3); width: calc(var(--space-5) * 7); max-height: calc(100vh - var(--space-5)); padding: var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); }
.mf-rail__bar { display: flex; align-items: center; gap: var(--space-2); }
.mf-rail__bar h2 { font-size: var(--font-size-base); }
.mf-rail__strip { display: none; writing-mode: vertical-rl; color: var(--color-text-muted); font-family: var(--font-family-heading); text-transform: uppercase; letter-spacing: 0.08em; }
.mf-rail__body { display: flex; flex-direction: column; gap: var(--space-2); min-height: 0; flex: 1; }
.mf-rail__body .kit-input { max-width: none; }
.mf-rail__groups { overflow-y: auto; min-height: 0; flex: 1; display: grid; gap: var(--space-3); align-content: start; }
.mf-rail__group h3 { font-size: var(--font-size-small); color: var(--color-text-muted); margin-bottom: var(--space-1); }
.mf-rail__group h3 small { margin-left: var(--space-1); }
.mf-rail__group-toggle { display: flex; align-items: center; gap: var(--space-1); width: 100%; margin: 0; padding: 0; border: 0; background: none; color: inherit; font: inherit; text-align: left; cursor: pointer; }
.mf-rail__group-toggle:hover { color: var(--color-text); }
.mf-rail__caret { display: inline-block; width: var(--space-3); flex-shrink: 0; text-align: center; }
.mf-rail__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); }
.mf-rail__list[hidden] { display: none; }
.mf-rail__entry { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-2); align-items: center; padding: var(--space-1) var(--space-2); border: var(--border-width) solid transparent; border-radius: var(--radius-medium); color: var(--color-text); text-decoration: none; }
.mf-rail__entry:hover { background: var(--color-surface); border-color: var(--color-border); }
.mf-rail__entry.is-current { background: var(--color-surface); border-color: var(--color-primary); }
.mf-rail__thumb { width: var(--space-5); height: var(--space-5); border-radius: var(--radius-pill); overflow: hidden; }
.mf-rail__thumb .art__glyph { font-size: var(--font-size-base); }
.mf-rail__text b { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-rail__text small { display: block; color: var(--color-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-rail__new { flex-shrink: 0; padding-top: var(--space-2); border-top: var(--border-width) solid var(--color-border); }
.mf-rail__new .kit-button { flex-direction: column; align-items: stretch; }
.mf-rail__open, .mf-rail__veil { display: none; }
@media (min-width: 961px) {
  .mf-rail[data-collapsed="true"] .mf-rail__panel { width: calc(var(--space-5) + var(--space-2)); align-items: center; padding: var(--space-2); }
  .mf-rail[data-collapsed="true"] .mf-rail__bar h2, .mf-rail[data-collapsed="true"] .mf-rail__body { display: none; }
  .mf-rail[data-collapsed="true"] .mf-rail__strip { display: block; }
}
@media (max-width: 960px) {
  .mf-rail__open { display: inline-flex; }
  .mf-rail[data-open="true"] .mf-rail__veil { display: block; position: fixed; inset: 0; z-index: 149; background: color-mix(in srgb, var(--color-surface) 70%, transparent); }
  .mf-rail__panel { position: fixed; top: 0; bottom: 0; left: 0; z-index: 150; width: min(85vw, calc(var(--space-5) * 9)); max-height: none; border-radius: 0 var(--radius-large) var(--radius-large) 0; transform: translateX(-105%); visibility: hidden; }
  .mf-rail[data-open="true"] .mf-rail__panel { transform: none; visibility: visible; }
}
`;

  // apps/campaign/campaign-elements/src/index.ts
  var NARROW = "(max-width: 960px)";
  function domBus(doc) {
    const win = doc.defaultView;
    const send = (name, detail) => {
      if (win) doc.dispatchEvent(new win.CustomEvent(`fdm:${name}`, { detail }));
    };
    return {
      open: (request) => send("open-entity", request),
      navigate(href) {
        if (win) win.location.href = href;
      },
      query: () => send("entity-query", {}),
      onOpened(listener) {
        const handler = (event) => listener(event.detail ?? {});
        doc.addEventListener("fdm:entity-opened", handler);
        return () => doc.removeEventListener("fdm:entity-opened", handler);
      }
    };
  }
  function ensureStyle(doc) {
    if (doc.querySelector('style[data-micro-ui="CampaignElements"]')) return;
    const style = doc.createElement("style");
    style.dataset.microUi = "CampaignElements";
    style.textContent = STYLE;
    doc.head.append(style);
  }
  function memoryOf(doc) {
    try {
      return doc.defaultView?.localStorage ?? null;
    } catch {
      return null;
    }
  }
  function mount(host2, props2, shell2, bus = domBus(host2.ownerDocument)) {
    const doc = host2.ownerDocument;
    const win = doc.defaultView;
    const memory = memoryOf(doc);
    ensureStyle(doc);
    let current = props2;
    const modeOf = (p) => p.mode === "rail" ? "rail" : "tabs";
    const campaignOf = (p) => (p.campaign ?? "").trim() || (new URLSearchParams(win?.location.search ?? "").get("campaign") ?? "").trim();
    const publish = (p) => {
      const campaign = campaignOf(p);
      if (campaign !== "") shell2.context?.set("campaign", campaign);
    };
    publish(current);
    let players = [];
    let characters = [];
    let places = [];
    let events = [];
    let media = [];
    let sounds = [];
    const status = /* @__PURE__ */ new Map();
    let generation = 0;
    let query = "";
    let shown2 = new URLSearchParams(win?.location.search ?? "").get("id") ?? "";
    const folded = /* @__PURE__ */ new Set();
    const mobile = typeof win?.matchMedia === "function" ? win.matchMedia(NARROW) : null;
    const narrow = () => mobile?.matches === true;
    const drawn = /* @__PURE__ */ new WeakMap();
    const put = (el, html) => {
      if (drawn.get(el) === html && el.isConnected) return;
      drawn.set(el, html);
      el.innerHTML = html;
    };
    const part = (id) => {
      const found = host2.querySelector(`#${id}`);
      if (!found) throw new Error(`CampaignElements: #${id} is not in the panel`);
      return found;
    };
    const sections = {
      players: { own: LIST_PLAYERS, also: [LIST_CHARACTERS], html: () => playersPanel(players, characters, status.get(LIST_CHARACTERS) === "failed"), count: () => players.length },
      characters: { own: LIST_CHARACTERS, also: [], html: () => charactersPanel(characters), count: () => characters.length },
      places: { own: LIST_PLACES, also: [], html: () => placesPanel(places), count: () => places.length },
      events: { own: LIST_EVENTS, also: [], html: () => eventsPanel(events), count: () => events.length },
      media: { own: LIST_MEDIA, also: [], html: () => mediaPanel(media), count: () => media.length },
      sounds: { own: LIST_SOUNDS, also: [], html: () => soundsPanel(sounds), count: () => sounds.length }
    };
    function paintTabs() {
      TABS.forEach(([key, label], i) => {
        const section = sections[key];
        if (!section) return;
        const waiting = [section.own, ...section.also].some((id) => (status.get(id) ?? "pending") === "pending");
        const failed = status.get(section.own) === "failed";
        const ready = !waiting && !failed;
        put(part(`elements-tabs-panel-${i}`), failed ? unreadable("cette liste") : ready ? section.html() : '<p class="small muted">Chargement\u2026</p>');
        part(`elements-tabs-tab-${i}`).textContent = ready ? tabLabel(label, section.count()) : label;
      });
    }
    function selectTab(index2) {
      TABS.forEach((_, i) => {
        part(`elements-tabs-tab-${i}`).setAttribute("aria-selected", String(i === index2));
        part(`elements-tabs-panel-${i}`).hidden = i !== index2;
      });
    }
    let index = [];
    function paintRail() {
      const found = search(index, query);
      const failed = NEEDED.rail.some((id) => status.get(id) === "failed");
      put(part("rail-groups"), (failed ? unreadable("une partie des \xE9l\xE9ments") : "") + railGroups(found, folded, shown2));
      part("rail-count").textContent = countText(query, found.length);
    }
    function syncRail() {
      const rail = part("elements-rail");
      const open = rail.dataset.open === "true";
      const strip = rail.dataset.collapsed === "true";
      const toggle = part("rail-toggle");
      toggle.textContent = narrow() ? "\u2715" : strip ? "\xBB" : "\xAB";
      toggle.title = narrow() ? "Fermer la liste" : strip ? "D\xE9plier la liste" : "Replier la liste";
      toggle.setAttribute("aria-expanded", String(narrow() ? open : !strip));
      part("rail-open").setAttribute("aria-expanded", String(open));
      part("rail-veil").hidden = !open;
    }
    function setOpen(open) {
      part("elements-rail").dataset.open = String(open);
      syncRail();
    }
    function setFolded(strip) {
      part("elements-rail").dataset.collapsed = String(strip);
      writeFolded(memory, strip);
      syncRail();
    }
    function toggleGroup(button) {
      const group = button.dataset.group;
      if (folded.has(group)) folded.delete(group);
      else folded.add(group);
      const closed = folded.has(group);
      button.setAttribute("aria-expanded", String(!closed));
      const caret = button.querySelector(".mf-rail__caret");
      if (caret) caret.textContent = closed ? "\u25B8" : "\u25BE";
      part(`rail-list-${group}`).hidden = closed;
    }
    function follow(opened) {
      shown2 = opened.id ?? "";
      for (const link2 of host2.querySelectorAll(".mf-rail__entry")) {
        const on = link2.dataset.entity === shown2;
        link2.classList.toggle("is-current", on);
        if (on) link2.setAttribute("aria-current", "true");
        else link2.removeAttribute("aria-current");
      }
    }
    const readers = {
      [LIST_PLAYERS]: (data) => players = parsePlayers(data),
      [LIST_CHARACTERS]: (data) => characters = parseCharacters(data),
      [LIST_PLACES]: (data) => places = parseElements(data),
      [LIST_EVENTS]: (data) => events = parseElements(data),
      [LIST_MEDIA]: (data) => media = parseMedia(data),
      [LIST_SOUNDS]: (data) => sounds = parseSounds(data)
    };
    const NEEDED = {
      tabs: [LIST_PLAYERS, LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS, LIST_MEDIA, LIST_SOUNDS],
      rail: [LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS]
    };
    function paint() {
      if (modeOf(current) === "tabs") return paintTabs();
      index = railIndex(characters, places, events);
      paintRail();
    }
    async function load() {
      const mine = generation += 1;
      const variables = { campaign: campaignOf(current) };
      await Promise.all(
        NEEDED[modeOf(current)].map(async (id) => {
          try {
            const answer = await shell2.capability(id, void 0, variables);
            if (mine !== generation) return;
            readers[id]?.(answer.data);
            status.set(id, "ok");
          } catch {
            if (mine !== generation) return;
            status.set(id, "failed");
          }
          paint();
        })
      );
    }
    function frame() {
      status.clear();
      query = "";
      host2.innerHTML = modeOf(current) === "rail" ? railSkeleton() : tabsSkeleton();
      if (modeOf(current) === "tabs") return;
      part("elements-rail").dataset.collapsed = String(readFolded(memory));
      syncRail();
    }
    host2.addEventListener("input", (event) => {
      const input = event.target;
      if (input?.id !== "rail-search-input") return;
      query = input.value;
      paintRail();
    });
    host2.addEventListener("click", (event) => {
      const target = event.target;
      if (!target) return;
      const link2 = target.closest("a[data-entity]");
      if (link2) {
        const mouse = event;
        if (mouse.metaKey || mouse.ctrlKey || mouse.shiftKey || mouse.button !== 0) return;
        event.preventDefault();
        bus.open({ id: link2.dataset.entity });
        setOpen(false);
        return;
      }
      if (target.id === "rail-veil") return setOpen(false);
      const tab = target.closest('[role="tab"]');
      if (tab) return selectTab(TABS.findIndex(([key]) => key === tab.dataset.tab));
      const button = target.closest("button");
      if (!button) return;
      if (button.dataset.href) return bus.navigate(button.dataset.href);
      if (button.dataset.group) return toggleGroup(button);
      if (button.id === "rail-toggle") return narrow() ? setOpen(false) : setFolded(part("elements-rail").dataset.collapsed !== "true");
      if (button.id === "rail-open") return setOpen(true);
      const kind = /^rail-new-(.+)$/.exec(button.id)?.[1];
      const fresh = kind === void 0 ? void 0 : NEW_KINDS[kind];
      if (fresh !== void 0) {
        bus.open({ new: fresh });
        setOpen(false);
      }
    });
    host2.addEventListener("keydown", (event) => {
      const rail = host2.querySelector("#elements-rail");
      if (event.key === "Escape" && rail?.dataset.open === "true") {
        setOpen(false);
        part("rail-open").focus();
      }
    });
    const close = () => {
      if (host2.querySelector("#elements-rail")) setOpen(false);
    };
    mobile?.addEventListener("change", close);
    const poll = {};
    const stopOpened = bus.onOpened((opened) => {
      if (host2.isConnected) follow(opened);
      else release();
    });
    function release() {
      poll.stop?.();
      stopOpened();
      mobile?.removeEventListener("change", close);
    }
    let first = true;
    poll.stop = shell2.onDataVersion?.(() => {
      if (!host2.isConnected) return release();
      if (first) first = false;
      else void load();
    });
    const start = () => {
      void load().then(() => {
        if (modeOf(current) === "rail") bus.query();
      });
    };
    frame();
    start();
    return {
      update(next) {
        const before = current;
        current = next;
        publish(next);
        const another = campaignOf(before) !== campaignOf(next);
        const otherMode = modeOf(before) !== modeOf(next);
        if (another) [players, characters, places, events, media, sounds] = [[], [], [], [], [], []];
        if (another || otherMode) frame();
        start();
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
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
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
        for (const problem of index.problems || []) console.error(`mock server: ${problem}`);
      }
      return index && index.manifests || {};
    };
    s.undeclared = [];
    s.planned = async (id) => {
      const graph = await s.product("plan/graph.json");
      const entry2 = graph && graph.ids && graph.ids[id];
      return Boolean(entry2 && entry2.kind === "data-capability" && entry2.producer != null);
    };
    s.tell = (message, to, commandId, extra = {}) => {
      const m = { message, to: Array.isArray(to) ? to : [to], commandId, at: (/* @__PURE__ */ new Date()).toISOString(), ...extra };
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
    const pendingIn = (partition) => [...s.queue.values()].filter((e) => e.partition === partition && !FINAL.has(e.state)).sort((a, b) => a.position - b.position);
    const declaredAhead = (entry2, merging = /* @__PURE__ */ new Set()) => {
      const fields = Object.keys(entry2.payload).filter((f) => !merging.has(f));
      const ahead = pendingIn(entry2.partition).find((e) => e.position < entry2.position && e.state !== "parked" && fields.some((f) => f in e.payload));
      if (ahead) {
        const f = fields.find((x) => x in ahead.payload);
        return { by: ahead.by, command: ahead.commandId, value: ahead.payload[f], field: f };
      }
      const read2 = entry2.basedOn && entry2.basedOn.values || {};
      for (const f of fields) {
        if (f in read2 && s.confirmed.has(`${entry2.partition}.${f}`) && !same(s.confirmed.get(`${entry2.partition}.${f}`), read2[f])) {
          return { by: "(applied)", command: "(applied)", value: s.confirmed.get(`${entry2.partition}.${f}`), field: f };
        }
      }
      return null;
    };
    s.confirmed = /* @__PURE__ */ new Map();
    const resultOf = (entry2) => ({ commandId: entry2.commandId, status: entry2.state, dataVersion: entry2.dataVersion || null, violations: entry2.violations || [], reviewId: entry2.reviewId || null });
    const answer = (entry2, replayed = false) => json({ commandId: entry2.commandId, partition: entry2.partition, replayed, warnings: replayed ? [] : entry2.warnings || [], result: resultOf(entry2) }, 202);
    const drive = async () => {
      for (const partition of new Set([...s.queue.values()].filter((e) => !FINAL.has(e.state)).map((e) => e.partition))) {
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
      const commandId = uuid();
      const partition = `${aggregate || dataCapability}/${targetId || commandId}`;
      const entry2 = { commandId, dataCapability, version: Number(version) || 1, aggregate, partition, targetId, by, payload, basedOn: body.basedOn, idempotencyKey: body.idempotencyKey, handler, position: s.next(), state: "queued", warnings: [] };
      s.queue.set(commandId, entry2);
      s.commands.set(commandId, entry2);
      const known = (await s.manifests())[dataCapability];
      const keys = known ? undeclaredKeys(known.manifest, payload) : [];
      const warnUndeclared = () => {
        if (!keys.length) return;
        const warning = undeclaredWarning(keys, dataCapability);
        entry2.warnings.push(warning);
        s.undeclared.push({ commandId, dataCapability, keys, manifest: known.path, warning });
        console.error(`mock server: ${warning} (${known.path}) \u2014 the real server accepts it with this warning and the journeys fail on it: send only what the manifest's payload declares`);
      };
      if (!handler && await s.planned(dataCapability)) {
        entry2.state = "applied";
        entry2.warnings.push(`mocked-data-capability: ${dataCapability} is planned, not built`);
        warnUndeclared();
        s.tell("CommandApplied", by, commandId, { mocked: true });
        s.bump();
        return answer(entry2);
      }
      if (!handler) {
        warnUndeclared();
        entry2.state = "rejected";
        entry2.violations = ["unhandled-data-capability"];
        s.tell("CommandRejected", by, commandId, { violations: entry2.violations, reason: `${dataCapability} has no handler: write mock/handlers/${dataCapability}.js` });
        return answer(entry2);
      }
      const verdict = await invariants(aggregate, payload);
      if (verdict.reject.length || verdict.review.length) {
        entry2.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId, { violations: verdict.reject.concat(verdict.review) });
      }
      warnUndeclared();
      const needsReview = verdict.review.length > 0 || Object.keys(payload).some((f) => declared[f] && declared[f].criticality === "critical");
      const merging = new Set(Object.keys(declared).filter((f) => declared[f].merge));
      const ahead = declaredAhead(entry2, merging);
      if (needsReview) {
        entry2.state = "awaiting_review";
        entry2.reviewId = `r_${s.next()}`;
        entry2.warnings.push(`review ${entry2.reviewId} required`);
        s.tell("ReviewRequired", "role:approver", commandId, { actions: ["approve", "reject"] });
      } else if (ahead && (body.mode || "confirm_on_stale") === "confirm_on_stale") {
        entry2.state = "awaiting_confirmation";
        entry2.aheadOf = ahead.command;
        entry2.warnings.push("a value is declared ahead: confirm or cancel");
        s.tell("ValueDeclaredAhead", by, commandId, { field: `${partition}.${ahead.field}`, yourValue: payload[ahead.field], declaredAhead: { value: ahead.value, by: ahead.by, command: ahead.command }, actions: ["confirm_overwrite", "cancel", "edit"] });
      }
      s.tell("CommandQueued", by, commandId);
      await drive();
      return answer(entry2);
    };
    const gesture = async (id, verb, body) => {
      const entry2 = s.queue.get(id);
      if (!entry2) return notFound();
      const by = body && body.by || entry2.by;
      if (by !== entry2.by) return refused(`${id} is ${entry2.by}'s to ${verb}`);
      if (verb === "confirm") {
        if (entry2.state !== "awaiting_confirmation") return refused(`${id} is ${entry2.state}, not awaiting a confirmation`);
        entry2.state = "confirmed";
      } else if (verb === "cancel") {
        if (FINAL.has(entry2.state)) return refused(`${id} is ${entry2.state}`);
        entry2.state = "cancelled";
        s.tell("CommandCancelled", entry2.by, id);
      } else if (verb === "requeue") {
        if (entry2.state !== "parked") return refused(`${id} is ${entry2.state}, not parked`);
        entry2.state = "cancelled";
        const fresh = { ...entry2, commandId: `${id}-r`, position: s.next(), state: "queued", warnings: [], requeuedFrom: id, aheadOf: void 0, atHeadSince: void 0 };
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
        if (entry2.state !== "awaiting_review") return refused(`${id} is ${entry2.state}, not awaiting a review`);
        entry2.state = verb === "approve" ? "confirmed" : "rejected";
        if (verb === "reject") entry2.violations = [`review ${entry2.reviewId} rejected`];
      } else return notFound();
      await drive();
      return json({ result: resultOf(entry2) });
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
        const entry2 = s.commands.get(decodeURIComponent(lookup[1]));
        return entry2 ? json({ result: resultOf(entry2) }) : notFound();
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

  // mock/src/CampaignElements.js
  var { host, props } = mountPoint();
  var shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
  compose(props, (resolved) => mount(host, resolved, shell));
})();
