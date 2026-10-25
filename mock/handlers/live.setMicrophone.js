export const commands = {
  // live.setMicrophone (crates/live/data-capabilities/set-microphone): the DM's
  // microphone turned on or off in the campaign's session. `on` is a boolean,
  // never anything else. The mock-up has one session and no speech-to-text port:
  // the pill reads its feed from the fixture, so the toggle only has to be
  // accepted, and the pill refreshes.
  'live.setMicrophone': async (server, { payload }) => {
    if (typeof payload.on !== 'boolean') return ['unprocessable'];
    server.bump();
    return undefined;
  },
};
