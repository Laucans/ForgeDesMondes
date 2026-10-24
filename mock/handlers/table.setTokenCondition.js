import { onToken } from './_lib/commands.js';

export const commands = {
  // table.setTokenCondition (crates/table/data-capabilities/set-token-condition): the badge
  // `payload.state` put on that token when `payload.on`, after the others and once, or taken off,
  // the others keeping their order — its conditions alone. A state that is no name without the
  // blanks around it, or holds U+0000, or an `on` that is no boolean, is `state-persisted-reset`.
  'table.setTokenCondition': (server, { payload, targetId }) => onToken(server, { id: targetId || payload.instance }, payload.token, (t) => {
    const state = typeof payload.state === 'string' ? payload.state.trim() : '';
    if (!state || state.includes('\0') || typeof payload.on !== 'boolean') return ['state-persisted-reset'];
    const held = t.states || [];
    t.states = payload.on ? (held.includes(state) ? held : [...held, state]) : held.filter((s) => s !== state);
    return undefined;
  }),
};
