import assert from 'node:assert/strict';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import { browserPlayer, soundUrl } from '../src/audio.ts';

/** A page whose media elements play (or refuse to) as told: jsdom plays nothing itself. */
function page(plays: boolean) {
  const dom = new JSDOM('<!doctype html><body></body>');
  const media = dom.window.HTMLMediaElement.prototype;
  media.play = function play(this: HTMLMediaElement) {
    return plays ? Promise.resolve() : Promise.reject(new Error('NotSupportedError'));
  };
  media.pause = () => undefined;
  Object.assign(globalThis, { document: dom.window.document });
  return { dom, audios: () => [...dom.window.document.querySelectorAll('audio')] };
}

test('a sound is an <audio> element of the page, at the gain it is given, until it is stopped', () => {
  const { audios } = page(true);
  const voice = browserPlayer({ file: 'porte qui grince.ogg', duration: 2 }, 0.64, () => assert.fail('a stopped sound does not end'), () => assert.fail('nor is it blocked'));
  assert.equal(audios().length, 1);
  assert.equal(audios()[0]?.volume, 0.64);
  assert.equal(audios()[0]?.getAttribute('src'), '/sounds/porte%20qui%20grince.ogg');
  voice.setGain(0.32);
  assert.equal(audios()[0]?.volume, 0.32);
  voice.stop();
  assert.equal(audios().length, 0);
});

test('a sound that ends on its own leaves the page and says so once', () => {
  const { dom, audios } = page(true);
  let ended = 0;
  browserPlayer({ file: 'a.ogg', duration: 1 }, 0.5, () => (ended += 1), () => assert.fail('not blocked'));
  audios()[0]?.dispatchEvent(new dom.window.Event('ended'));
  audios()[0]?.dispatchEvent(new dom.window.Event('ended'));
  assert.equal(ended, 1);
  assert.equal(audios().length, 0);
});

test('a file the browser cannot play is silence for as long as the sound lasts', async () => {
  const { audios } = page(false);
  let ended = 0;
  browserPlayer({ file: 'missing.ogg', duration: 0.02 }, 0.5, () => (ended += 1), () => assert.fail('a missing file is not a blocked one'));
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(ended, 0, 'not at once');
  assert.equal(audios().length, 1, 'the element stays while the silence lasts');
  await new Promise((resolve) => setTimeout(resolve, 60));
  assert.equal(ended, 1);
  assert.equal(audios().length, 0);
});

test('a file is asked of the shell by its name, each part of its path escaped', () => {
  assert.equal(soundUrl('a b/forêt?.ogg'), '/sounds/a%20b/for%C3%AAt%3F.ogg');
});

test('a browser that wants a gesture of the DM first is told so, and the sound leaves the page', async () => {
  const { dom, audios } = page(true);
  dom.window.HTMLMediaElement.prototype.play = () => Promise.reject(new dom.window.DOMException('blocked', 'NotAllowedError'));
  Object.assign(globalThis, { DOMException: dom.window.DOMException });
  const told: string[] = [];
  browserPlayer({ file: 'a.ogg', duration: 5 }, 0.5, () => told.push('ended'), () => told.push('blocked'));
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(told, ['blocked']);
  assert.equal(audios().length, 0);
});
