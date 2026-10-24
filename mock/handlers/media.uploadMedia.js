export const commands = {
  // media.uploadMedia (crates/media/data-capabilities/upload-media): a
  // battlemap or an illustration added to the library, the path the DM chose
  // recorded and no file copied. A battlemap arrives with its default grid
  // (`grid-only-for-battlemap`) — a 32 px hex, pointy, 45 % opaque — at 5
  // feet per hex; an illustration has none. A place, when one is named, is a
  // lieu of the campaign (`media-place-is-lieu`); a kind that is neither is
  // refused (`media-kind`, the table's CHECK).
  'media.uploadMedia': async (server, { payload }) => {
    const { kind, place = null } = payload;
    if (kind !== 'battlemap' && kind !== 'illustration') return ['media-kind'];
    if (place && !((await server.load('campaign.listPlaces')) || []).some((p) => p.id === place)) return ['media-place-is-lieu'];
    const base = { id: server.uuid(), title: String(payload.title || '').trim() || 'Sans titre', file: payload.file || '', uploadedAt: new Date().toISOString().slice(0, 10), place };
    // The library is read as `media.listMedia`, the grid editor as `media`: the new medium is in both.
    for (const name of ['media', 'media.listMedia']) {
      const media = await server.load(name);
      if (!media) continue;
      if (kind === 'battlemap') media.battlemaps.push({ ...base, width: payload.width || 0, height: payload.height || 0, grid: { size: 32, ox: 0, oy: 0, orient: 'pointy', color: 'text', opacity: 45 } });
      else media.illustrations.push({ ...base, ratio: payload.width > 0 && payload.height > 0 ? Math.round((payload.width / payload.height) * 100) / 100 : null });
    }
    return undefined;
  },
};
