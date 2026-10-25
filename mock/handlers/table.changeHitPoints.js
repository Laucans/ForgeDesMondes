import { onToken } from './_lib/commands.js';

export const commands = {
  // table.changeHitPoints (crates/table/data-capabilities/change-hit-points):
  // hp-change-one-token — the targeted token only; pj-hp-not-tracked — a PC has
  // no hit points; the current ones stay within 0 and the maximum, and an NPC
  // is dead iff they are 0 (dead-derived).
  'table.changeHitPoints': (server, { payload, targetId }) => onToken(server, { id: targetId }, payload.token, (t) => {
    if (t.kind !== 'pnj' || typeof t.hp !== 'number') return ['pj-hp-not-tracked'];
    t.hp = Math.max(0, Math.min(t.hpMax, t.hp + Math.trunc(Number(payload.delta) || 0)));
    t.dead = t.hp === 0;
    return undefined;
  }),
};
