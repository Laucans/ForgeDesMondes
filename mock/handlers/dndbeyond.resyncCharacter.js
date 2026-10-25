import { changedBy, elementOf } from './_lib/commands.js';

export const commands = {
  // dndbeyond.resyncCharacter (crates/dndbeyond/data-capabilities/resync-character): the stats,
  // hit points and spells of a PC linked to D&D Beyond are read again — the patch of
  // fixtures/EntityEditor.json, and the spells its « Sorts et capacités » line adds, as the
  // character-sheets fake plays the level-up — whose changes the history keeps; the aliases, the
  // notes and the token are left as they are (`dndbeyond-resync-scope`). A sheet with no link is
  // refused.
  'dndbeyond.resyncCharacter': async (server, { targetId }) => {
    const { element } = await elementOf(server, targetId);
    if (!element || !element.ddb || !element.ddb.url) return ['not-found'];
    const { ddbResync } = await server.load('EntityEditor');
    const resync = ddbResync[element.id] || ddbResync.default;
    const { stats, pvMax } = resync.patch;
    const spells = resync.changes.find((c) => c.field === 'Sorts et capacités');
    const names = (line) => line.split(',').map((name) => name.trim()).filter(Boolean);
    const known = (element.features || []).map((f) => f.name);
    const gained = spells ? names(spells.new).filter((name) => !names(spells.old).includes(name) && !known.includes(name)) : [];
    const features = gained.length ? { features: [...(element.features || []), ...gained.map((name) => ({ name, text: 'Ajouté à la fiche D&D Beyond lors de la montée de niveau.' }))] } : {};
    await changedBy(server, element, { ...(stats ? { stats: { ...element.stats, ...stats } } : {}), ...(pvMax ? { pvMax } : {}), ...features, ddb: { ...element.ddb, syncedAt: new Date().toISOString().slice(0, 19) } }, 'dndbeyond', 'Resynchronisation D&D Beyond', resync.changes.filter((c) => c.field === 'PV max' || (c.field === 'Sorts et capacités' && gained.length) || (stats && c.field in stats)));
    return undefined;
  },
};
