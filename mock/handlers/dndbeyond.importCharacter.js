import { changedBy } from './_lib/commands.js';

export const commands = {
  // dndbeyond.importCharacter (crates/dndbeyond/data-capabilities/import-character): a D&D
  // Beyond URL (or a bare id) imports the PC of the campaign linked to that character — the
  // character-sheets fake knows the ones the fixtures link, as they were last synchronised —
  // its statblock and sync date alone, its name, aliases, notes and token left as the DM made
  // them (`dndbeyond-resync-scope`), as a version of source dndbeyond. A PC targeted is filled
  // only if it is that one; a URL of another host, or of a character the fake does not know,
  // is refused and nothing is written.
  'dndbeyond.importCharacter': async (server, { payload, targetId }) => {
    // CharacterId::parse: a bare id, or a character's URL on dndbeyond.com — its host in any
    // case, its path as written.
    const url = String(payload.url ?? '').trim();
    const [, host, path] = /^https?:\/\/([^/]+)\/(.*)$/.exec(url) || [];
    const match = host && ['www.dndbeyond.com', 'dndbeyond.com'].includes(host.toLowerCase()) ? /^(?:profile\/[^/?#]+\/)?characters\/(\d+)(?:[/?#]|$)/.exec(path) : null;
    const digits = match ? match[1] : /^\d+$/.test(url) ? url : null;
    const ddbId = digits && BigInt(digits) > 0n ? String(BigInt(digits)) : null;
    if (!ddbId) return ['not-found'];
    const characters = (await server.load('campaign.listCharacters')) || [];
    const linked = characters.find((e) => e.kind === 'pj' && e.ddb && String(e.ddb.url ?? '').replace(/\/+$/, '').endsWith(`/characters/${ddbId}`));
    if (!linked) return ['not-found'];
    if (targetId && targetId !== linked.id) return ['already-imported'];
    await changedBy(server, linked, { ddb: { ...linked.ddb, syncedAt: new Date().toISOString().slice(0, 19) } }, 'dndbeyond', 'Import D&D Beyond', []);
    return undefined;
  },
};
