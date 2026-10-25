export const commands = {
  // soundbox.playSound (crates/soundbox/data-capabilities/play-sound): a
  // playback of the sound starts, whatever else plays (`simultaneous-playback`),
  // from the soundbox (the default) or a suggestion's pastille; any other
  // source, the player view's, is refused (`dm-side-only`). The sound plays
  // as soundbox.listSounds says it: `playing`.
  'soundbox.playSound': async (server, { payload, targetId }) => {
    const id = targetId || payload?.sound;
    const sound = (await server.load('soundbox.listSounds')).sounds.find((candidate) => candidate.id === id);
    if (!sound) return ['not-found'];
    const source = payload?.source ?? 'soundbox';
    if (!['soundbox', 'suggestion'].includes(source)) return ['dm-side-only'];
    sound.playing = true;
    return undefined;
  },
};
