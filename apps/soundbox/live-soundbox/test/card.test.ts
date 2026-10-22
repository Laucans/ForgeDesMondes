import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { JSDOM } from 'jsdom';
import type { Player } from '../src/audio.ts';
import { mount, type Timing } from '../src/index.ts';
import type { Shell } from '../src/types.ts';

interface Fixture {
  masterVolume: number;
  sounds: Array<{ id: string; name: string; tags: string[]; duration: number; volume: number; file: string }>;
}

/** The mock-up's fixture of the Capability: the library a panel is opened on. */
const fixture = JSON.parse(readFileSync(new URL('../../../../mock/fixtures/soundbox.listSounds.json', import.meta.url), 'utf8')) as Fixture;
const CROWD = 'snd-foule';
const DOOR = 'snd-porte';

interface Sent {
  dataCapability: string;
  payload: Record<string, unknown>;
  target?: { id: string };
  caller: { by: string };
}

interface Options {
  campaign?: string;
  /** What the server answers a command with; `applied` by default. */
  verdict?: (sent: Sent) => { status: string; violations?: string[] };
  unreadable?: boolean;
  /** Sounds a playback of which already runs when the panel opens. */
  playing?: string[];
  /** Commands wait for it before the server answers. */
  gate?: Promise<void>;
}

/**
 * A shell over a small server: it answers the library as the Capability does
 * (EffectiveVolume@1: volume × master, as a fraction) and applies a command
 * as the DataGuard would.
 */
function fakeShell(options: Options = {}) {
  const library = structuredClone(fixture);
  const playing = new Set<string>(options.playing);
  let unreadable = options.unreadable ?? false;
  const verdicts = new Map<string, { status: string; violations?: string[] }>();
  const sent: Sent[] = [];
  const reads: Array<Record<string, unknown> | undefined> = [];
  let version = 1;
  let onVersion: ((dataVersion: number) => void) | undefined;
  let onContext: ((name: string, value: unknown) => void) | undefined;
  let campaign = options.campaign;
  const apply = (command: Sent): void => {
    const id = command.target?.id;
    if (command.dataCapability === 'soundbox.playSound@1' && id) playing.add(id);
    if (command.dataCapability === 'soundbox.stopAll@1') playing.clear();
    if (command.dataCapability === 'soundbox.setSoundVolume@1') {
      const sound = library.sounds.find((candidate) => candidate.id === id);
      if (sound) sound.volume = command.payload.volume as number;
    }
    if (command.dataCapability === 'soundbox.setMasterVolume@1') library.masterVolume = command.payload.volume as number;
    version += 1;
  };
  const shell: Shell = {
    async capability(id, sha256, variables) {
      assert.equal(id, 'soundbox.listSounds');
      assert.equal(sha256, undefined, 'a Micro-UI names a Capability alone');
      reads.push(variables);
      if (unreadable) throw new Error('down');
      return {
        asOf: version,
        data: {
          masterVolume: library.masterVolume,
          sounds: library.sounds.map((s) => ({ ...s, gain: (s.volume * library.masterVolume) / 10000, playing: playing.has(s.id) })),
        },
      };
    },
    async command(body) {
      const command = body as Sent;
      sent.push(command);
      await options.gate;
      const verdict = options.verdict?.(command) ?? { status: 'applied' };
      if (verdict.status === 'applied') apply(command);
      verdicts.set(`c${sent.length}`, verdict);
      return { commandId: `c${sent.length}`, result: { status: verdict.status, violations: verdict.violations ?? [] } };
    },
    async commandResult(commandId) {
      const verdict = verdicts.get(commandId) ?? { status: 'applied' };
      return { result: { commandId, status: verdict.status, violations: verdict.violations ?? [] } };
    },
    onDataVersion(listener) {
      onVersion = listener;
      return () => {
        onVersion = undefined;
      };
    },
    context: {
      get: (name) => (name === 'campaign' ? campaign : undefined),
      on(listener) {
        onContext = listener;
        return () => {
          onContext = undefined;
        };
      },
    },
  };
  return {
    shell,
    sent,
    reads,
    playing,
    library,
    /** Another view changed the data. */
    elsewhere: (change: () => void) => {
      change();
      version += 1;
      onVersion?.(version);
    },
    setCampaign: (id: string) => {
      campaign = id;
      onContext?.('campaign', id);
    },
    listening: () => onVersion !== undefined,
    readable: (yes: boolean) => {
      unreadable = !yes;
    },
  };
}

