// LiveSoundbox (system soundbox): the DM view's sound panel — sounds that
// layer over each other, one volume per sound, the master volume and
// « Tout arrêter ». It reads `soundbox.listSounds` and triggers
// `soundbox.playSound`, `soundbox.stopAll`, `soundbox.setSoundVolume` and
// `soundbox.setMasterVolume`, by identifier, through the shell it is handed:
// no network call of its own. Every sound is heard at the gain the Capability
// gives (EffectiveVolume@1), read again after each command, never worked out
// here: a sound played while a volume is still being set starts once that
// volume has been read back.

import { browserPlayer, type Player, type Voice } from './audio.ts';
import { CAMPAIGN_CONTEXT, LIST_SOUNDS, PLAY_SOUND, SET_MASTER_VOLUME, SET_SOUND_VOLUME, STOP_ALL } from './ids.ts';
import { allTags, commandBody, commandId, isFinal, parseLibrary, reasons, settled, tagKey, visible, type Settled } from './model.ts';
import { grid, playingCount, setSlider, skeleton, tagChips } from './render.ts';
import { STYLE } from './style.ts';
import type { Library, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 30 };

/** A sound heard on this view. */
interface Heard {
  voice: Voice;
  startedAt: number;
}

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="LiveSoundbox"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'LiveSoundbox';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the panel and wires it. */
export function mount(host: HTMLElement, _props: Record<string, unknown>, shell: Shell, player: Player = browserPlayer, timing: Timing = WAITING): { update(props?: Record<string, unknown>): void } {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`LiveSoundbox: #${id} is not in the panel`);
    return found;
  };
  const boxOf = (id: string): HTMLElement | undefined => [...host.querySelectorAll<HTMLElement>('[data-id]')].find((box) => box.dataset.id === id);

  let library: Library = parseLibrary(undefined);
  let query = '';
  const chosen = new Set<string>();
  const heard = new Map<string, Heard>();
  /** Plays asked of the server and not yet settled, by sound: what the library cannot show yet. */
  const asked = new Map<string, number>();
  /** The sounds the library last said were playing: a sound that starts playing elsewhere (the suggestion pill) is heard here. */
  let running = new Set<string>();
  let firstRead = true;
  let unreadable = false;
  let over = false;
  let dragging = false;
  let repaintLater = false;
  let loads = 0;
  let seen = -1;

  const campaign = (): Record<string, string> => {
    const id = shell.context.get(CAMPAIGN_CONTEXT);
    return typeof id === 'string' && id !== '' ? { campaign: id } : {};
  };

  const say = (message: string): void => {
    part('soundbox-message').textContent = message;
  };

  /** The bar of a playing sound fills over what is left of it. */
  const progress = (id: string): void => {
    const box = boxOf(id);
    const bar = box?.querySelector<HTMLElement>('.mf-live-sb__progress');
    if (!bar || typeof bar.animate !== 'function') return;
    for (const animation of bar.getAnimations()) animation.cancel();
    const now = heard.get(id);
    const duration = library.sounds.find((sound) => sound.id === id)?.duration;
    if (!now || duration === null || duration === undefined) return;
    const done = Math.min(1, (Date.now() - now.startedAt) / (duration * 1000));
    bar.animate([{ width: `${done * 100}%` }, { width: '100%' }], { duration: duration * 1000 * (1 - done), easing: 'linear' });
  };

  /** Redraws `region` and gives the focus back to the control that had it: a key press on a slider or a button must go on working. */
  const keepingFocus = (region: HTMLElement, draw: () => void): void => {
    const active = host.ownerDocument.activeElement as HTMLElement | null;
    const key = active && region.contains(active) ? (['play', 'volume', 'tag'] as const).map((name) => [name, active.dataset[name]] as const).find(([, value]) => value !== undefined) : undefined;
    draw();
    if (key) [...region.querySelectorAll<HTMLElement>(`[data-${key[0]}]`)].find((element) => element.dataset[key[0]] === key[1])?.focus();
  };

  const paintCount = (): void => {
    part('soundbox-playing-count').textContent = playingCount(heard.size);
  };
  const paintGrid = (): void => {
    // A slider being dragged stays in the DM's hand: the grid is redrawn when it is released.
    if (dragging) {
      repaintLater = true;
      return;
    }
    repaintLater = false;
    const region = part('soundbox-grid');
    keepingFocus(region, () => {
      region.innerHTML = grid(visible(library.sounds, query, chosen), new Set(heard.keys()));
    });
    for (const id of heard.keys()) progress(id);
    paintCount();
  };
  const paintBox = (id: string): void => {
    boxOf(id)?.classList.toggle('is-playing', heard.has(id));
    progress(id);
    paintCount();
  };
  const paintTags = (): void => {
    const tags = allTags(library.sounds);
    for (const tag of [...chosen]) if (!tags.includes(tag)) chosen.delete(tag);
    const region = part('soundbox-tags');
    keepingFocus(region, () => {
      region.innerHTML = tagChips(tags, chosen);
    });
  };
  /** The master slider follows the library, in place, unless the DM has a hand on it. */
  const paintMaster = (): void => {
    if (dragging) return;
    const slider = part<HTMLInputElement>('soundbox-master-volume');
    slider.disabled = false;
    setSlider(slider, library.masterVolume);
  };

  const hush = (id: string): void => {
    heard.get(id)?.voice.stop();
    heard.delete(id);
  };

  /** A sound starts to be heard here, at the last gain read. */
  function listen(id: string): Heard | undefined {
    const sound = library.sounds.find((candidate) => candidate.id === id);
    if (!sound) return undefined;
    hush(id);
    const now: Heard = {
      startedAt: Date.now(),
      voice: player(
        sound,
        sound.gain,
        () => {
          if (heard.get(id) !== now) return;
          heard.delete(id);
          paintBox(id);
        },
        () => {
          if (heard.get(id) !== now) return;
          heard.delete(id);
          paintBox(id);
          say('Le navigateur bloque le son : un clic sur la page l’autorise.');
        },
      ),
    };
    heard.set(id, now);
    paintBox(id);
    return now;
  }

  /** What the library says about each sound: the gain of those heard here, whether they still play, and those that began playing elsewhere. */
  const reconcile = (): void => {
    for (const [id, now] of [...heard]) {
      const sound = library.sounds.find((candidate) => candidate.id === id);
      if (!sound || (!sound.playing && (asked.get(id) ?? 0) === 0)) hush(id);
      else now.voice.setGain(sound.gain);
    }
    const playing = new Set(library.sounds.filter((sound) => sound.playing).map((sound) => sound.id));
    // What already played when this view opened was started before it: only a start seen since is heard.
    if (!firstRead) for (const id of playing) if (!running.has(id) && !heard.has(id) && (asked.get(id) ?? 0) === 0) listen(id);
    running = playing;
    firstRead = false;
  };

  /** The read asked last: an older read answers when it has, so that whoever waits for a read waits for the library as it now stands. */
  let newest: Promise<boolean> = Promise.resolve(true);
  function load(): Promise<boolean> {
    loads += 1;
    newest = read(loads);
    return newest;
  }

  async function read(mine: number): Promise<boolean> {
    try {
      const answer = await shell.capability(LIST_SOUNDS, undefined, campaign());
      if (over) return true;
      // A later read has been asked since: it is the one that counts.
      if (mine !== loads) return newest;
      library = parseLibrary(answer.data);
      seen = answer.asOf;
      reconcile();
      paintTags();
      paintGrid();
      paintMaster();
      if (unreadable) {
        unreadable = false;
        say('');
      }
      return true;
    } catch {
      unreadable = true;
      say('Impossible de lire les sons.');
      return false;
    }
  }

  /** One command at a time per target, so a command never queues behind this panel's own. */
  const lines = new Map<string, Promise<unknown>>();
  function inLine<T>(key: string, run: () => Promise<T>): Promise<T> {
    const result = (lines.get(key) ?? Promise.resolve()).then(run, run);
    lines.set(key, result.catch(() => undefined));
    return result;
  }

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, payload: Record<string, unknown>, targetId?: string): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, payload, targetId));
    const id = commandId(answer);
    let state = settled(answer);
    for (let polled = 0; !isFinal(state.status) && id !== '' && polled < timing.polls; polled += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  /**
   * Sends a command and reads the library again. It answers how the command
   * stands: `applied`, `refused` (rejected, cancelled, expired or not sent) or
   * `waiting` (the server has not settled it yet); the message says why when it
   * was not applied.
   */
  async function act(key: string, wrong: string, dataCapability: string, payload: Record<string, unknown>, targetId?: string): Promise<'applied' | 'refused' | 'waiting'> {
    let outcome: 'applied' | 'refused' | 'waiting' = 'refused';
    try {
      const state = await inLine(key, () => send(dataCapability, payload, targetId));
      if (state.status === 'applied') {
        outcome = 'applied';
        say('');
      } else if (isFinal(state.status)) say(`${wrong} : ${state.violations.length > 0 ? reasons(state.violations) : state.status}.`);
      else {
        outcome = 'waiting';
        say(`${wrong} : le serveur ne l’a pas encore pris en compte (état : ${state.status || 'inconnu'}).`);
      }
    } catch {
      say(`${wrong}.`);
    }
    await load();
    return outcome;
  }

  /** Volume commands asked of the server whose read of the library has not answered yet. */
  const volumes = new Set<Promise<unknown>>();
  function changingVolume(change: Promise<unknown>): void {
    const tracked: Promise<unknown> = change.then(() => {
      volumes.delete(tracked);
    });
    volumes.add(tracked);
  }
  async function volumesSettled(): Promise<void> {
    while (volumes.size > 0) await Promise.allSettled([...volumes]);
  }

  /** Plays the DM asked and that are not heard yet, because a volume was still being set: the one ticket of a sound that may start. */
  const starting = new Map<string, object>();

  function play(id: string): void {
    if (!library.sounds.some((candidate) => candidate.id === id)) return;
    const ticket = {};
    starting.set(id, ticket);
    let now: Heard | undefined;
    const begin = (): void => {
      if (over || starting.get(id) !== ticket) return;
      starting.delete(id);
      now = listen(id);
    };
    // The sound starts under the DM's hand, at the gain the library gives: when a volume is still being set, once that volume has been read back.
    const started = volumes.size === 0 ? Promise.resolve(begin()) : volumesSettled().then(begin);
    asked.set(id, (asked.get(id) ?? 0) + 1);
    const request = act('playback', 'Le son n’a pas pu être lancé', PLAY_SOUND, {}, id).then((outcome) => {
      // A play the server has not settled yet is still the DM's: it is heard until the library says otherwise.
      if (outcome !== 'refused') return;
      if (starting.get(id) === ticket) starting.delete(id);
      if (now && heard.get(id) === now) {
        hush(id);
        paintBox(id);
      }
    });
    void Promise.all([started, request]).finally(() => {
      asked.set(id, (asked.get(id) ?? 1) - 1);
    });
  }

  function stopAll(): void {
    starting.clear();
    for (const id of [...heard.keys()]) hush(id);
    paintGrid();
    // Behind any play already asked, so that the server stops what the DM started.
    void act('playback', 'Impossible de tout arrêter', STOP_ALL, campaign());
  }

  host.addEventListener('input', (event) => {
    const target = event.target as { id?: string; type?: string; value?: string; dataset?: Record<string, string | undefined> } | null;
    if (target?.id === 'soundbox-search-input') {
      query = target.value ?? '';
      paintGrid();
    } else if (target?.type === 'range') {
      dragging = true;
      const slider = event.target as HTMLInputElement;
      if (target.dataset?.volume !== undefined) {
        const sound = library.sounds.find((candidate) => candidate.id === target.dataset?.volume);
        if (sound) sound.volume = Number(slider.value);
        const readout = slider.nextElementSibling;
        if (readout) readout.textContent = slider.value;
      } else {
        // The kit's Slider: the value and the filled part of the track follow the thumb.
        slider.style.setProperty('--fill', `${slider.value}%`);
        const out = slider.closest('.kit-slider__field')?.querySelector('.kit-slider__out');
        if (out) out.textContent = slider.value;
      }
    }
  });

  host.addEventListener('change', (event) => {
    const slider = event.target as HTMLInputElement | null;
    if (slider?.type !== 'range') return;
    dragging = false;
    const volume = Number(slider.value);
    const id = slider.dataset.volume;
    if (id !== undefined) changingVolume(act(`volume:${id}`, 'Le volume du son n’a pas pu être changé', SET_SOUND_VOLUME, { volume }, id));
    else if (slider.name === 'master') changingVolume(act('master', 'Le volume global n’a pas pu être changé', SET_MASTER_VOLUME, { ...campaign(), volume }));
    // The grid that waited for the drag to end is drawn now, whatever the command answers.
    if (repaintLater) paintGrid();
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes === PLAY_SOUND && button.dataset.play !== undefined) play(button.dataset.play);
    else if (button.dataset.writes === STOP_ALL) stopAll();
    else if (button.dataset.tag !== undefined) {
      const tag = tagKey(button.dataset.tag);
      if (!chosen.delete(tag)) chosen.add(tag);
      paintTags();
      paintGrid();
    }
  });

  paintCount();
  void load();

  /** What the panel listens to; kept before the listening starts, so that `end` may run from inside a listener. */
  const stopWatching: Array<() => void> = [];
  // The library moves with the data (a stop from another tab, a volume changed elsewhere) and with the campaign.
  stopWatching.push(
    shell.onDataVersion((dataVersion) => {
      if (!host.isConnected) return end();
      if (dataVersion !== seen) void load();
    }),
  );
  stopWatching.push(
    shell.context.on((name) => {
      if (!host.isConnected) return end();
      if (name !== CAMPAIGN_CONTEXT) return;
      // Another campaign is another library: what plays here is not its sounds, and what already plays there was not started by this view.
      starting.clear();
      for (const id of [...heard.keys()]) hush(id);
      chosen.clear();
      running = new Set();
      firstRead = true;
      void load();
    }),
  );
  /** The panel left the page: its sounds stop with it. */
  function end(): void {
    over = true;
    starting.clear();
    for (const stop of stopWatching) stop();
    for (const id of [...heard.keys()]) hush(id);
  }

  // A drag that ends where it began fires no `change`: the slider is let go all the same.
  const release = (): void => {
    dragging = false;
  };
  host.addEventListener('pointerup', release);
  host.addEventListener('pointercancel', release);
  host.addEventListener('keyup', release);
  host.addEventListener('focusout', () => {
    release();
    if (repaintLater) paintGrid();
  });
  return { update: () => void load() };
}
