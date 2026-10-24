import { soundTags } from './_lib/commands.js';

export const commands = {
  // soundbox.addSound (crates/soundbox/data-capabilities/add-sound): a sound
  // added to the library. One with no name or no file is refused before
  // anything is queued, as the server refuses it. Its tags, a list of texts,
  // are normalised (`tags-normalized`); its own volume is a percentage from
  // 0 to 100 — a number or the text of one — full when none is given
  // (`sound-volume-range`).
  'soundbox.addSound': async (server, { payload }) => {
    const name = typeof payload.name === 'string' ? payload.name.trim() : '';
    const file = typeof payload.file === 'string' ? payload.file.trim() : '';
    if (!name || !file) return ['unprocessable'];
    const tags = payload.tags ?? [];
    if (!Array.isArray(tags) || !tags.every((t) => typeof t === 'string')) return ['tags-normalized'];
    const given = payload.volume ?? 100;
    const volume = typeof given === 'number' || (typeof given === 'string' && given.trim() !== '') ? Number(given) : NaN;
    if (!(volume >= 0 && volume <= 100)) return ['sound-volume-range'];
    const library = await server.load('soundbox.listSounds');
    library.sounds.unshift({ id: server.uuid(), name, tags: soundTags(tags), duration: 0, volume: Math.round(volume), file });
    return undefined;
  },
};
