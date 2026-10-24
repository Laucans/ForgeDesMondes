import { instanceOf } from './_lib/reads.js';

export const commands = {
  // live.showOnPlayerView (crates/live/data-capabilities/show-on-player-view):
  // a battlemap instance of the session, or an illustration of the campaign,
  // becomes the player view's base, framed as the DM framed it (1 and 0, 0
  // when no framing is sent); the floating window stays as it is. Any other
  // base would leave it incoherent (`base-coherent`).
  'live.showOnPlayerView': async (server, { payload }) => {
    let base;
    if (payload.kind === 'battlemap') {
      const data = await instanceOf(server, payload.instance);
      if (!data) return ['base-coherent'];
      base = { kind: 'battlemap', id: data.id };
    } else if (payload.kind === 'illustration') {
      // An illustration listed, or one uploaded since (`media.uploadMedia`).
      const listed = [await server.load('media'), await server.load('media.listMedia')];
      if (!listed.some((media) => media && media.illustrations.some((i) => i.id === payload.media))) return ['base-coherent'];
      base = { kind: 'illustration', id: payload.media };
    } else return ['base-coherent'];
    const store = window.Mock && window.Mock.store;
    if (store) {
      const diffusion = store.get('diffusion', { base: { kind: 'cover' }, window: null, view: null });
      diffusion.base = base;
      diffusion.view = { z: payload.zoom ?? 1, px: payload.panX ?? 0, py: payload.panY ?? 0 };
      store.set('diffusion', diffusion);
    }
    return undefined;
  },
};
