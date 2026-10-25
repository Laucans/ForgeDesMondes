// live.listSuggestions (crates/live/capabilities/list-suggestions): the pill's
// stack — the 5 most recent suggestions — and the session's whole history,
// both newest first. The fixture lists the suggestions newest first and gives
// no time: each is heard a minute before the previous one. `heard`, the
// sentence the mock-up's pill narrates, is not part of the read.
const PILE = 5;
const MINUTE = 60 * 1000;
// Anchored once: a suggestion keeps its time from one read to the next.
const NOW = Date.now();

export const reads = {
  'live.listSuggestions': async (server) => {
    const heard = await server.load('live.listSuggestions');
    const history = heard.map(({ id, kind, type, name, via }, i) => ({
      id,
      kind,
      ...(kind === 'entity' ? { type } : {}),
      name,
      via,
      detectedAt: new Date(NOW - i * MINUTE).toISOString(),
    }));
    return { suggestions: history.slice(0, PILE), history };
  },
};