/** Voices of a fake player: what is heard, at which gain. */
function fakePlayer() {
  const voices: Array<{ file: string; gain: number; stopped: boolean; end: () => void; block: () => void }> = [];
  const player: Player = (sound, gain, onEnded, onBlocked) => {
    const voice = {
      file: sound.file,
      gain,
      stopped: false,
      end: () => {
        voice.stopped = true;
        onEnded();
      },
      block: () => {
        voice.stopped = true;
        onBlocked();
      },
    };
    voices.push(voice);
    return {
      setGain(next) {
        voice.gain = next;
      },
      stop() {
        voice.stopped = true;
      },
    };
  };
  return { player, voices, heard: () => voices.filter((v) => !v.stopped) };
}

const timing: Timing = { sleep: async () => undefined, everyMs: 0, polls: 5 };

/** Lets every pending promise of the panel finish. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setImmediate(resolve));
};

async function panel(options?: Options) {
  const dom = new JSDOM('<!doctype html><html><head></head><body><div data-component="LiveSoundbox"></div></body></html>');
  const host = dom.window.document.querySelector<HTMLElement>('[data-component]') as HTMLElement;
  const fake = fakeShell(options);
  const audio = fakePlayer();
  const mounted = mount(host, {}, fake.shell, audio.player, timing);
  await settle();
  const $ = (selector: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(selector);
    assert.ok(found, `${selector} is on the panel`);
    return found;
  };
  const playingIds = (): string[] => [...host.querySelectorAll<HTMLElement>('.is-playing')].map((box) => box.dataset.id ?? '');
  const click = async (selector: string): Promise<void> => {
    $(selector).dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));
    await settle();
  };
  const play = (id: string): Promise<void> => click(`[data-play="${id}"]`);
  /** Moves a range to `value` and releases it. */
  const slide = async (selector: string, value: number): Promise<void> => {
    const slider = $(selector) as HTMLInputElement;
    slider.value = String(value);
    slider.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
    slider.dispatchEvent(new dom.window.Event('change', { bubbles: true }));
    await settle();
  };
  const type = (value: string): void => {
    const field = $('#soundbox-search-input') as HTMLInputElement;
    field.value = value;
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  };
  return { host, $, playingIds, click, play, slide, type, mounted, dom, ...fake, ...audio };
}

test('the panel draws the mock-up: search, master volume, « Tout arrêter », tags and one box per sound', async () => {
  const p = await panel();
  assert.equal(p.host.querySelectorAll('.mf-live-sb__box').length, fixture.sounds.length);
  assert.equal((p.$('#soundbox-master-volume') as HTMLInputElement).value, String(fixture.masterVolume));
  assert.equal(p.$('#soundbox-stop-all').textContent, 'Tout arrêter');
  assert.equal(p.$('#soundbox-tags').querySelectorAll('.chip').length > 3, true);
  assert.equal(p.$(`#soundbox-${DOOR} b`).textContent, 'Porte qui grince');
  assert.equal(p.$(`#soundbox-${DOOR} small`).textContent, 'porte · bois · intérieur · 2,4 s');
  assert.equal(p.$('#soundbox-playing-count').textContent, 'Le son sort uniquement de cette vue');
  assert.deepEqual(p.reads, [{}], 'one read, with no campaign while the screen has none');
});

