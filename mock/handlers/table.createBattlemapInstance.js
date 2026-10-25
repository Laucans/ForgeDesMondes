export const commands = {
  // campaign.restoreEntityVersion (EntityVersion@1, restore-exact): the element `targetId` goes back to exactly
  // the version `payload.version` — or, with `payload.changes`, only the changes at those positions of the
  // version after it are undone, each field back to what it was. Either way the restoration is a new version
  // on top of the history — source restauration, restoredFrom the version — whose changes are what moved since
  // the version it replaces; versions 1..n stay as they were. A version the element does not have is refused.
  // table.createBattlemapInstance (crates/table/data-capabilities/create-battlemap-instance):
  // the scene's own copy of a battlemap of the campaign, empty; the battlemap, its grid and
  // the scene's media are left as they are (`instance-isolated`). A scene of another session,
  // or a battlemap the campaign's library does not list, is `not-found`; a scene that has its
  // instance is refused (`one-instance-per-scene`).
  'table.createBattlemapInstance': async (server, { payload }) => {
    const session = await server.load('session.getSession');
    const scene = (await server.load('session.listScenes')).find((sc) => sc.id === payload.scene);
    const map = (await server.load('media.listMedia')).battlemaps.find((m) => m.id === payload.battlemap);
    if (!session || session.id !== payload.session || !scene || !map) return ['not-found'];
    if (scene.battlemap) return ['one-instance-per-scene'];
    const id = `bmi-${map.id}-${scene.id}`;
    server.instances = server.instances || new Map();
    server.instances.set(id, { id, battlemap: map.id, session: payload.session, scene: scene.id, label: scene.title, roster: [], pjInitiative: {}, state: { tokens: [], combat: null } });
    scene.battlemap = id;
    return undefined;
  },
};
