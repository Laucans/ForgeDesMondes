import { onToken } from './_lib/commands.js';

export const commands = {
  // table.renameToken: only that token's name; an empty name is refused.
  'table.renameToken': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
    if (!String(payload.name || '').trim()) return ['name-required'];
    t.name = String(payload.name).trim();
    return undefined;
  }),
};
