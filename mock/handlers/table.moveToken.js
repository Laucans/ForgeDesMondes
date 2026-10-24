import { onToken } from './_lib/commands.js';

export const commands = {
  // table.moveToken (crates/table/data-capabilities/move-token): (q, r) is the
  // one position both views read.
  'table.moveToken': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => { t.q = payload.q; t.r = payload.r; }),
};
