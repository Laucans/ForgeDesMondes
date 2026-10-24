import { STATUS_LABELS } from './_lib/reads.js';

export const reads = {
  // session.getLiveSession (crates/session/capabilities/get-live-session): the running session as
  // `{ session, microphone }` — its header and the PCs ticked present (those of the session in
  // `session.listSessions`), the microphone listening until the session is closed — and, beside
  // them, the scenes of its screen (fixtures/LiveSessionBar.json) the bar offers and marks the
  // current one of.
  'session.getLiveSession': async (server) => {
    const live = await server.load('session.getLiveSession');
    const bar = await server.load('LiveSessionBar');
    if (!live) return live;
    const listed = (await server.load('session.listSessions')).find((s) => s.id === live.id);
    const status = live.status || 'live';
    return {
      session: { ...live, status, statusLabel: STATUS_LABELS[status] ?? null, present: listed?.present ?? [] },
      microphone: { on: status !== 'closed' },
      ...(bar ? { scenes: bar.scenes, currentScene: bar.currentScene } : {}),
    };
  },
};
