// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

export const READS = {
  // soundbox.listSounds (crates/soundbox/capabilities/list-sounds): the
  // library as the fixture holds it, each sound with its effective gain —
  // EffectiveVolume@1, volume × master as a fraction — and whether a playback
  // of it runs (`server.playing`, which the soundbox's commands change).
  'soundbox.listSounds': async (server) => {
    const library = await server.load('soundbox.listSounds');
    if (!library) return undefined;
    const playing = (server.playing = server.playing || new Set());
    return {
      masterVolume: library.masterVolume,
      sounds: library.sounds.map((sound) => ({ ...sound, gain: Math.round(sound.volume * library.masterVolume) / 10000, playing: playing.has(sound.id) })),
    };
  },
};
