export const reads = {
  // live.getTranscriptionVocabulary (crates/live/capabilities/get-transcription-vocabulary):
  // the words the microphone listens for — every name and alias of the campaign's
  // characters, places and events, then the name of each of its sounds, each term as
  // written and listed once. A campaign other than the mock-up's has none.
  'live.getTranscriptionVocabulary': async (server, { campaign } = {}) => {
    const { campaign: own, characters = [], places = [], events = [] } = await server.load('entities');
    if (campaign && campaign !== own.id) return { terms: [] };
    const { sounds = [] } = await server.load('sounds');
    const terms = [...characters, ...places, ...events].flatMap((e) => [e.name, ...(e.aliases || [])]);
    return { terms: [...new Set([...terms, ...sounds.map((s) => s.name)])] };
  },
};
