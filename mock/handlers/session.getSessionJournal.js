import { closedBySession } from './_lib/reads.js';

export const reads = {
  'session.getSessionJournal': async (server, variables) => {
    const bar = await closedBySession(server, variables);
    if (!bar) return server.load('session.getSessionJournal');
    return { journal: Array.from({ length: bar.closing.journalLines }, (_, i) => ({ t: bar.session.startedAt, text: `Ligne ${i + 1} du journal transcrit` })) };
  },
};
