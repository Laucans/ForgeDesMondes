import { advance, onCombat } from './_lib/commands.js';

export const commands = {
  // table.nextTurn: next-turn — the next living participant by rank, the round counting up when it comes back to the first.
  'table.nextTurn': (server, { targetId }) => onCombat(server, targetId, (combat, state) => { advance(combat, state); return undefined; }),
};
