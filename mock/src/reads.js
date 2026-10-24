// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

// SessionLifecycle@1: the label of each status, and the mock-up's fixtures'
// « played » for what the Concept calls `closed`.
const STATUS_LABELS = { prep: 'En préparation', live: 'En direct', closed: 'Jouée' };

export const READS = {
  // table.getBattlemapInstance: the fixture's instance, or one a command made.
  'table.getBattlemapInstance': async (server, { instance }) => {
    const fixture = await server.load('table.getBattlemapInstance');
    if (!instance || instance === fixture.id) return fixture;
    return server.instances && server.instances.get(instance);
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
