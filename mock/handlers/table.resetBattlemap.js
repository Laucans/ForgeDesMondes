import { instanceOf, shareInstance } from './_lib/reads.js';

export const commands = {
  // table.resetBattlemap: state-persisted-reset — the instance's tokens, combat and initiative are removed: an empty map.
  'table.resetBattlemap': async (server, { targetId }) => {
    const data = await instanceOf(server, targetId);
    if (!data) return ['instance-isolated'];
    data.state = { tokens: [], combat: null };
    data.pjInitiative = {};
    shareInstance(data);
    return undefined;
  },
};
