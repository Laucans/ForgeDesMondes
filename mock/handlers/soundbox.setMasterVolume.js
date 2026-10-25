export const commands = {
  // soundbox.setMasterVolume (crates/soundbox/data-capabilities/set-master-volume):
  // the master slider, in percent, kept to the whole percent; every sound
  // follows it, keeping their ratio (EffectiveVolume@1). A volume outside
  // 0..100, or no number, is refused by `master-volume-range`, never clamped;
  // a request naming no campaign is `not-found`.
  'soundbox.setMasterVolume': async (server, { payload, targetId }) => {
    if (!String(targetId || payload.campaign || '').trim()) return ['not-found'];
    const { volume } = payload;
    if (typeof volume !== 'number' || !(volume >= 0 && volume <= 100)) return ['master-volume-range'];
    for (const name of ['soundbox.listSounds', 'sounds']) {
      const fixture = await server.load(name);
      if (fixture) fixture.masterVolume = Math.round(volume);
    }
    return undefined;
  },
};