test('every action has its actuator and every need its part, by identifier', async () => {
  const p = await panel();
  const writes = (id: string) => p.host.querySelectorAll(`[data-writes="${id}"]`).length;
  assert.equal(writes('soundbox.playSound'), fixture.sounds.length);
  assert.equal(writes('soundbox.setSoundVolume'), fixture.sounds.length);
  assert.equal(writes('soundbox.stopAll'), 1);
  assert.equal(writes('soundbox.setMasterVolume'), 1);
  assert.equal(p.host.querySelectorAll('[data-reads="soundbox.listSounds"]').length >= 1, true);
});

test('a sound played over another one layers: both are shown playing and both are heard', async () => {
  const p = await panel();
  await p.play(CROWD);
  await p.play(DOOR);
  assert.deepEqual(
    p.sent.map((c) => [c.dataCapability, c.target?.id, c.caller.by]),
    [
      ['soundbox.playSound@1', CROWD, 'dm'],
      ['soundbox.playSound@1', DOOR, 'dm'],
    ],
  );
  assert.deepEqual(p.playingIds().sort(), [CROWD, DOOR].sort());
  assert.deepEqual(p.heard().map((v) => [v.file, v.gain]), [['foule-taverne.ogg', 0.44], ['porte-qui-grince.ogg', 0.64]]);
  assert.equal(p.$('#soundbox-playing-count').textContent, '2 sons en cours · le son sort uniquement de cette vue');
});

test('the master volume released at 40 takes every playing sound to the gain the library now gives, their ratio kept', async () => {
  const p = await panel();
  await p.play(CROWD);
  await p.play(DOOR);
  await p.slide('#soundbox-master-volume', 40);
  assert.deepEqual(p.sent.at(-1), { dataCapability: 'soundbox.setMasterVolume@1', payload: { volume: 40 }, caller: { by: 'dm', from: 'LiveSoundbox', roles: [] } });
  const [crowd, door] = p.heard();
  assert.deepEqual([crowd?.gain, door?.gain], [0.22, 0.32]);
  assert.equal((p.$('#soundbox-master-volume') as HTMLInputElement).value, '40');
  assert.deepEqual(p.playingIds().sort(), [CROWD, DOOR].sort(), 'the sounds go on playing');
});

test('while the master is only being moved, nothing is sent and nothing changes', async () => {
  const p = await panel();
  await p.play(CROWD);
  const slider = p.$('#soundbox-master-volume') as HTMLInputElement;
  slider.value = '10';
  slider.dispatchEvent(new p.host.ownerDocument.defaultView!.Event('input', { bubbles: true }));
  await settle();
  assert.equal(p.sent.length, 1, 'only the play so far');
  assert.equal(p.heard()[0]?.gain, 0.44);
  assert.equal(p.$('.kit-slider__out').textContent, '10');
});

test('« Tout arrêter » silences every sound at once and tells the server', async () => {
  const p = await panel();
  await p.play(CROWD);
  await p.play(DOOR);
  await p.click('#soundbox-stop-all');
  assert.equal(p.sent.at(-1)?.dataCapability, 'soundbox.stopAll@1');
  assert.deepEqual(p.heard(), []);
  assert.deepEqual(p.playingIds(), []);
  assert.equal(p.$('#soundbox-playing-count').textContent, 'Le son sort uniquement de cette vue');
  assert.equal(p.playing.size, 0);
});

test('« Tout arrêter » is heard at once, before the server has answered', async () => {
  const p = await panel({ gate: new Promise<void>(() => undefined) });
  await p.play(CROWD);
  assert.equal(p.heard().length, 1, 'a sound starts before the server has answered');
  await p.click('#soundbox-stop-all');
  assert.deepEqual(p.heard(), []);
  assert.deepEqual(p.playingIds(), []);
});

test('a sound\'s own slider changes that sound alone, and its gain follows the master', async () => {
  const p = await panel();
  await p.play(DOOR);
  await p.slide(`[data-volume="${DOOR}"]`, 50);
  assert.deepEqual(p.sent.at(-1)?.payload, { volume: 50 });
  assert.equal(p.sent.at(-1)?.target?.id, DOOR);
  assert.equal(p.sent.at(-1)?.dataCapability, 'soundbox.setSoundVolume@1');
  assert.equal(p.heard()[0]?.gain, 0.4);
  assert.equal(p.library.sounds.find((s) => s.id === CROWD)?.volume, 55);
});

