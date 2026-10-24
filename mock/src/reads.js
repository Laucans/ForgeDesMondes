// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

const bridged = new WeakSet();

/**
 * The battlemap instance of `fixtures/table.getBattlemapInstance.json`, shared
 * by the tabs of this machine (the DM view, the player view) through the
 * mock-up's store: what one tab changes the other reads, as two views of the
 * one server would. Another id is an instance a command made
 * (table.createBattlemapInstance), kept in this tab; undefined when none has it.
 */
export async function instanceOf(server, id) {
  const data = await server.load('table.getBattlemapInstance');
  if (!data) return undefined;
  if (id && data.id !== id) return server.instances ? server.instances.get(id) : undefined;
  const store = window.Mock && window.Mock.store;
  if (store && !bridged.has(data)) {
    bridged.add(data);
    data.state = store.get(`map:${data.id}`, data.state);
    store.on(`map:${data.id}`, (state) => {
      // Our own write comes back as the same object; another tab's is new.
      if (state && state !== data.state) {
        data.state = state;
        server.bump();
      }
    });
  }
  return data;
}

/** The instance's state written for the other tabs. */
export function shareInstance(data) {
  if (window.Mock) window.Mock.store.set(`map:${data.id}`, data.state);
}

// HealthBand@1: told from the current and the maximum hit points and nothing else.
function healthBand(hp, hpMax) {
  if (typeof hp !== 'number' || typeof hpMax !== 'number') return null;
  const current = Math.max(0, Math.min(hpMax, hp));
  return current === 0 ? 'mort' : current === hpMax ? 'indemne' : 2 * current > hpMax ? 'blessé' : 'en sang';
}

// What a token carries besides its place: the token of its character (colour, initials, portrait), as the Capability gives it.
async function dressed(server, tokens) {
  const characters = (await server.load('campaign.listCharacters')) || [];
  return tokens.map((t) => {
    const c = characters.find((x) => x.id === t.char);
    return c ? { ...t, initials: c.initials, color: c.color, portrait: c.portrait } : t;
  });
}

let diffusionBridged = false;

/**
 * The diffusion row of `fixtures/live.getDiffusion.json`, shared by the tabs of
 * this machine (the DM view, the player view, the piloted tab) through the
 * mock-up's store: what one tab sends or drags the others read, as two views of
 * the one row would. Kept in the mock-up's own shape: the window `{ id, x, y, w }`,
 * the frame `{ z, px, py }`.
 */
export async function diffusionRow(server) {
  const fixture = await server.load('live.getDiffusion');
  const store = window.Mock && window.Mock.store;
  if (store && !diffusionBridged) {
    diffusionBridged = true;
    store.on('diffusion', () => server.bump());
  }
  return store ? store.get('diffusion', fixture) : structuredClone(fixture);
}

/** Width over height of an illustration of the library (media.listMedia); a square when it is unknown. */
export async function ratioOfIllustration(server, id) {
  const media = await server.load('media.listMedia');
  const found = media && media.illustrations.find((i) => i.id === id);
  return found && found.ratio > 0 ? found.ratio : 1;
}

// SessionLifecycle@1: the label of each status, and the mock-up's fixtures'
// « played » for what the Concept calls `closed`.
const STATUS_LABELS = { prep: 'En préparation', live: 'En direct', closed: 'Jouée' };

