export const commands = {
  // session.updateSceneNotes (crates/session/data-capabilities/update-scene-notes):
  // the DM writes the notes of a scene of the fixtures' list, kept as
  // written (a blank text is a text). Notes that are no text, or a scene the
  // list does not hold, change nothing: the mock answers violation ids
  // where the server answers 422 (404 for a scene it does not hold).
  'session.updateSceneNotes': async (server, { payload }) => {
    if (typeof payload.notes !== 'string') return ['scene-notes-text'];
    const scene = (await server.load('session.listScenes')).find((sc) => sc.id === payload.scene);
    if (!scene) return ['scene-not-in-session'];
    scene.notes = payload.notes;
    return undefined;
  },
};
