// The helpers of mock/src/reads.js, split by scripts/split-handlers.mjs: shared by the handler files that import them.
// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

export const bridged = new WeakSet();

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
export function healthBand(hp, hpMax) {
  if (typeof hp !== 'number' || typeof hpMax !== 'number') return null;
  const current = Math.max(0, Math.min(hpMax, hp));
  return current === 0 ? 'mort' : current === hpMax ? 'indemne' : 2 * current > hpMax ? 'blessé' : 'en sang';
}

// What a token carries besides its place: the token of its character (colour, initials, portrait), as the Capability gives it.
export async function dressed(server, tokens) {
  const characters = (await server.load('campaign.listCharacters')) || [];
  return tokens.map((t) => {
    const c = characters.find((x) => x.id === t.char);
    return c ? { ...t, initials: c.initials, color: c.color, portrait: c.portrait } : t;
  });
}

export let diffusionBridged = false;

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
export const STATUS_LABELS = { prep: 'En préparation', live: 'En direct', closed: 'Jouée' };
