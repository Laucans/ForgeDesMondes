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
    let players = [];
    let characters = [];
    let places = [];
    let events = [];
    let media = [];
    let sounds = [];
    const status = /* @__PURE__ */ new Map();
    let generation = 0;
    let query = "";
    let shown = new URLSearchParams(win?.location.search ?? "").get("id") ?? "";
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
      put(part("rail-groups"), (failed ? unreadable("une partie des \xE9l\xE9ments") : "") + railGroups(found, folded, shown));
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
      shown = opened.id ?? "";
      for (const link2 of host2.querySelectorAll(".mf-rail__entry")) {
        const on = link2.dataset.entity === shown;
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
    // soundbox.addSound (crates/soundbox/data-capabilities/add-sound): a sound
    // added to the library. One with no name or no file is refused before
    // anything is queued, as the server refuses it. Its tags, a list of texts,
    // are normalised (`tags-normalized`); its own volume is a percentage from
    // 0 to 100 — a number or the text of one — full when none is given
    // (`sound-volume-range`).
    "soundbox.addSound": async (server, { payload }) => {
      const name = typeof payload.name === "string" ? payload.name.trim() : "";
      const file = typeof payload.file === "string" ? payload.file.trim() : "";
      if (!name || !file) return ["unprocessable"];
      const tags = payload.tags ?? [];
      if (!Array.isArray(tags) || !tags.every((t) => typeof t === "string")) return ["tags-normalized"];
      const given = payload.volume ?? 100;
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
    "session.createSession": async (server, { payload }) => {
      const sessions = await server.load("session.listSessions");
      const characters = await server.load("campaign.listCharacters");
      const campaign = (await server.load("campaign.listCampaigns") || []).find((c) => c.id === payload.campaign);
      const present = Array.isArray(payload.present) ? payload.present : [];
      const pcs = new Set(characters.filter((c) => c.kind === "pj").map((c) => c.id));
      if (present.some((id) => !pcs.has(id))) return ["present-only-pj"];
      if (campaign && campaign.type === "oneshot" && (campaign.sessions ?? 0) > 0) return ["oneshot-single-session"];
      if (campaign) campaign.sessions = (campaign.sessions ?? 0) + 1;
      sessions.push({
        id: server.uuid(),
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
    "session.addPresentCharacter": async (server, { payload, targetId }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
      if (!session) return ["not-found"];
      const characters = await server.load("campaign.listCharacters");
      if (!characters.some((c) => c.kind === "pj" && c.id === payload.character)) return ["present-only-pj"];
      if (!session.present.includes(payload.character)) session.present.push(payload.character);
      return void 0;
    },
    // session.reorderScenes (crates/session/data-capabilities/reorder-scenes):
    // the session's scenes in the order the DM gives, every rank rewritten at
    // once, 1 to n. An order that omits a scene of the fixtures' session,
    // repeats one or names one it does not hold is `scene-order-contiguous`,
    // and no rank changes.
    "session.reorderScenes": async (server, { payload }) => {
      const scenes = await server.load("session.listScenes");
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
    "session.removePresentCharacter": async (server, { payload, targetId }) => {
      const session = (await server.load("session.listSessions")).find((s) => s.id === (targetId || payload.session));
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
    "media.uploadMedia": async (server, { payload }) => {
      const { kind, place = null } = payload;
      if (kind !== "battlemap" && kind !== "illustration") return ["media-kind"];
      if (place && !(await server.load("campaign.listPlaces") || []).some((p) => p.id === place)) return ["media-place-is-lieu"];
      const base = { id: server.uuid(), title: String(payload.title || "").trim() || "Sans titre", file: payload.file || "", uploadedAt: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10), place };
      for (const name of ["media", "media.listMedia"]) {
        const media = await server.load(name);
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
    // session.addScene (crates/session/data-capabilities/add-scene): a new
    // scene, ranked after the session's last (`scene-order-contiguous`), with
    // its title, its notes and the places it links — each a place of the
    // fixtures' campaign, else `scene-links-same-campaign` — whose media
    // follow it to the session (`session-media-by-place`, derived, never
    // stored). Its notes are kept as written. A blank title is
    // `scene-title-required`: the mock answers violation ids, where the
    // server answers 422.
    "session.addScene": async (server, { payload }) => {
      const title = String(payload.title || "").trim();
      if (!title) return ["scene-title-required"];
      const known = new Set((await server.load("campaign.listPlaces")).map((p) => p.id));
      const places = [...new Set(Array.isArray(payload.places) ? payload.places : [])];
      if (places.some((id) => !known.has(id))) return ["scene-links-same-campaign"];
      const library = await server.load("media.listMedia");
      const media = library.battlemaps.concat(library.illustrations).filter((m) => places.includes(m.place)).map((m) => m.id);
      const scenes = await server.load("session.listScenes");
      scenes.push({
        id: server.uuid(),
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
    "session.addPlaceToSession": async (server, { payload }) => {
      const place = payload.place;
      const scenes = await server.load("session.listScenes") || [];
      const first = scenes.reduce((low, sc) => !low || sc.order < low.order ? sc : low, null);
      const scene = payload.scene ? scenes.find((sc) => sc.id === payload.scene) : first;
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
    "table.createBattlemapInstance": async (server, { payload }) => {
      const session = await server.load("session.getSession");
      const scene = (await server.load("session.listScenes")).find((sc) => sc.id === payload.scene);
      const map = (await server.load("media.listMedia")).battlemaps.find((m) => m.id === payload.battlemap);
      if (!session || session.id !== payload.session || !scene || !map) return ["not-found"];
      if (scene.battlemap) return ["one-instance-per-scene"];
      const id = `bmi-${map.id}-${scene.id}`;
      server.instances = server.instances || /* @__PURE__ */ new Map();
      server.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
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
    "table.placeTokens": async (server, { payload, targetId }) => {
      const count = payload.count ?? 1;
      if (!Number.isInteger(payload.q) || !Number.isInteger(payload.r)) return ["single-position-source"];
      if (!Number.isInteger(count) || count < 1 || count > 12) return ["multi-drop-numbering"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instance = id === fixture.id ? fixture : server.instances && server.instances.get(id);
      const character = (await server.load("campaign.listCharacters")).find((c) => c.id === payload.character);
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
          q: payload.q,
          r: payload.r,
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
    "table.renameToken": async (server, { payload, targetId }) => {
      const same2 = (a, b) => String(a ?? "").toLowerCase() === String(b ?? "").toLowerCase();
      const name = typeof payload.name === "string" ? payload.name.trim() : "";
      if (!name) return ["multi-drop-numbering"];
      const tokenId = payload.token ?? targetId;
      const instance = payload.token != null && targetId && !same2(targetId, tokenId) ? targetId : null;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const inst = instances.find((i) => (instance == null || same2(i.id, instance)) && i.state.tokens.some((t) => same2(t.id, tokenId)));
      if (!inst) return ["not-found"];
      inst.state.tokens.find((t) => same2(t.id, tokenId)).name = name;
      return void 0;
    },
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
    // soundbox.updateSound (crates/soundbox/data-capabilities/update-sound): the
    // name, tags and volume of one sound, tags normalised and volume ranged as
    // for an addition; a sound that is not there is `not-found`.
    "soundbox.updateSound": async (server, { payload, targetId }) => {
      const sound = (await server.load("soundbox.listSounds")).sounds.find((s) => s.id === targetId);
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
    "soundbox.setMasterVolume": async (server, { payload, targetId }) => {
      if (!String(targetId || payload.campaign || "").trim()) return ["not-found"];
      const { volume } = payload;
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
    "soundbox.setSoundVolume": async (server, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const fixtures = (await Promise.all(["soundbox.listSounds", "sounds"].map((name) => server.load(name)))).filter(Boolean);
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
    "campaign.createCampaign": async (server, { payload }) => {
      const name = String(payload.name ?? "").trim();
      if (!name) return ["campaign-name-required"];
      if (!["campagne", "oneshot"].includes(payload.type)) return ["campaign-type-unknown"];
      const text2 = (field) => typeof payload[field] === "string" ? payload[field] : null;
      (await server.load("campaign.listCampaigns")).unshift({
        id: server.uuid(),
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
      const entry2 = history[id] || (history[id] = { name: after.name, versions: [] });
      entry2.name = after.name;
      const changes = [.../* @__PURE__ */ new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => f !== "id").sort().filter((f) => JSON.stringify(before[f] ?? null) !== JSON.stringify(after[f] ?? null)).map((field) => ({ field, old: before[field] ?? null, new: after[field] ?? null }));
      const v = Math.max(0, ...entry2.versions.map((version) => version.v)) + 1;
      entry2.versions.unshift({ v, date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 19), origin: "dm", source: "Fiche enregistr\xE9e par le DM", seen: true, changes });
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
    "table.moveToken": async (server, { payload, targetId }) => {
      const axis = (n) => Number.isInteger(n) && n >= -(2 ** 31) && n < 2 ** 31;
      if (!axis(payload.q) || !axis(payload.r)) return ["single-position-source"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => t.id === payload.token));
      if (!holder) return ["not-found"];
      if (holder.id !== id) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => t.id === payload.token);
      token.q = payload.q;
      token.r = payload.r;
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
    "table.setTokenVisibility": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      if (typeof payload.visible !== "boolean") return ["players-see-band-only"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      holder.state.tokens.find((t) => same2(t.id, payload.token)).visible = payload.visible;
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
    "table.setTokenCondition": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const state = typeof payload.state === "string" ? payload.state.trim() : "";
      if (!state || state.includes("\0") || typeof payload.on !== "boolean") return ["state-persisted-reset"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => same2(t.id, payload.token));
      const held = token.states || [];
      token.states = payload.on ? held.includes(state) ? held : [...held, state] : held.filter((s) => s !== state);
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
    "table.changeHitPoints": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      if (!Number.isInteger(payload.delta)) return ["pj-hp-not-tracked"];
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const holder = instances.find((i) => i.state.tokens.some((t) => same2(t.id, payload.token)));
      if (!holder) return ["not-found"];
      if (!same2(id, holder.id)) return ["instance-isolated"];
      const token = holder.state.tokens.find((t) => same2(t.id, payload.token));
      if (!Number.isInteger(token.hp) || !Number.isInteger(token.hpMax)) return ["pj-hp-not-tracked"];
      token.hp = Math.max(0, Math.min(token.hpMax, token.hp + payload.delta));
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
    "table.removeToken": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const named = payload.token != null;
      const tokenId = named ? payload.token : targetId;
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
    "table.nextTurn": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload?.instance;
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
    "table.startCombat": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      if (instance.state.combat) return ["one-active-combat"];
      const tokens = instance.state.tokens;
      if (!tokens.length) return ["npc-initiative-bounds"];
      const figure = (figures, key) => Object.entries(figures || {}).find(([k]) => same2(k, key))?.[1];
      if (Object.keys(payload.rolls || {}).some((k) => !tokens.some((t) => same2(t.id, k) && t.kind === "pnj"))) return ["npc-initiative-bounds"];
      const characters = await server.load("campaign.listCharacters");
      const seated = [];
      for (const t of tokens) {
        if (t.kind !== "pj" && t.kind !== "pnj") return ["npc-initiative-bounds"];
        const score2 = characters.find((c) => same2(c.id, t.char))?.stats?.DEX;
        const pc = t.kind === "pj";
        const dexScore = pc ? score2 ?? null : score2 ?? 10;
        const roll = pc ? null : figure(payload.rolls, t.id) ?? 1 + Math.floor(Math.random() * 20);
        const init = pc ? figure(payload.initiative, t.char) : roll + Math.floor((dexScore - 10) / 2);
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
    "table.addParticipant": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      const token = instance?.state.tokens.find((t) => same2(t.id, payload.token));
      if (!token) return ["not-found"];
      const combat = instance.state.combat;
      if (!combat) return ["initiative-tiebreak"];
      const order = combat.order || [];
      if (order.some((o) => same2(o.id, token.id))) return ["initiative-tiebreak"];
      if (token.kind !== "pj" && token.kind !== "pnj") return ["npc-initiative-bounds"];
      const pc = token.kind === "pj";
      if (pc ? !Number.isInteger(payload.initiative) : payload.initiative != null) return ["npc-initiative-bounds"];
      const characters = await server.load("campaign.listCharacters");
      const score2 = characters.find((c) => same2(c.id, token.char))?.stats?.DEX;
      const dexScore = pc ? score2 ?? null : score2 ?? 10;
      const dex = dexScore === null ? 0 : Math.floor((dexScore - 10) / 2);
      const init = pc ? payload.initiative : 1 + Math.floor(Math.random() * 20) + dex;
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
    "table.reorderInitiative": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload.instance;
      const fixture = await server.load("table.getBattlemapInstance");
      const instances = [fixture, ...server.instances ? server.instances.values() : []];
      const instance = instances.find((i) => same2(i.id, id));
      if (!instance) return ["not-found"];
      const combat = instance.state.combat;
      const held = combat && combat.order || [];
      const named = Array.isArray(payload.order) ? payload.order : [];
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
    "table.removeParticipant": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const named = payload?.participant != null;
      const participant = named ? payload.participant : targetId;
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
    "table.endCombat": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload?.instance;
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
    "table.resetBattlemap": async (server, { payload, targetId }) => {
      const same2 = (a, b) => a != null && String(a).trim().toLowerCase() === String(b ?? "").trim().toLowerCase();
      const id = targetId || payload?.instance;
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
    "soundbox.playSound": async (server, { payload, targetId }) => {
      const id = targetId || payload?.sound;
      const sound = (await server.load("soundbox.listSounds")).sounds.find((candidate) => candidate.id === id);
      if (!sound) return ["not-found"];
      const source = payload?.source ?? "soundbox";
      if (!["soundbox", "suggestion"].includes(source)) return ["dm-side-only"];
      sound.playing = true;
      return void 0;
    }
  };

  // mock/src/reads.js
  var STATUS_LABELS = { prep: "En pr\xE9paration", live: "En direct", closed: "Jou\xE9e" };
  var READS = {
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
    "soundbox.listSounds": async (server, { search: search2, tag }) => {
      const { masterVolume, sounds } = await server.load("soundbox.listSounds");
      const lower = (text2) => String(text2 ?? "").trim().toLowerCase();
      return {
        masterVolume,
        sounds: sounds.filter((s) => lower(s.name).includes(lower(search2)) && (!lower(tag) || s.tags.some((t) => lower(t) === lower(tag)))).map((s) => ({ ...s, gain: s.volume * masterVolume / 1e4, playing: s.playing === true }))
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
      const { opacity, ...grid2 } = battlemap.grid;
      return { ...battlemap, grid: { ...grid2, opacity: opacity / 100, feetPerHex: 5 } };
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
  var FINAL = /* @__PURE__ */ new Set(["applied", "rejected", "cancelled", "expired"]);
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
      const entry2 = graph && graph.ids && graph.ids[id];
      return Boolean(entry2 && entry2.kind === "data-capability" && entry2.producer != null);
    };
    s.tell = (message, to, commandId, extra = {}) => {
      const m = { message, to: Array.isArray(to) ? to : [to], commandId, at: (/* @__PURE__ */ new Date()).toISOString(), ...extra };
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
    const answer = (entry2, replayed = false) => json({ commandId: entry2.commandId, partition: entry2.partition, replayed, warnings: entry2.warnings || [], result: resultOf(entry2) }, 202);
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
      const handler = COMMANDS[dataCapability];
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
      if (!handler && await s.planned(dataCapability)) {
        entry2.state = "applied";
        entry2.warnings.push(`mocked-data-capability: ${dataCapability} is planned, not built`);
        s.tell("CommandApplied", by, commandId, { mocked: true });
        s.bump();
        return answer(entry2);
      }
      if (!handler) {
        entry2.state = "rejected";
        entry2.violations = ["unhandled-data-capability"];
        s.tell("CommandRejected", by, commandId, { violations: entry2.violations, reason: `${dataCapability} is not in mock/src/commands.js` });
        return answer(entry2);
      }
      const verdict = await invariants(aggregate, payload);
      if (verdict.reject.length || verdict.review.length) {
        entry2.warnings.push(`invariant(s) at risk: ${verdict.reject.concat(verdict.review).join(", ")}`);
        s.tell("InvariantAtRisk", by, commandId, { violations: verdict.reject.concat(verdict.review) });
      }
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
