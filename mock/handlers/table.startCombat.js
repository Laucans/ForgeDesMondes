import { dexModifier } from './_lib/commands.js';
import { instanceOf, shareInstance } from './_lib/reads.js';

export const commands = {
  // table.startCombat: the NPC roll a d20 plus the DEX modifier of their sheet,
  // the DM enters the PCs' initiative; the order is the initiative, then the DEX,
  // then at random (initiative-tiebreak). A hidden token is in the order and keeps its rank.
  'table.startCombat': async (server, { payload, targetId }) => {
    const data = await instanceOf(server, targetId);
    if (!data) return ['instance-isolated'];
    if (data.state.combat) return ['one-active-combat'];
    const characters = (await server.load('campaign.listCharacters')) || [];
    const rolls = payload.rolls || {};
    const entered = payload.initiative || {};
    const order = [];
    for (const t of data.state.tokens.filter((x) => !x.dead)) {
      const dex = dexModifier(characters, t.char);
      if (t.kind === 'pnj') {
        const roll = Number(rolls[t.id]);
        // npc-initiative-bounds: a d20 between 1 and 20.
        if (!(roll >= 1 && roll <= 20)) return ['npc-initiative-bounds'];
        order.push({ id: t.id, init: roll + dex, dex, tie: Math.random() });
      } else {
        const init = Number(entered[t.char]);
        if (!(init >= 1)) return ['npc-initiative-bounds'];
        order.push({ id: t.id, init, dex, tie: Math.random() });
      }
    }
    if (!order.length) return ['one-active-combat'];
    order.sort((a, b) => b.init - a.init || b.dex - a.dex || b.tie - a.tie);
    data.pjInitiative = { ...data.pjInitiative, ...entered };
    data.state.combat = { round: 1, current: order[0].id, order: order.map(({ id, init, dex }) => ({ id, init, dex })) };
    shareInstance(data);
    return undefined;
  },
};
