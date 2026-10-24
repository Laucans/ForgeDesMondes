import { STATUS_LABELS } from './_lib/reads.js';

export const reads = {
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
};
