import { changedBy, elementOf } from './_lib/commands.js';

export const commands = {
  // ai.proposeCharacter (crates/ai/data-capabilities/propose-character): the
  // initial prompt told or typed gives the character's background and behaviour and, for an NPC,
  // a statblock — the proposals of fixtures/EntityEditor.json, the fake llm's answers. It applies
  // at once and is a version like any other (source ia), unseen until the DM looks. A prompt with
  // no text, or an element the lists do not hold, is refused.
  'ai.proposeCharacter': async (server, { payload, targetId }) => {
    const { element } = await elementOf(server, targetId);
    if (!element || !String(payload.prompt ?? '').trim()) return ['not-found'];
    const { proposals } = await server.load('EntityEditor');
    const proposal = proposals[element.id] || proposals.default;
    const patch = { prompt: payload.prompt, background: proposal.background, behavior: proposal.behavior };
    if (element.kind === 'pnj') Object.assign(patch, { stats: { ...proposal.stats }, ca: proposal.ca, pvMax: proposal.pvMax, features: proposal.features.map((f) => ({ ...f })) });
    await changedBy(server, element, patch, 'ia', 'Prompt initial', element.kind === 'pnj' ? proposal.changes : proposal.changes.slice(0, 1));
    return undefined;
  },
};
