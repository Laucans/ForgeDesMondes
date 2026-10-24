import { onCombat } from './_lib/commands.js';

export const commands = {
  // table.removeParticipant (crates/table/data-capabilities/remove-participant): the participant
  // `payload.participant` — named by its token, as the carousel shows it — taken out of the open combat;
  // its token stays on the map and the order closes up. The participant that holds the turn is refused
  // by `next-turn`: the Battlemap sends table.nextTurn first. One not in the combat is `not-found`.
  'table.removeParticipant': (server, { payload, targetId }) => onCombat(server, targetId, (combat) => {
    if (!combat.order.some((o) => o.id === payload.participant)) return ['not-found'];
    if (combat.current === payload.participant) return ['next-turn'];
    combat.order = combat.order.filter((o) => o.id !== payload.participant);
    return undefined;
  }),
};