test('a play the server refuses is not heard, and says why', async () => {
  const p = await panel({ verdict: () => ({ status: 'rejected', violations: ['dm-side-only'] }) });
  await p.play(DOOR);
  assert.deepEqual(p.heard(), []);
  assert.deepEqual(p.playingIds(), []);
  assert.match(p.$('#soundbox-message').textContent ?? '', /seule la DM view joue un son/);
});

test('a sound that ends on its own is no longer shown playing', async () => {
  const p = await panel();
  await p.play(DOOR);
  p.voices[0]?.end();
  assert.deepEqual(p.playingIds(), []);
  assert.equal(p.$('#soundbox-playing-count').textContent, 'Le son sort uniquement de cette vue');
});

test('a sound played again starts over: one voice of it at a time', async () => {
  const p = await panel();
  await p.play(DOOR);
  await p.play(DOOR);
  assert.equal(p.voices.length, 2);
  assert.equal(p.heard().length, 1);
});

test('a stop made elsewhere silences this view when the data moves', async () => {
  const p = await panel();
  await p.play(CROWD);
  p.elsewhere(() => p.playing.clear());
  await settle();
  assert.deepEqual(p.heard(), []);
  assert.deepEqual(p.playingIds(), []);
});

test('the search and the tag filter narrow the boxes, and the sounds playing go on', async () => {
  const p = await panel();
  await p.play(CROWD);
  p.type('porte');
  assert.deepEqual([...p.host.querySelectorAll<HTMLElement>('.mf-live-sb__box')].map((b) => b.dataset.id), [DOOR]);
  p.type('');
  await p.click('#soundbox-tag-combat');
  assert.deepEqual([...p.host.querySelectorAll<HTMLElement>('.mf-live-sb__box')].map((b) => b.dataset.id), ['snd-epees', 'snd-fleche', 'snd-boule-feu']);
  assert.equal(p.$('#soundbox-tag-combat').getAttribute('aria-pressed'), 'true');
  p.type('zzz');
  assert.equal(p.$('#soundbox-grid').textContent, 'Aucun son ne correspond.');
  assert.equal(p.heard().length, 1, 'filtering is not stopping');
  p.type('');
  await p.click('#soundbox-tag-combat');
  assert.equal(p.host.querySelectorAll('.mf-live-sb__box').length, fixture.sounds.length);
  assert.deepEqual(p.playingIds(), [CROWD], 'the sound still plays when its box is back');
});

test('the campaign of the screen is named in the read and in the commands that need it', async () => {
  const p = await panel({ campaign: 'cendres-valombre' });
  assert.deepEqual(p.reads, [{ campaign: 'cendres-valombre' }]);
  await p.slide('#soundbox-master-volume', 60);
  assert.deepEqual(p.sent.at(-1)?.payload, { campaign: 'cendres-valombre', volume: 60 });
  await p.click('#soundbox-stop-all');
  assert.deepEqual(p.sent.at(-1)?.payload, { campaign: 'cendres-valombre' });
  const before = p.reads.length;
  p.setCampaign('autre');
  await settle();
  assert.deepEqual(p.reads.at(-1), { campaign: 'autre' });
  assert.equal(p.reads.length > before, true);
});

test('a library that cannot be read says so, and still offers « Tout arrêter »', async () => {
  const p = await panel({ unreadable: true });
  assert.match(p.$('#soundbox-message').textContent ?? '', /Impossible de lire les sons/);
  assert.equal(p.$('#soundbox-stop-all').textContent, 'Tout arrêter');
  assert.equal((p.$('#soundbox-master-volume') as HTMLInputElement).disabled, true);
});

test('a panel that left the page stops its sounds and stops listening', async () => {
  const p = await panel();
  await p.play(CROWD);
  p.host.remove();
  p.elsewhere(() => undefined);
  await settle();
  assert.deepEqual(p.heard(), []);
  assert.equal(p.listening(), false);
});

