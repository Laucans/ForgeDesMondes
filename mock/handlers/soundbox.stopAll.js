export const commands = {
  // soundbox.stopAll (crates/soundbox/data-capabilities/stop-all): every
  // running playback gets its stop (`stop-all`); with none running it still
  // succeeds and changes nothing.
  'soundbox.stopAll': async (server) => {
    for (const sound of (await server.load('soundbox.listSounds')).sounds) sound.playing = false;
    return undefined;
  },
};
