import { dexModifier, onCombat } from './_lib/commands.js';

export const commands = {
  // table.addParticipant (crates/table/data-capabilities/add-participant): a token joins the running
  // order at the place its initiative earns. An NPC rolls a d20 plus its DEX modifier and is given no
  // figure; a PC takes the `payload.initiative` the DM entered (`npc-initiative-bounds`). A token not
  // on the instance is `not-found`; one already in the order is `initiative-tiebreak`.
  'table.addParticipant': (server, { payload, targetId }) => onCombat(server, targetId || payload.instance, async (combat, state) => {
    const token = state.tokens.find((t) => t.id === payload.token);
    if (!token) return ['not-found'];
    if (combat.order.some((o) => o.id === token.id)) return ['initiative-tiebreak'];
    const pc = token.kind === 'pj';
    if (pc ? !Number.isInteger(payload.initiative) : payload.initiative != null) return ['npc-initiative-bounds'];
    const dex = dexModifier((await server.load('campaign.listCharacters')) || [], token.char);
    const init = pc ? payload.initiative : 1 + Math.floor(Math.random() * 20) + dex;
    const at = combat.order.findIndex((o) => o.init < init || (o.init === init && o.dex < dex));
    combat.order.splice(at < 0 ? combat.order.length : at, 0, { id: token.id, init, dex });
    return undefined;
  }),
};
