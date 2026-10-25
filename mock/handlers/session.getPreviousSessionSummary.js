import { closedBySession } from './_lib/reads.js';

export const reads = {
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
