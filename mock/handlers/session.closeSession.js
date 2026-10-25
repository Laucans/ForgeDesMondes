export const commands = {
  // session.closeSession (crates/session/data-capabilities/close-session): the
  // live session closed in one action (`close-session`) — its microphone off,
  // its close dated, its journal kept as it was — and a summary of its journal
  // written by the active provider's summary model, the journal's text alone
  // sent, as a Journal request (`text-only-no-audio`). The summary is the one
  // the next preparation shows. A session that is not live is refused by
  // `status-lifecycle`; one that is not the live session is `not-found`.
  'session.closeSession': async (server, { payload, targetId }) => {
    const live = await server.load('session.getLiveSession');
    if (!live || live.id !== (targetId || payload.id)) return ['not-found'];
    if ((live.status || 'live') !== 'live') return ['status-lifecycle'];
    const settings = await server.load('ai.getSettings');
    const binary = settings.mode === 'binary' && settings.binary;
    const model = (binary || settings.providers?.[settings.provider] || {}).summaryModel || '';
    const provider = binary ? `${binary.binary}-binary` : settings.provider === 'opencode' ? 'opencode-server' : 'cloud';
    const { journal = [] } = (await server.load('session.getSessionJournal')) || {};
    const lines = journal.length ? journal.map((line) => `${line.t} ${line.text}`) : ['(aucune ligne transcrite)'];
    const text = [`Journal de la séance « ${live.title} »`, ...lines].join('\n');
    const requests = (await server.load('ai.listProviderRequests'))?.requests;
    if (requests) requests.push({ sequence: requests.length + 1, kind: 'journal', provider, text, modalities: ['text'] });
    const summary = `Résumé de la séance ${live.order} généré par ${model} à partir de ${journal.length} lignes du journal.`;
    Object.assign(live, { status: 'closed', closedAt: new Date().toISOString(), microphone: { on: false }, summary, summaryModel: model });
    const previous = await server.load('session.getPreviousSessionSummary');
    if (previous) Object.assign(previous, { session: { id: live.id, order: live.order, title: live.title, date: live.date }, summary, summaryMeta: `Généré à la clôture par ${model}` });
    return undefined;
  },
};
