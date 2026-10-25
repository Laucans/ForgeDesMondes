import { dressed, healthBand, instanceOf } from './_lib/reads.js';

export const reads = {
  // table.getBattlemapInstance (crates/table/capabilities/get-battlemap-instance):
  // the DM sees everything; the players no hidden token, no hit point figure, no
  // note — each visible NPC gives its HealthBand@1 band instead.
  'table.getBattlemapInstance': async (server, { instance, view } = {}) => {
    const data = await instanceOf(server, instance);
    if (!data) return data;
    // dead-derived: an NPC is dead iff its current hit points are 0.
    const dressedTokens = (await dressed(server, data.state.tokens)).map((t) => (t.kind === 'pnj' ? { ...t, dead: t.hp === 0 } : t));
    if (view !== 'player') return { ...data, state: { ...data.state, tokens: dressedTokens } };
    const tokens = dressedTokens
      .filter((t) => t.visible)
      // An explicit list of what a token gives the players: its place, its look and its band, no figure and no note.
      .map((t) => ({ id: t.id, char: t.char, kind: t.kind, name: t.name, num: t.num, q: t.q, r: t.r, visible: true, states: t.states, dead: t.dead, initials: t.initials, color: t.color, portrait: t.portrait, healthBand: t.kind === 'pnj' ? healthBand(t.hp, t.hpMax) : null }));
    // hidden-keeps-rank: the players' order is the DM's without the hidden tokens, so a revealed one returns to its rank.
    const seen = new Set(tokens.map((t) => t.id));
    // A hidden token holding the turn is not named: nobody the players see has it.
    const combat = data.state.combat ? { ...data.state.combat, current: seen.has(data.state.combat.current) ? data.state.combat.current : '', order: data.state.combat.order.filter((o) => seen.has(o.id)) } : null;
    return { id: data.id, battlemap: data.battlemap, session: data.session, scene: data.scene, label: data.label, state: { tokens, combat } };
  },
};
