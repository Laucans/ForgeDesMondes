import assert from 'node:assert/strict';
import { test } from 'node:test';
import { browserMicrophone, createDictation, noDeviceTake, type Microphone, type Recording } from './dictation.js';

const text = (encoded: string): string => Buffer.from(encoded, 'base64').toString('utf8');

/** A microphone whose recorder hands over `sound` when it stops, and notes what was done with it. */
function microphone(sound: string) {
  const log: string[] = [];
  let chunk: (blob: Blob) => void = () => undefined;
  const recorder: Recording = {
    start(onChunk) {
      log.push('start');
      chunk = onChunk;
    },
    async stop() {
      log.push('stop');
      chunk(new Blob([sound]));
    },
  };
  const mic: Microphone = {
    open: async () => {
      log.push('open');
      return { recorder, release: () => void log.push('release') };
    },
  };
  return { mic, log };
}

test('the no-device take is the Micro-UI\'s id, as base64', () => {
  assert.equal(noDeviceTake('OralBrief'), 'T3JhbEJyaWVm');
  assert.equal(text(noDeviceTake('OralBrief')), 'OralBrief');
});

test('the shell hears no words itself: the transcript is always empty', async () => {
  const { mic } = microphone('sound');
  const dictation = createDictation('OralBrief', mic);
  dictation.start();
  await dictation.audio();
  assert.equal(dictation.transcript(), '');
});

test('nothing asks for the microphone until a take starts', async () => {
  const { mic, log } = microphone('sound');
  const dictation = createDictation('OralBrief', mic);
  assert.deepEqual(log, []);
  assert.equal(await dictation.audio(), '');
  assert.deepEqual(log, []);
});

test('a take is recorded, stopped, released and handed over as base64, once', async () => {
  const { mic, log } = microphone('un son');
  const dictation = createDictation('OralBrief', mic);
  dictation.start();
  const heard = await dictation.audio();
  assert.equal(text(heard), 'un son');
  assert.deepEqual(log, ['open', 'start', 'stop', 'release']);
  assert.equal(await dictation.audio(), '');
});

test('a second start while a take goes on opens no second recorder', async () => {
  const { mic, log } = microphone('x');
  const dictation = createDictation('OralBrief', mic);
  dictation.start();
  dictation.start();
  await dictation.audio();
  assert.equal(log.filter((entry) => entry === 'open').length, 1);
});

test('a new take starts after the last was handed over', async () => {
  const { mic, log } = microphone('x');
  const dictation = createDictation('OralBrief', mic);
  dictation.start();
  await dictation.audio();
  dictation.start();
  await dictation.audio();
  assert.deepEqual(log.filter((entry) => entry === 'open').length, 2);
});

test('a recording that holds no sound gives no audio', async () => {
  const { mic } = microphone('');
  const dictation = createDictation('OralBrief', mic);
  dictation.start();
  assert.equal(await dictation.audio(), '');
});

test('with no microphone the take is the Micro-UI\'s id, whoever the Micro-UI is', async () => {
  for (const microUi of ['OralBrief', 'NoteImport']) {
    const dictation = createDictation(microUi, null);
    dictation.start();
    assert.equal(text(await dictation.audio()), microUi);
    assert.equal(await dictation.audio(), '', 'handed over once');
  }
});

test('a microphone the DM refuses gives the no-device take too', async () => {
  const refused: Microphone = { open: async () => Promise.reject(new Error('NotAllowedError')) };
  const dictation = createDictation('OralBrief', refused);
  dictation.start();
  assert.equal(text(await dictation.audio()), 'OralBrief');
});

test('a long take is encoded whole', async () => {
  const sound = 'a'.repeat(100_000);
  const { mic } = microphone(sound);
  const dictation = createDictation('OralBrief', mic);
  dictation.start();
  assert.equal(text(await dictation.audio()), sound);
});

test('a recorder already dead when the take ends does not hold the screen: the take stops and the stream is released', async () => {
  const stopped: string[] = [];
  const track = { stop: () => void stopped.push('track') };
  class DeadRecorder {
    state = 'inactive';
    ondataavailable: unknown = null;
    onstop: unknown = null;
    onerror: unknown = null;
    start(): void {}
    stop(): void {
      throw new Error('InvalidStateError');
    }
  }
  const original = (globalThis as { MediaRecorder?: unknown }).MediaRecorder;
  (globalThis as { MediaRecorder?: unknown }).MediaRecorder = DeadRecorder;
  try {
    const devices = { getUserMedia: async () => ({ getTracks: () => [track] }) } as unknown as MediaDevices;
    const mic = browserMicrophone({ mediaDevices: devices });
    assert.notEqual(mic, null);
    const dictation = createDictation('OralBrief', mic);
    dictation.start();
    assert.equal(await dictation.audio(), '');
    assert.deepEqual(stopped, ['track']);
  } finally {
    (globalThis as { MediaRecorder?: unknown }).MediaRecorder = original;
  }
});

test('the browser has a microphone only where getUserMedia is', () => {
  assert.equal(browserMicrophone(undefined), null);
  assert.equal(browserMicrophone({ mediaDevices: undefined as unknown as MediaDevices }), null);
  assert.equal(browserMicrophone({ mediaDevices: {} as MediaDevices }), null);
});
