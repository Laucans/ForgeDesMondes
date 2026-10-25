import { SAVED_BY, normalized } from './_lib/commands.js';

export const commands = {
  // ai.analyzeBrief (crates/ai/data-capabilities/analyze-brief): the brief
  // kept as text alone and cut into scenes; what the campaign holds is linked
  // to them, an NPC the brief speaks of gains a version of source `brief`
  // (read `ia`), every unknown NPC, place or event waits on the to-create
  // list with its type and excerpt, and nothing is created
  // (`brief-no-auto-creation`). The mock's AI finds what the fixture
  // OralBrief lists; a dictation is heard as its transcript. A brief that
  // holds no word is refused before anything is written.
  'ai.analyzeBrief': async (server, { payload }) => {
    const oral = await server.load('OralBrief');
    const typed = String(payload.text || '').trim();
    const lines = typed ? typed.split(/\n+/).map((line) => line.trim()).filter(Boolean) : payload.dictation ? oral.transcript : [];
    if (lines.length === 0) return ['no-brief'];
    const seconds = Number.isInteger(payload.durationSeconds) ? payload.durationSeconds : 0;
    const brief = await server.load('session.getLastBrief');
    Object.assign(brief, {
      session: payload.session,
      lastRun: new Date().toISOString().slice(0, 19),
      duration: `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`,
      transcript: lines,
      result: JSON.parse(JSON.stringify(oral.result)),
    });
    const fixture = await server.load('campaign.listEntitiesToCreate');
    const entries = Array.isArray(fixture) ? fixture : fixture.items;
    // The line that names it: by one of its words that is no article.
    const said = (name) => lines.find((line) => name.split(/[\s’']+/).some((word) => word.length > 3 && normalized(line).includes(normalized(word)))) || name;
    for (const missing of oral.result.missing) {
      const waiting = entries.some((e) => e.status === 'todo' && e.type === missing.type && normalized(e.name) === normalized(missing.name));
      if (!waiting) entries.push({ id: `tc-${server.uuid()}`, type: missing.type, name: missing.name, session: payload.session, sessionTitle: '', excerpt: said(missing.name), status: 'todo', entity: null });
    }
    const history = (await server.load('campaign.listEntityVersions')) || {};
    for (const known of oral.result.recognized.filter((r) => r.updated)) {
      const entry = history[known.id] || (history[known.id] = { name: known.name, versions: [] });
      const v = Math.max(0, ...entry.versions.map((version) => version.v)) + 1;
      entry.versions.unshift({ v, date: new Date().toISOString().slice(0, 19), origin: SAVED_BY.brief, source: 'brief', restoredFrom: null, seen: false, changes: [{ field: 'notes', old: null, new: said(known.name) }] });
    }
    return undefined;
  },
};
