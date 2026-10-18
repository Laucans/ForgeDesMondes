// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

const bridged = new WeakSet();
// The state each instance was loaded with: « Réinitialiser la carte » sets it back.
const pristine = new Map();

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
  if (!pristine.has(data.id)) pristine.set(data.id, structuredClone(data.state));
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

/** The state the instance was loaded with, as a copy. */
export const pristineState = (id) => structuredClone(pristine.get(id));

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
    // dead-derived: an NPC is dead iff its current hit points are 0.
    const dressedTokens = (await dressed(server, data.state.tokens)).map((t) => (t.kind === 'pnj' ? { ...t, dead: t.hp === 0 } : t));
    if (view !== 'player') return { ...data, state: { ...data.state, tokens: dressedTokens } };
    const tokens = dressedTokens
      .filter((t) => t.visible)
      .map(({ hp, hpMax, ...rest }) => ({ ...rest, healthBand: rest.kind === 'pnj' ? healthBand(hp, hpMax) : null }));
    // hidden-keeps-rank: the players' order is the DM's without the hidden tokens, so a revealed one returns to its rank.
    const seen = new Set(tokens.map((t) => t.id));
    // A hidden token holding the turn is not named: nobody the players see has it.
    const combat = data.state.combat ? { ...data.state.combat, current: seen.has(data.state.combat.current) ? data.state.combat.current : '', order: data.state.combat.order.filter((o) => seen.has(o.id)) } : null;
    return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat } };
  },
  // media.getBattlemap (crates/media/capabilities/get-battlemap): one battlemap
  // by its id, its grid per HexGrid@1 — the opacity a fraction of 1, a hex 5 feet.
  'media.getBattlemap': async (server, { id } = {}) => {
    const map = await server.load('media.getBattlemap');
    if (!map || (id && map.id !== id)) return undefined;
    return { ...map, grid: { ...map.grid, opacity: map.grid.opacity > 1 ? map.grid.opacity / 100 : map.grid.opacity, feetPerHex: 5 } };
  },
};