test('a shell that tells the data version the moment it is listened to does not break a panel that is not on the page', async () => {
  const dom = new JSDOM('<!doctype html><html><head></head><body></body></html>');
  const host = dom.window.document.createElement('div');
  const fake = fakeShell();
  const eager: Shell = { ...fake.shell, onDataVersion: (listener) => (listener(1), () => undefined) };
  const audio = fakePlayer();
  assert.doesNotThrow(() => mount(host, {}, eager, audio.player, timing));
  await settle();
  assert.deepEqual(audio.heard(), []);
});

test('a sound that starts playing elsewhere, from the suggestion pill, is heard here', async () => {
  const p = await panel();
  p.elsewhere(() => p.playing.add(DOOR));
  await settle();
  assert.deepEqual(p.heard().map((v) => [v.file, v.gain]), [['porte-qui-grince.ogg', 0.64]]);
  assert.deepEqual(p.playingIds(), [DOOR]);
  assert.equal(p.sent.length, 0, 'this view asked for nothing');
});

test('what already plays when the view opens is not started again', async () => {
  const p = await panel({ playing: [CROWD] });
  assert.deepEqual(p.heard(), []);
  assert.deepEqual(p.playingIds(), []);
  p.elsewhere(() => p.playing.add(DOOR));
  await settle();
  assert.deepEqual(p.heard().map((v) => v.file), ['porte-qui-grince.ogg'], 'only the sound that began since');
});

test('« Tout arrêter » is sent after the plays asked before it, so the server stops them', async () => {
  let open: () => void = () => undefined;
  const p = await panel({ gate: new Promise<void>((resolve) => (open = resolve)) });
  await p.play(CROWD);
  await p.click('#soundbox-stop-all');
  assert.deepEqual(p.sent.map((c) => c.dataCapability), ['soundbox.playSound@1'], 'the stop waits in line');
  open();
  await settle();
  assert.deepEqual(p.sent.map((c) => c.dataCapability), ['soundbox.playSound@1', 'soundbox.stopAll@1']);
});

test('a master volume the server refuses goes back to the volume it holds', async () => {
  const p = await panel({ verdict: () => ({ status: 'rejected', violations: ['master-volume-range'] }) });
  const slider = p.$('#soundbox-master-volume') as HTMLInputElement;
  await p.slide('#soundbox-master-volume', 150);
  assert.equal(p.$('#soundbox-master-volume'), slider, 'the slider is redrawn in place, its focus kept');
  assert.equal(slider.value, String(fixture.masterVolume));
  assert.equal(p.$('.kit-slider__out').textContent, String(fixture.masterVolume));
  assert.match(p.$('#soundbox-message').textContent ?? '', /le volume global reste entre 0 et 100 %/);
});

test('a slider let go where it began does not hold the grid back', async () => {
  const p = await panel();
  const slider = p.$(`[data-volume="${DOOR}"]`) as HTMLInputElement;
  slider.dispatchEvent(new p.dom.window.Event('input', { bubbles: true }));
  p.elsewhere(() => {
    const sound = p.library.sounds.find((s) => s.id === DOOR);
    if (sound) sound.volume = 10;
  });
  await settle();
  assert.equal(p.$(`[data-volume="${DOOR}"]`), slider, 'the slider in the DM\'s hand is not replaced');
  slider.dispatchEvent(new p.dom.window.Event('pointerup', { bubbles: true }));
  p.elsewhere(() => undefined);
  await settle();
  assert.equal((p.$(`[data-volume="${DOOR}"]`) as HTMLInputElement).value, '10');
});

test('a redraw gives the focus back to the control that had it', async () => {
  const p = await panel();
  p.host.ownerDocument.body.append(p.host);
  (p.$(`[data-volume="${DOOR}"]`) as HTMLInputElement).focus();
  p.elsewhere(() => undefined);
  await settle();
  assert.equal((p.host.ownerDocument.activeElement as HTMLElement | null)?.dataset.volume, DOOR);
  await p.click('#soundbox-tag-combat');
  p.$('#soundbox-tag-combat').focus();
  p.elsewhere(() => undefined);
  await settle();
  assert.equal((p.host.ownerDocument.activeElement as HTMLElement | null)?.dataset.tag, 'combat');
});

