import { SAVED_BY } from './_lib/commands.js';

export const commands = {
  // ai.createFromBrief (crates/ai/data-capabilities/create-from-brief): the
  // to-create entry becomes an element of the campaign, from its excerpt alone
  // or from the excerpt and the DM's refinement (the fixture ToCreateList
  // gives the AI's proposal for each); the entry is created once its element
  // is set (`to-create-leaves-once-created`), and an entry already created is
  // refused. An NPC gets its background, behaviour and sheet. The element has
  // its first version, of source `brief`, and the entry keeps the refinement
  // it was made from.
  'ai.createFromBrief': async (server, { payload }) => {
    const entries = await server.load('campaign.listEntitiesToCreate');
    const entry = (Array.isArray(entries) ? entries : entries?.items || []).find((candidate) => candidate.id === payload.entry);
    if (!entry) return ['not-found'];
    if (entry.status === 'created') return ['to-create-leaves-once-created'];
    const given = (await server.load('ToCreateList')).proposals?.[entry.id] || {};
    const refined = typeof payload.refinement === 'string' && payload.refinement.trim() !== '';
    const proposal = (refined ? given.refined : given.fromBrief) || { description: entry.excerpt };
    const { statblock, ...text } = proposal;
    const kind = { pnj: 'pnj', lieu: 'lieu', 'événement': 'evenement' }[entry.type];
    const entities = await server.load('entities');
    const list = { pnj: entities.characters, lieu: entities.places, evenement: entities.events }[kind];
    const id = `${entry.id.replace(/^tc-/, '')}-${server.uuid().slice(0, 8)}`;
    const figure = (value) => (Number.isNaN(parseInt(value, 10)) ? null : parseInt(value, 10));
    const vital = (label) => statblock?.vitals?.find((v) => v.label === label)?.value;
    list.push({
      id, name: entry.name, aliases: [], kind, notes: '', ...text,
      ...(statblock ? { type: statblock.type, stats: statblock.abilities, ca: figure(vital('CA')), pvMax: figure(vital('PV')), speed: vital('Vitesse'), features: statblock.features } : {}),
    });
    const history = (await server.load('campaign.listEntityVersions')) || {};
    history[id] = { name: entry.name, versions: [{ v: 1, date: new Date().toISOString().slice(0, 19), origin: SAVED_BY.brief, source: 'brief', seen: false, changes: [] }] };
    entry.status = 'created';
    entry.entity = id;
    entry.refinement = refined ? payload.refinement.trim() : null;
    return undefined;
  },
};
