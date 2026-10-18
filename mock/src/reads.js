// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

// The label of each status, SessionLifecycle@1; the mock-up's list writes a
// session already played `played`.
const STATUS_LABELS = { prep: 'En préparation', live: 'En direct', closed: 'Jouée', played: 'Jouée' };

export const READS = {
  // One session's header and its present PCs: a session of `session.listSessions`.
  'session.getSession': async (server, { id }) => {
    const session = (await server.load('session.listSessions')).find((s) => s.id === id);
    if (!session) return undefined;
    const { summary: _summary, ...header } = session;
    return { ...header, statusLabel: STATUS_LABELS[session.status] ?? null };
  },
  // A session's scenes by rank: the fixture is the scenes of session 4, the one
  // in preparation; another session has none yet.
  'session.listScenes': async (server, { session }) =>
    session === 's4' ? await server.load('session.listScenes') : [],
};
