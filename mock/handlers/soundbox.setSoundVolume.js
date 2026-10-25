export const commands = {
  // soundbox.setSoundVolume (crates/soundbox/data-capabilities/set-sound-volume):
  // one sound's own volume, in percent, from 0 to 100; out of range it is
  // refused by `sound-volume-range`, never clamped. Nothing else of the sound
  // changes; its effective gain follows (EffectiveVolume@1).
  'soundbox.setSoundVolume': async (server, { payload, targetId }) => {
    const id = targetId || payload?.sound;
    const fixtures = (await Promise.all(['soundbox.listSounds', 'sounds'].map((name) => server.load(name)))).filter(Boolean);
    const found = fixtures.map((fixture) => fixture.sounds.find((candidate) => candidate.id === id)).filter(Boolean);
    if (!found.length) return ['not-found'];
    const volume = payload?.volume;
    if (typeof volume !== 'number' || !(volume >= 0 && volume <= 100)) return ['sound-volume-range'];
    for (const sound of found) sound.volume = volume;
    return undefined;
  },
};
