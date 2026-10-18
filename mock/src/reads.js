// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

// The session the bar closes is the one of fixtures/LiveSessionBar.json: its journal and its summary are
// the `closing` block of that fixture, which the bar's recap reads right after the close. Any other
// session answers its own fixture.
async function closedBySession(server, variables) {
  const bar = await server.load('LiveSessionBar');
  return bar && variables.session === bar.session.id ? bar : null;
}

export const READS = {
  // session.getLiveSession: the running session, and the scenes of its screen
  // (fixtures/LiveSessionBar.json) the bar offers and marks the current one of.
  'session.getLiveSession': async (server) => {
    const live = await server.load('session.getLiveSession');
    const bar = await server.load('LiveSessionBar');
    return live && bar ? { ...live, scenes: bar.scenes, currentScene: bar.currentScene } : live;
  },
  'session.getSessionJournal': async (server, variables) => {
    const bar = await closedBySession(server, variables);
    if (!bar) return server.load('session.getSessionJournal');
    return { journal: Array.from({ length: bar.closing.journalLines }, (_, i) => ({ t: bar.session.startedAt, text: `Ligne ${i + 1} du journal transcrit` })) };
  },
  'session.getPreviousSessionSummary': async (server, variables) => {
    const bar = await closedBySession(server, variables);
    if (!bar) return server.load('session.getPreviousSessionSummary');
    const { id, order, title, date } = bar.session;
    return {
      session: { id, order, title, date },
      summary: bar.closing.summary,
      summaryMeta: `Généré à la clôture par ${bar.closing.summaryModel}`,
      mentions: bar.closing.mentions.map((name) => ({ id: name, type: 'pnj', name })),
      threads: bar.closing.threads,
    };
  },
};
