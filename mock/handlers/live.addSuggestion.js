export const commands = {
  // live.addSuggestion (crates/live/data-capabilities/add-suggestion): the
  // microphone recognised an element or a sound, and the pill proposes it. A
  // suggestion aims at exactly one of an element and a sound
  // (`suggestion-target`) and keeps the words heard (`live-only-local-audio`);
  // the mock-up has one live session, so it does not look at the session's
  // state. The pill reads the fixture newest first: the new suggestion goes
  // in front, named as the suggestion already in it for that target, if any.
  'live.addSuggestion': async (server, { payload }) => {
    const { entityId, soundId } = payload;
    const matchedText = String(payload.matchedText || '').trim();
    if (!entityId === !soundId) return ['suggestion-target'];
    if (!matchedText) return ['live-only-local-audio'];
    const id = entityId || soundId;
    const heard = await server.load('live.listSuggestions');
    const known = heard.find((suggestion) => suggestion.id === id);
    heard.unshift({
      heard: matchedText,
      kind: entityId ? 'entity' : 'sound',
      ...(entityId ? { type: known?.type || 'pnj' } : {}),
      id,
      name: known?.name || matchedText,
      via: matchedText,
    });
    server.bump();
  },
};