export const READS = {
  // live.getDiffusion (crates/live/capabilities/get-diffusion): the one row both
  // player views read, so view 'dm' and view 'player' answer the same values —
  // position, size, zoom and pan. The window's x and w are fractions of the stage's
  // width, y of its height; h, derived from the image's ratio, is in the width's unit
  // and never stored. Nothing else leaves the row: no token, no HP figure, no note.
  'live.getDiffusion': async (server) => {
    const row = await diffusionRow(server);
    const win = row.window;
    const ratio = win ? await ratioOfIllustration(server, win.id || win.media) : 1;
    return {
      base: { kind: row.base.kind, id: row.base.id || '' },
      window: win ? { media: win.id || win.media, x: win.x, y: win.y, w: win.w, h: win.w / ratio } : null,
      view: row.view ? { zoom: row.view.z ?? row.view.zoom, panX: row.view.px ?? row.view.panX, panY: row.view.py ?? row.view.panY } : null,
    };
  },
  // table.getBattlemapInstance (crates/table/capabilities/get-battlemap-instance):
  // the DM sees everything; the players no hidden token, no hit point figure, no
  // note — each visible NPC gives its HealthBand@1 band instead.
  'table.getBattlemapInstance': async (server, { instance, view } = {}) => {
    const data = await instanceOf(server, instance);
    if (!data) return data;
    // dead-derived: an NPC is dead iff its current hit points are 0.
    const dressedTokens = (await dressed(server, data.state.tokens)).map((t) => (t.kind === 'pnj' ? { ...t, dead: t.hp === 0 } : t));
    if (view !== 'player') return { ...data, state: { ...data.state, tokens: dressedTokens } };
    const tokens = dressedTokens
      .filter((t) => t.visible)
      // An explicit list of what a token gives the players: its place, its look and its band, no figure and no note.
      .map((t) => ({ id: t.id, char: t.char, kind: t.kind, name: t.name, num: t.num, q: t.q, r: t.r, visible: true, states: t.states, dead: t.dead, initials: t.initials, color: t.color, portrait: t.portrait, healthBand: t.kind === 'pnj' ? healthBand(t.hp, t.hpMax) : null }));
    // hidden-keeps-rank: the players' order is the DM's without the hidden tokens, so a revealed one returns to its rank.
    const seen = new Set(tokens.map((t) => t.id));
    // A hidden token holding the turn is not named: nobody the players see has it.
    const combat = data.state.combat ? { ...data.state.combat, current: seen.has(data.state.combat.current) ? data.state.combat.current : '', order: data.state.combat.order.filter((o) => seen.has(o.id)) } : null;
    return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat } };
  },
  // session.getSession (crates/session/capabilities/get-session): one session's
  // header and the PCs present, a session of `session.listSessions` (the ones
  // created since included), its status labelled as SessionLifecycle@1 says. An
  // unknown id is no session: `null`, as the server answers it.
  'session.getSession': async (server, { id }) => {
    const session = (await server.load('session.listSessions')).find((s) => s.id === id);
    if (!session) return null;
    const status = session.status === 'played' ? 'closed' : session.status;
    return { id: session.id, order: session.order, title: session.title, date: session.date, status, statusLabel: STATUS_LABELS[status] ?? null, present: session.present };
  },
  // campaign.listImportedNotes (crates/campaign/capabilities/list-imported-notes):
  // the notes already imported, newest first, and the proposals of the latest
  // analysed note still awaiting validation. The imports are the fixture's
  // list; the proposals are what `ai.extractFromNotes` recorded (commands.js).
  'campaign.listImportedNotes': async (server) => {
    const fixture = await server.load('campaign.listImportedNotes');
    const imports = Array.isArray(fixture) ? fixture : (fixture && fixture.imports) || [];
    // Newest first, as the Capability answers.
    return { imports: [...imports].sort((x, y) => String(y.date).localeCompare(String(x.date))), proposals: server.noteProposals || [] };
  },
  // campaign.listEntitiesToCreate (crates/campaign/capabilities/list-entities-to-create):
  // the to-create list, answered as `{ items }`; the fixture is the bare list of entries.
  'campaign.listEntitiesToCreate': async (server) => {
    const fixture = await server.load('campaign.listEntitiesToCreate');
    return { items: Array.isArray(fixture) ? fixture : (fixture && fixture.items) || [] };
  },
  // soundbox.listSounds (crates/soundbox/capabilities/list-sounds): the
  // campaign's sounds with their effective gain — the own volume times the
  // master volume, both percents (EffectiveVolume@1) — and whether one plays
  // now (the fixture's `playing`, which soundbox.playSound and soundbox.stopAll move). The optional `search` keeps the
  // names that hold it and `tag` the sounds that carry it, whatever the case
  // (SoundTag@1); the fixture's `duration` is kept, the store has none yet.
  'soundbox.listSounds': async (server, { search, tag }) => {
    const { masterVolume, sounds } = await server.load('soundbox.listSounds');
    const lower = (text) => String(text ?? '').trim().toLowerCase();
    return {
      masterVolume,
      sounds: sounds
        .filter((s) => lower(s.name).includes(lower(search)) && (!lower(tag) || s.tags.some((t) => lower(t) === lower(tag))))
        .map((s) => ({ ...s, gain: (s.volume * masterVolume) / 10000, playing: s.playing === true })),
    };
  },
  // media.getBattlemap (crates/media/capabilities/get-battlemap): one
  // battlemap of the media library, by its id, its grid as HexGrid@1 states
  // it — the opacity a fraction of 1 where the mock-up's fixtures write a
  // percentage, and a hex always 5 feet. An illustration or an unknown id
  // is no battlemap: `null`, as the server answers it, not a `not-found`.
  'media.getBattlemap': async (server, { id }) => {
    const battlemap = (await server.load('media')).battlemaps.find((b) => b.id === id);
    if (!battlemap) return null;
    const { opacity, ...grid } = battlemap.grid;
    return { ...battlemap, grid: { ...grid, opacity: opacity / 100, feetPerHex: 5 } };
  },
  // session.listScenes (crates/session/capabilities/list-scenes): a session's scenes by rank. The fixture
  // is the scenes of session 4, the one in preparation; another session has none yet.
  'session.listScenes': async (server, { session }) =>
    session === 's4' ? await server.load('session.listScenes') : [],
  // session.getLastBrief (crates/session/capabilities/get-last-brief): a session's last oral brief and what its
  // analysis found. The fixture is the brief of session 4; another session has none: the same shape, empty.
  'session.getLastBrief': async (server, { session }) =>
    session === 's4'
      ? await server.load('session.getLastBrief')
      : { session, lastRun: null, duration: null, transcript: [], result: { scenes: [], recognized: [], missing: [] } },
  // One element's history, newest first: the version list of `entity` in the fixture; an element the fixture
  // does not know but `entities` does has never been saved (an empty history), any other has no answer (null).
  'campaign.listEntityVersions': async (server, { entity }) => {
    const history = (await server.load('campaign.listEntityVersions'))[entity];
    if (history) return history;
    const entities = await server.load('entities');
    const known = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === entity);
    return known ? { name: known.name, versions: [] } : null;
  },
  // One element's sheet: a character, a place or an event of `entities`.
  'campaign.getEntity': async (server, { id }) => {
    const entities = await server.load('entities');
    const entity = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === id);
    // AbilityModifier@1: the modifier of each score is computed, never stored.
    if (!entity?.stats) return entity;
    return { ...entity, modifiers: Object.fromEntries(Object.entries(entity.stats).map(([ability, score]) => [ability, typeof score === 'number' ? Math.floor((score - 10) / 2) : null])) };
  },
};
