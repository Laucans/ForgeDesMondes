export const commands = {
  // session.addPlaceToSession (crates/session/data-capabilities/add-place-to-session):
  // a place of the campaign joins a scene of the session — `payload.scene`,
  // else the first — after its last element, and stays there once (a place
  // already in the scene is not linked twice). Its media follow by
  // `session-media-by-place`: they are listed in the scene's media, never
  // stored apart. A place that is no lieu of the campaign is refused by
  // `scene-links-same-campaign`; a scene the session does not have is
  // `not-found` — the server answers 422 `scene-links-same-campaign` for a
  // scene of another session, which the fixtures, one session's, never list.
  'session.addPlaceToSession': async (server, { payload }) => {
    const place = payload.place;
    const scenes = (await server.load('session.listScenes')) || [];
    const first = scenes.reduce((low, sc) => (!low || sc.order < low.order ? sc : low), null);
    const scene = payload.scene ? scenes.find((sc) => sc.id === payload.scene) : first;
    if (!scene) return ['not-found'];
    if (!((await server.load('campaign.listPlaces')) || []).some((p) => p.id === place)) return ['scene-links-same-campaign'];
    if (!scene.places.includes(place)) scene.places.push(place);
    const library = (await server.load('media.listMedia')) || { battlemaps: [], illustrations: [] };
    for (const medium of library.battlemaps.concat(library.illustrations)) {
      if (medium.place === place && !scene.media.includes(medium.id)) scene.media.push(medium.id);
    }
    return undefined;
  },
};
