import { soundTags } from './_lib/commands.js';

export const commands = {
  // soundbox.updateSound (crates/soundbox/data-capabilities/update-sound): the
  // name, tags and volume of one sound, tags normalised and volume ranged as
  // for an addition; a sound that is not there is `not-found`.
  'soundbox.updateSound': async (server, { payload, targetId }) => {
    const sound = (await server.load('soundbox.listSounds')).sounds.find((s) => s.id === targetId);
    if (!sound) return ['not-found'];
    if ('volume' in payload && !(Number(payload.volume) >= 0 && Number(payload.volume) <= 100)) return ['sound-volume-range'];
    if ('name' in payload) sound.name = String(payload.name);
    if ('tags' in payload) sound.tags = soundTags(payload.tags);
    if ('volume' in payload) sound.volume = Number(payload.volume);
    if ('file' in payload) sound.file = String(payload.file);
    return undefined;
  },
};
