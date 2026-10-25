import { onToken } from './_lib/commands.js';

export const commands = {
  // table.removeToken: the token leaves the map.
  'table.removeToken': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t, state) => { state.tokens = state.tokens.filter((x) => x !== t); }),
};
