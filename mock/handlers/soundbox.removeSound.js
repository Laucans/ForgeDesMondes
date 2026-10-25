export const commands = {
  // soundbox.removeSound (crates/soundbox/data-capabilities/remove-sound): the
  // sound leaves the library's readers (a soft delete, its tombstone is the
  // server's).
  'soundbox.removeSound': async (server, { targetId }) => {
    const library = await server.load('soundbox.listSounds');
    const at = library.sounds.findIndex((s) => s.id === targetId);
    if (at < 0) return ['not-found'];
    library.sounds.splice(at, 1);
    return undefined;
  },
};
