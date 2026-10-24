export const commands = {
  // live.showWindowOnPlayerView (crates/live/data-capabilities/show-window-on-player-view):
  // an illustration of the campaign opens in the floating window at its
  // default position and width, the three set together
  // (`window-fields-together`); its height follows the image's ratio and is
  // never stored (`window-proportional`). Another illustration replaces the
  // window's; the base and the framing stay as they are.
  'live.showWindowOnPlayerView': async (server, { payload }) => {
    const id = payload.media ?? payload.id;
    // A mock-up page that opened the window itself names the kind alone.
    if (!id) return payload.kind && payload.kind !== 'illustration' ? ['window-proportional'] : undefined;
    // An illustration listed, or one uploaded since (`media.uploadMedia`).
    const listed = [await server.load('media'), await server.load('media.listMedia')];
    if (!listed.some((media) => media && media.illustrations.some((i) => i.id === id))) return ['window-proportional'];
    const store = window.Mock && window.Mock.store;
    if (store) {
      const diffusion = store.get('diffusion', { base: { kind: 'cover' }, window: null, view: null });
      diffusion.window = { id, x: 0.6, y: 0.12, w: 0.3 };
      store.set('diffusion', diffusion);
    }
    return undefined;
  },
};