test('the message of a library that could not be read goes once it can', async () => {
  const p = await panel({ unreadable: true });
  assert.match(p.$('#soundbox-message').textContent ?? '', /Impossible de lire les sons/);
  p.readable(true);
  p.elsewhere(() => undefined);
  await settle();
  assert.equal(p.$('#soundbox-message').textContent, '');
  assert.equal(p.host.querySelectorAll('.mf-live-sb__box').length, fixture.sounds.length);
  assert.equal((p.$('#soundbox-master-volume') as HTMLInputElement).disabled, false);
});

test('the panel reads the library again when it is told its props moved', async () => {
  const p = await panel();
  const before = p.reads.length;
  p.mounted.update({});
  await settle();
  assert.equal(p.reads.length, before + 1);
});

test('a browser that blocks a sound started from elsewhere says so instead of showing it playing', async () => {
  const p = await panel();
  p.elsewhere(() => p.playing.add(DOOR));
  await settle();
  p.voices[0]?.block();
  assert.deepEqual(p.playingIds(), []);
  assert.match(p.$('#soundbox-message').textContent ?? '', /Le navigateur bloque le son/);
});

test('a play the server has not settled yet is heard until the library says otherwise', async () => {
  const p = await panel({ verdict: () => ({ status: 'queued' }) });
  await p.play(DOOR);
  assert.equal(p.heard().length, 1);
  assert.match(p.$('#soundbox-message').textContent ?? '', /pas encore pris en compte \(état : queued\)/);
});

test('another campaign is another library: its running sounds are not started here, the tag filter is cleared', async () => {
  const p = await panel({ campaign: 'a' });
  await p.play(CROWD);
  await p.click('#soundbox-tag-combat');
  p.playing.add(DOOR);
  p.setCampaign('b');
  await settle();
  assert.deepEqual(p.heard(), [], 'the sounds of the other campaign stop, those already running there are not started');
  assert.equal(p.host.querySelectorAll('.mf-live-sb__box').length, fixture.sounds.length, 'the filter went with the campaign');
});

test('a panel that left the page starts nothing, whatever answers late', async () => {
  const p = await panel();
  p.host.remove();
  p.elsewhere(() => undefined);
  await settle();
  p.elsewhere(() => p.playing.add(DOOR));
  p.mounted.update({});
  await settle();
  assert.deepEqual(p.voices.length, 0);
});

test('a slider whose drag is cancelled is let go', async () => {
  const p = await panel();
  const slider = p.$(`[data-volume="${DOOR}"]`);
  slider.dispatchEvent(new p.dom.window.Event('input', { bubbles: true }));
  slider.dispatchEvent(new p.dom.window.Event('pointercancel', { bubbles: true }));
  p.elsewhere(() => {
    const sound = p.library.sounds.find((s) => s.id === DOOR);
    if (sound) sound.volume = 5;
  });
  await settle();
  assert.equal((p.$(`[data-volume="${DOOR}"]`) as HTMLInputElement).value, '5');
});

test('the grid that waited for a drag is drawn when the slider is released, before the server has answered', async () => {
  const p = await panel({ gate: new Promise<void>(() => undefined) });
  const slider = p.$(`[data-volume="${DOOR}"]`) as HTMLInputElement;
  slider.dispatchEvent(new p.dom.window.Event('input', { bubbles: true }));
  p.elsewhere(() => {
    const sound = p.library.sounds.find((s) => s.id === CROWD);
    if (sound) sound.volume = 5;
  });
  await settle();
  assert.equal((p.$(`[data-volume="${CROWD}"]`) as HTMLInputElement).value, '55', 'held back while the slider is in hand');
  slider.dispatchEvent(new p.dom.window.Event('change', { bubbles: true }));
  await settle();
  assert.equal((p.$(`[data-volume="${CROWD}"]`) as HTMLInputElement).value, '5');
});
