export const commands = {
  // session.addScene (crates/session/data-capabilities/add-scene): a new
  // scene, ranked after the session's last (`scene-order-contiguous`), with
  // its title, its notes and the places it links — each a place of the
  // fixtures' campaign, else `scene-links-same-campaign` — whose media
  // follow it to the session (`session-media-by-place`, derived, never
  // stored). Its notes are kept as written. A blank title is
  // `scene-title-required`: the mock answers violation ids, where the
  // server answers 422.
  'session.addScene': async (server, { payload }) => {
    const title = String(payload.title || '').trim();
    if (!title) return ['scene-title-required'];
    const known = new Set((await server.load('campaign.listPlaces')).map((p) => p.id));
    const places = [...new Set(Array.isArray(payload.places) ? payload.places : [])];
    if (places.some((id) => !known.has(id))) return ['scene-links-same-campaign'];
    const library = await server.load('media.listMedia');
    const media = library.battlemaps.concat(library.illustrations).filter((m) => places.includes(m.place)).map((m) => m.id);
    const scenes = await server.load('session.listScenes');
    scenes.push({
      id: server.uuid(),
      order: scenes.reduce((last, sc) => Math.max(last, sc.order), 0) + 1,
      title,
      places,
      characters: [],
      events: [],
      media,
      toCreate: [],
      battlemap: null,
      notes: typeof payload.notes === 'string' ? payload.notes : null,
    });
    return undefined;
  },
};
