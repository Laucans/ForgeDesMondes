import { onToken } from './_lib/commands.js';

export const commands = {
  // table.setTokenVisibility: hidden tokens keep their place; the players do not read them.
  'table.setTokenVisibility': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => { t.visible = payload.visible === true; }),
};
