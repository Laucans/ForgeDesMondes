export const commands = {
  // live.showCover (crates/live/data-capabilities/show-cover): the player
  // view goes back to the campaign's cover, with neither a media nor an
  // instance as its base (`base-coherent`); the floating window and the
  // framing stay as they are.
  'live.showCover': async () => {
    const store = window.Mock && window.Mock.store;
    if (store) {
      const diffusion = store.get('diffusion', { base: { kind: 'cover' }, window: null, view: null });
      diffusion.base = { kind: 'cover' };
      store.set('diffusion', diffusion);
    }
    return undefined;
  },
};
