export const commands = {
  // session.addJournalLine (crates/session/data-capabilities/add-journal-line):
  // a phrase the live session's microphone heard becomes the next line of
  // its journal, dated now, text only (`text-only-no-audio`). A blank phrase
  // adds nothing; the live session closed takes no more lines
  // (`status-lifecycle`); a session that is not the live one is `not-found`.
  'session.addJournalLine': async (server, { payload, targetId }) => {
    const text = String(payload.text || '').trim();
    if (!text) return ['text-only-no-audio'];
    const live = await server.load('session.getLiveSession');
    if (!live || live.id !== targetId) return ['not-found'];
    if ((live.status || 'live') !== 'live') return ['status-lifecycle'];
    const journal = await server.load('session.getSessionJournal');
    const now = new Date();
    const t = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`;
    (journal.journal ||= []).push({ t, text });
    server.bump();
    return undefined;
  },
};
