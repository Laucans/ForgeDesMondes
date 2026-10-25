// The dictation the shell hands a Micro-UI that listens (`mount`'s fifth
// argument): the DM's microphone, as audio. The shell hears no words itself —
// `transcript()` is always empty — and the audio goes to the server inside the
// Micro-UI's own command, for the speech-to-text port to transcribe on the
// DM's machine.
//
// Nothing happens until a Micro-UI calls `start()`, so a Micro-UI that ignores
// the dictation never asks for the microphone.
//
// With no microphone to record from — the browser has none, the DM refused it
// — `audio()` still hands over a take: the bytes of the Micro-UI's id, never
// empty, so the port is always asked. A real engine hears no words in it and
// the command's own refusal tells the DM so.

/** A recorder over the microphone: every chunk of sound goes to `start`'s callback; `stop` resolves once the last has. */
export interface Recording {
  start(onChunk: (chunk: Blob) => void): void;
  stop(): Promise<void>;
}

/** What a recording needs from the browser: a microphone, and a recorder over it. */
export interface Microphone {
  /** Rejects when there is no microphone or the DM refuses it. */
  open(): Promise<{ recorder: Recording; release(): void }>;
}

/** The browser's own microphone; null where `getUserMedia` does not exist. */
export function browserMicrophone(nav: Pick<Navigator, 'mediaDevices'> | undefined = typeof navigator === 'undefined' ? undefined : navigator): Microphone | null {
  const devices: MediaDevices | undefined = nav?.mediaDevices;
  if (!devices || typeof devices.getUserMedia !== 'function' || typeof MediaRecorder === 'undefined') return null;
  return {
    async open() {
      const stream = await devices.getUserMedia({ audio: true });
      const media = new MediaRecorder(stream);
      const recorder: Recording = {
        start(onChunk) {
          media.ondataavailable = (event) => onChunk(event.data);
          media.start();
        },
        stop: () =>
          new Promise<void>((resolve) => {
            media.onstop = () => resolve();
            media.stop();
          }),
      };
      return { recorder, release: () => stream.getTracks().forEach((track) => track.stop()) };
    },
  };
}

/** Where dictated text comes from, as the Micro-UIs declare it. */
export interface Dictation {
  transcript(): string;
  start(): void;
  audio(): Promise<string>;
}

const base64 = (bytes: Uint8Array): string => {
  let binary = '';
  for (let at = 0; at < bytes.length; at += 0x8000) binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(binary);
};

/** The no-device take of `microUi`: its id as bytes, base64. */
export const noDeviceTake = (microUi: string): string => base64(new TextEncoder().encode(microUi));

/** A take in progress: the recorder opened, or null when no microphone answered. */
type Take = Promise<{ recorder: Recording; release(): void; chunks: Blob[] } | null>;

export function createDictation(microUi: string, microphone: Microphone | null = browserMicrophone()): Dictation {
  let take: Take | null = null;

  const begin = async (): Promise<Awaited<Take>> => {
    if (!microphone) return null;
    try {
      const open = await microphone.open();
      const chunks: Blob[] = [];
      open.recorder.start((chunk) => chunks.push(chunk));
      return { ...open, chunks };
    } catch {
      return null;
    }
  };

  return {
    transcript: () => '',
    start() {
      // A take already in progress goes on: one click, one take.
      take ??= begin();
    },
    async audio() {
      const current = take;
      take = null;
      if (!current) return '';
      const opened = await current;
      if (!opened) return noDeviceTake(microUi);
      await opened.recorder.stop();
      opened.release();
      const bytes = new Uint8Array(await new Blob(opened.chunks).arrayBuffer());
      // A take that recorded no sound is no take: the port is not asked to hear nothing.
      return bytes.length === 0 ? '' : base64(bytes);
    },
  };
}
