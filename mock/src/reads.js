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
  // campaign.getEntity (crates/campaign/capabilities/get-entity): one element's sheet, as the
  // Capability answers it — the lists `campaign.saveEntity` writes, so a save is read back. A PC or
  // an NPC has its stats with their modifier computed from the score (AbilityModifier@1), its token
  // (`tokenImage`: a dedicated image) and its D&D Beyond link; a place or an event has its kind
  // (`lieu`, `evenement`), aliases, description and notes alone. An unknown element is `null`.
  'campaign.getEntity': async (server, { id }) => {
    const lists = [['campaign.listCharacters', null], ['campaign.listPlaces', 'lieu'], ['campaign.listEvents', 'evenement']];
    for (const [name, kind] of lists) {
      const found = ((await server.load(name)) || []).find((e) => e.id === id);
      if (!found) continue;
      const base = { id: found.id, name: found.name, aliases: found.aliases || [], kind: kind || found.kind, description: found.description || '', notes: found.notes || '' };
      if (kind) return base;
      const modifiers = Object.fromEntries(Object.entries(found.stats).map(([ability, score]) => [ability, Math.floor((score - 10) / 2)]));
      const initials = found.initials || found.name.split(/\s+/).map((w) => w.charAt(0)).join('').slice(0, 2).toUpperCase();
      return { ...base, player: found.kind === 'pj' ? found.player || null : null, prompt: found.prompt || '', background: found.background || '', behavior: found.behavior || '', stats: found.stats, modifiers, ca: found.ca, pvMax: found.pvMax, features: found.features || [], color: found.color || 'accent', initials, portrait: Boolean(found.portrait), tokenImage: Boolean(found.token || found.tokenImage), crop: found.crop || null, ddb: found.ddb && found.ddb.url ? found.ddb : null };
    }
    return null;
  },
};
