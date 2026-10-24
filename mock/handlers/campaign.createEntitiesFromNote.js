export const commands = {
  // campaign.createEntitiesFromNote (crates/campaign/data-capabilities/create-entities-from-note):
  // one element per accepted proposal, the raw note kept as an import listed
  // newest first, the proposals no longer awaiting.
  'campaign.createEntitiesFromNote': async (server, { payload }) => {
    const proposals = server.noteProposals || [];
    const accepted = Array.isArray(payload.accepted) ? payload.accepted : [];
    const created = proposals.filter((p) => accepted.includes(p.id));
    const fixture = await server.load('campaign.listImportedNotes');
    const imports = Array.isArray(fixture) ? fixture : fixture.imports;
    const note = server.noteText || '';
    imports.unshift({ id: `imp-${server.uuid()}`, date: new Date().toISOString().slice(0, 10), title: note.split('\n')[0].replace(/[.\s]+$/, '') || 'Notes importées', chars: note.length, proposed: proposals.length, created: created.length });
    server.noteProposals = [];
    return undefined;
  },
};
