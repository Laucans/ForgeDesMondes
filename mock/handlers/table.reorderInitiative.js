import { onCombat } from './_lib/commands.js';

export const commands = {
  // table.reorderInitiative (crates/table/data-capabilities/reorder-initiative): the open combat
  // reordered as `payload.order` gives it, every participant once, named by its token — the ranks
  // 1 to n in that order; the turn and the round stay. An order that is no list, omits a
  // participant, repeats one or names one not in the combat is `initiative-tiebreak`.
  'table.reorderInitiative': (server, { payload, targetId }) => onCombat(server, targetId || payload.instance, (combat) => {
    const named = Array.isArray(payload.order) ? payload.order : [];
    if (named.length !== combat.order.length) return ['initiative-tiebreak'];
    const order = [];
    for (const id of named) {
      const found = combat.order.find((o) => o.id === id && !order.includes(o));
      if (!found) return ['initiative-tiebreak'];
      order.push(found);
    }
    combat.order = order;
    return undefined;
  }),
};
