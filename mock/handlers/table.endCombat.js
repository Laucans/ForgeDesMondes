import { onCombat } from './_lib/commands.js';

export const commands = {
  // table.endCombat: the combat ends; the tokens stay as they are.
  'table.endCombat': (server, { targetId }) => onCombat(server, targetId, (combat, state) => { state.combat = null; return undefined; }),
};
