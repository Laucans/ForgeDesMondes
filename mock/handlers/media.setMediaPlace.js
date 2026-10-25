export const commands = {
  // media.setMediaPlace (crates/media/data-capabilities/set-media-place): a
  // media — battlemap or illustration — attached to one of the campaign's
  // places, or detached with `place: null`, and nothing else of it changes. A
  // place that is no lieu the fixtures list is refused by
  // `media-place-is-lieu`; an unknown media is `not-found`.
  'media.setMediaPlace': async (server, { payload, targetId }) => {
    const id = targetId || payload.media;
    const listed = [await server.load('media'), await server.load('media.listMedia')];
    const found = listed.flatMap((media) => media.battlemaps.concat(media.illustrations)).filter((m) => m.id === id);
    if (!found.length) return ['not-found'];
    const place = payload.place ?? null;
    if (place !== null && !(await server.load('campaign.listPlaces')).some((p) => p.id === place)) return ['media-place-is-lieu'];
    for (const medium of found) medium.place = place;
    return undefined;
  },
};
