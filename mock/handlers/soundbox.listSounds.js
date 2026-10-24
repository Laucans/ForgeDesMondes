export const reads = {
  // soundbox.listSounds (crates/soundbox/capabilities/list-sounds): the
  // campaign's sounds with their effective gain — the own volume times the
  // master volume, both percents (EffectiveVolume@1) — and whether one plays
  // now (the fixture's `playing`, which soundbox.playSound and soundbox.stopAll move). The optional `search` keeps the
  // names that hold it and `tag` the sounds that carry it, whatever the case
  // (SoundTag@1); the fixture's `duration` is kept, the store has none yet.
  'soundbox.listSounds': async (server, { search, tag }) => {
    const { masterVolume, sounds } = await server.load('soundbox.listSounds');
    const lower = (text) => String(text ?? '').trim().toLowerCase();
    return {
      masterVolume,
      sounds: sounds
        .filter((s) => lower(s.name).includes(lower(search)) && (!lower(tag) || s.tags.some((t) => lower(t) === lower(tag))))
        .map((s) => ({ ...s, gain: (s.volume * masterVolume) / 10000, playing: s.playing === true })),
    };
  },
};
