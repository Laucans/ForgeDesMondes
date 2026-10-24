export const commands = {
  // ai.extractFromNotes (crates/ai/data-capabilities/extract-from-notes): the
  // note analysed into proposals, nothing created (`no-creation-before-
  // validation`). The mock's AI proposes what the fixture NoteImport lists; a
  // proposal that names an existing element starts unchecked.
  'ai.extractFromNotes': async (server, { payload }) => {
    const text = String(payload.text || '');
    const { proposals } = await server.load('NoteImport');
    server.noteText = text;
    server.noteProposals = proposals.map((p) => ({ id: p.id, type: p.type, name: p.name, excerpt: p.excerpt, existing: p.existing, checked: !p.existing }));
    return undefined;
  },
};
