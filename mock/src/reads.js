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
 * one server would. Undefined when it is another instance.
 */
export async function instanceOf(server, id) {
  const data = await server.load('table.getBattlemapInstance');
  if (!data || (id && data.id !== id)) return undefined;
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

export const READS = {
  // table.getBattlemapInstance (crates/table/capabilities/get-battlemap-instance):
  // the DM sees everything; the players no hidden token, no hit point figure, no
  // note — each visible NPC gives its HealthBand@1 band instead.
  'table.getBattlemapInstance': async (server, { instance, view } = {}) => {
    const data = await instanceOf(server, instance);
    if (!data) return data;
    const dressedTokens = await dressed(server, data.state.tokens);
    if (view !== 'player') return { ...data, state: { ...data.state, tokens: dressedTokens } };
    const tokens = dressedTokens
      .filter((t) => t.visible)
      .map(({ hp, hpMax, ...rest }) => ({ ...rest, healthBand: rest.kind === 'pnj' ? healthBand(hp, hpMax) : null }));
    return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat: null } };
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
    return [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === id);
  },
};
