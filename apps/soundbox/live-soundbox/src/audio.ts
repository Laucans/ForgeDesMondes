// The sound itself. A Voice is one playback heard on this view; the DM view
// is the only place a Micro-UI of the soundbox is mounted (dm-side-only), so
// it is the only place a sound is made.

export interface Voice {
  /** The gain `soundbox.listSounds` gives, a fraction in [0, 1]. */
  setGain(gain: number): void;
  stop(): void;
}

/**
 * Starts a sound at `gain`. `onEnded` is told once, when it ends on its own;
 * `onBlocked` once, when the browser refuses to play without a gesture of the DM.
 */
export type Player = (sound: { file: string; duration: number | null }, gain: number, onEnded: () => void, onBlocked: () => void) => Voice;

/** Where the shell serves the library's files. */
export const soundUrl = (file: string): string => `/sounds/${file.split('/').map(encodeURIComponent).join('/')}`;

/**
 * The browser's own audio, one <audio> element in the page for each sound
 * heard (the page's audio elements carry the output volume). A file the
 * browser cannot fetch or play is heard as silence for as long as the sound
 * lasts, so that what the DM sees keeps the sound's length.
 */
export const browserPlayer: Player = (sound, gain, onEnded, onBlocked) => {
  const element = document.createElement('audio');
  element.src = soundUrl(sound.file);
  element.hidden = true;
  document.body.append(element);
  let over = false;
  let silent: ReturnType<typeof setTimeout> | undefined;
  const end = (): void => {
    if (over) return;
    over = true;
    clearTimeout(silent);
    element.remove();
    onEnded();
  };
  const fallBack = (): void => {
    if (over || silent !== undefined) return;
    silent = setTimeout(end, (sound.duration ?? 0) * 1000);
  };
  element.volume = gain;
  element.addEventListener('ended', end);
  element.addEventListener('error', fallBack);
  element.play().catch((error: unknown) => {
    if (error instanceof DOMException && error.name === 'NotAllowedError') {
      // Not an end: the sound never started.
      over = true;
      element.remove();
      onBlocked();
    } else fallBack();
  });
  return {
    setGain(next) {
      element.volume = next;
    },
    stop() {
      over = true;
      clearTimeout(silent);
      element.pause();
      element.remove();
    },
  };
};
