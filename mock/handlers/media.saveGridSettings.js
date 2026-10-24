export const commands = {
  // media.saveGridSettings (crates/media/data-capabilities/save-grid-settings):
  // a battlemap's grid kept — the six fields of HexGrid@1, the opacity a
  // fraction of 1 where the mock-up's fixtures write a percentage. A hex size
  // of 0 or an opacity outside 0..1 is refused by `grid-values-range`; an
  // illustration or an unknown id has no grid (`grid-only-for-battlemap`).
  'media.saveGridSettings': async (server, { payload, targetId }) => {
    const battlemap = (await server.load('media')).battlemaps.find((b) => b.id === targetId);
    if (!battlemap) return ['grid-only-for-battlemap'];
    const { size, ox, oy, orient, color, opacity } = payload;
    if (!(size > 0) || !(opacity >= 0 && opacity <= 1)) return ['grid-values-range'];
    battlemap.grid = { size, ox, oy, orient, color, opacity: Math.round(opacity * 100) };
    return undefined;
  },
};
