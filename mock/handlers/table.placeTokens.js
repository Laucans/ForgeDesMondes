import { freeHexes } from './_lib/commands.js';
import { instanceOf, shareInstance } from './_lib/reads.js';

export const commands = {
  // table.placeTokens: one character dropped, once or « × n »; the new tokens
  // are numbered after those already taken for it (multi-drop-numbering), a
  // monster takes its hit points from its sheet, a PC has one token.
  'table.placeTokens': async (server, { payload, targetId }) => {
    const data = await instanceOf(server, targetId);
    const character = ((await server.load('campaign.listCharacters')) || []).find((c) => c.id === payload.character);
    if (!data || !character) return ['instance-isolated'];
    const { tokens } = data.state;
    const mine = tokens.filter((t) => t.char === character.id);
    const pnj = character.kind === 'pnj';
    // token-roster: a PC is placed when the scene's roster has it, and once.
    if (!pnj && (mine.length || !(data.roster || []).includes(character.id))) return ['token-roster'];
    const count = pnj ? Math.max(1, Math.min(12, Number(payload.count) || 1)) : 1;
    const base = character.tokenName || String(character.name).split(' ')[0];
    let num = Math.max(0, ...mine.map((t) => t.num || 0));
    const numbered = pnj && (count > 1 || mine.length > 0);
    for (const hex of freeHexes({ q: payload.q, r: payload.r }, tokens, count)) {
      num += 1;
      tokens.push({ id: `t-${character.id}${pnj ? `-${num}` : ''}`, char: character.id, kind: character.kind, name: numbered ? `${base} ${num}` : base, num: pnj ? num : null,
        q: hex.q, r: hex.r, visible: true, hp: pnj ? character.pvMax : null, hpMax: pnj ? character.pvMax : null, states: [], dead: false });
    }
    shareInstance(data);
    return undefined;
  },
};
