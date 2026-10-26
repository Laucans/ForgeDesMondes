// SuggestionPill (system live): the pill fixed at the bottom right of the DM
// view. It shows the 5 elements or sounds most recently heard at the table
// (live.listSuggestions), their history, and the microphone's on/pause toggle
// (live.setMicrophone), refreshed while the microphone is on. One click on an
// element asks the host to open it in the centre; a sound is armed by a first
// click and played by the second (soundbox.playSound). It reads and triggers
// them by identifier, through the shell it is handed: no network call of its
// own. The microphone button is the only way to start listening, and the pill
// never touches a sound card nor sends audio (live-only-local-audio): the
// transcription runs on the server's side of the machine, behind the command.

import { BUS_OPEN, LIST_SUGGESTIONS, PLAY_SOUND, SET_MICROPHONE, SOUND_SOURCE } from './ids.ts';
import { arrivals, commandBody, commandId, hostType, isAnswer, isFinal, keyOf, parseProps, parseSuggestions, reasons, settled, without, type Settled } from './model.ts';
import { MIC_LABEL, MIC_TEXTS, rows, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { MicState, Props, Shell, Suggestion, Suggestions, Timing } from './types.ts';

export type { Shell, Timing } from './types.ts';

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 1000, beatMs: 600 };

/** What the shell keeps of a mounted Micro-UI: new props follow the screen's context. */
export interface Mounted {
  update(raw: Record<string, unknown>): void;
}

const EMPTY_STACK = 'Activez le micro : les entités et les sons nommés apparaîtront ici.';
const EMPTY_HISTORY = 'Aucune suggestion pour l’instant.';

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="SuggestionPill"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'SuggestionPill';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the pill and wires it. */
export function mount(host: HTMLElement, raw: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): Mounted {
  const doc = host.ownerDocument;
  ensureStyle(doc);
  host.innerHTML = skeleton();

  let props: Props = parseProps(raw);
  let suggestions: Suggestions = parseSuggestions(undefined);
  /** Every detection a read has listed, and whether a read has yet: what the first one lists is already there, what comes after is an arrival. */
  let seen = new Set<string>();
  let seeded = false;
  /** The arrivals waiting for their turn on top, oldest first: the pill shows none of them yet. */
  let pending: Suggestion[] = [];
  let revealing = false;
  /** Starts the arrivals over: another campaign's are not this one's. */
  const forgetArrivals = (): void => {
    seen = new Set();
    seeded = false;
    pending = [];
    revealing = false;
  };
  let mic: MicState = 'idle';
  let busy = false;
  /** The sound a first click is waiting on: the second one plays it. */
  let armed: string | null = null;
  let generation = 0;
  let readNo = 0;
  let stopWatching: () => void = () => undefined;

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`SuggestionPill: #${id} is not on the screen`);
    return found;
  };

  /** The variables that name the campaign, when the screen gave it: the server's live session decides otherwise. */
  const scope = (): { campaign?: string } => (props.campaign === '' ? {} : { campaign: props.campaign });

  // ---------- painting ----------
  /** Who the status line speaks for: a message is taken down by its own source only, so a sound that plays does not hide the microphone's refusal. */
  let speaker: 'read' | 'mic' | 'sound' | '' = '';
  function say(text: string, from: 'read' | 'mic' | 'sound'): void {
    if (text === '' && speaker !== from) return;
    const box = part('suggestions-status');
    box.textContent = text;
    box.hidden = text === '';
    speaker = text === '' ? '' : from;
  }

  /** What the pill shows: the suggestions, less the arrivals still waiting. */
  const shown = (): Suggestions => without(suggestions, pending);

  function paintHeard(): void {
    const newest = shown().stack[0];
    part('suggestions-heard').textContent = mic === 'recording' ? (newest ? `« ${newest.via} »` : 'J’écoute…') : mic === 'paused' ? 'Micro en pause — rien n’est écouté.' : '';
  }

  function paintLists(): void {
    // A list drawn anew must not take the keyboard's place from the row it was on.
    // The rows are drawn in turn, so a row is found again by what it shows, never by its place.
    const active = doc.activeElement;
    const on = active !== null && host.contains(active) ? active.closest('button[data-kind]') : null;
    const focused = on === null ? null : { list: on.closest('ol')?.id ?? '', kind: on.getAttribute('data-kind') ?? '', id: on.getAttribute('data-id') ?? '' };
    const view = shown();
    part('suggestions-stack').innerHTML = rows(view.stack, 'suggestion', EMPTY_STACK, armed);
    part('suggestions-history-list').innerHTML = rows(view.history, 'suggestion-history', EMPTY_HISTORY, armed);
    if (focused !== null) {
      const buttons = [...host.querySelectorAll<HTMLElement>(`#${focused.list} button[data-kind]`)];
      buttons.find((r) => r.dataset.kind === focused.kind && r.dataset.id === focused.id)?.focus();
    }
    paintHeard();
  }

  function paintMic(): void {
    const box = part('suggestions-mic-box');
    box.setAttribute('data-state', mic);
    const words = box.querySelector('.kit-micbutton__state');
    if (words) words.textContent = MIC_TEXTS[mic];
    const button = part<HTMLButtonElement>('suggestions-mic-button');
    button.setAttribute('aria-pressed', String(mic === 'recording'));
    button.setAttribute('aria-label', `${MIC_LABEL} — ${MIC_TEXTS[mic]}`);
    button.disabled = busy;
    paintHeard();
  }

  /**
   * The arrivals come on top one at a time, in the order they were heard, each for a beat: the first at once,
   * and a read that lists several (the data is asked once a second) does not hide the ones in between.
   */
  async function reveal(): Promise<void> {
    if (revealing) return;
    revealing = true;
    const mine = generation;
    try {
      while (pending.length > 0 && mine === generation && host.isConnected) {
        pending = pending.slice(1);
        paintLists();
        await timing.sleep(timing.beatMs);
      }
    } finally {
      // Another campaign has started its own turn by now: it is not this one's to end.
      if (mine === generation) revealing = false;
    }
  }

  // ---------- reading ----------
  /** Reads the suggestions again. An older answer never replaces a newer one. */
  async function refresh(): Promise<void> {
    const mine = generation;
    const no = (readNo += 1);
    try {
      const read = await shell.capability(LIST_SUGGESTIONS, undefined, scope());
      if (mine !== generation || no < readNo) return;
      suggestions = parseSuggestions(read.data);
      const listed = new Set(suggestions.history.map(keyOf));
      // What a read no longer lists is not waiting any more; what it lists for the first time (after the first read) takes its turn.
      pending = pending.filter((s) => listed.has(keyOf(s)));
      if (seeded) pending = [...pending, ...arrivals(suggestions.history, seen)];
      seeded = seeded || isAnswer(read.data);
      for (const key of listed) seen.add(key);
      say('', 'read');
      // A sound that is no longer listed is not waiting for a second click.
      if (armed !== null && !suggestions.history.some((s) => s.kind === 'sound' && s.id === armed)) armed = null;
      paintLists();
      void reveal();
    } catch {
      if (mine !== generation || no < readNo) return;
      say('Impossible de lire les suggestions.', 'read');
    }
  }

  /** While the microphone is on, the suggestions are read again each time the data moves. */
  function watch(): void {
    stopWatching();
    stopWatching = shell.onDataVersion(() => {
      if (host.isConnected) void refresh();
      else stopWatching();
    }, timing.watchMs);
  }

  // ---------- commands ----------
  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, payload: Record<string, unknown>, targetId?: string): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, payload, targetId));
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  const why = (state: Settled): string => (state.violations.length > 0 ? reasons(state.violations) : isFinal(state.status) ? state.status : `état : ${state.status || 'inconnu'}`);

  /** The microphone's toggle: the button shows what was asked at once, and goes back when the server refuses. */
  async function toggleMic(): Promise<void> {
    if (busy) return;
    const before = mic;
    const on = before !== 'recording';
    busy = true;
    mic = on ? 'recording' : 'paused';
    paintMic();
    try {
      const state = await send(SET_MICROPHONE, { on });
      if (isFinal(state.status) && state.status !== 'applied') {
        mic = before;
        say(`Le micro n’a pas changé : ${why(state)}.`, 'mic');
      } else {
        // A command the server has not settled yet (queued behind another, parked) is still the DM's: it will apply, and the pill follows it.
        say(state.status === 'applied' ? '' : `Le micro : le serveur ne l’a pas encore pris en compte (${why(state)}).`, 'mic');
        if (on) watch();
        else stopWatching();
      }
    } catch {
      mic = before;
      say('La commande du micro n’a pas pu être envoyée.', 'mic');
    } finally {
      busy = false;
      paintMic();
    }
    await refresh();
  }

  async function play(id: string, name: string): Promise<void> {
    try {
      const state = await send(PLAY_SOUND, { source: SOUND_SOURCE }, id);
      say(state.status === 'applied' ? '' : `« ${name} » n’a pas pu être joué : ${why(state)}.`, 'sound');
    } catch {
      say(`« ${name} » n’a pas pu être joué.`, 'sound');
    }
  }

  // ---------- the DM's clicks ----------
  host.addEventListener('click', (event) => {
    const target = event.target as Element | null;
    if (target?.closest('#suggestions-history-toggle')) {
      const history = part('suggestions-history');
      history.hidden = !history.hidden;
      part('suggestions-history-toggle').setAttribute('aria-expanded', String(!history.hidden));
      return;
    }
    if (target?.closest('#suggestions-mic-button')) {
      void toggleMic();
      return;
    }
    const row = target?.closest<HTMLElement>('button[data-kind]');
    if (!row) return;
    const id = row.dataset.id ?? '';
    if (row.dataset.kind === 'sound') {
      // The first click arms the sound, the second plays it; another suggestion takes the arming away.
      if (armed === id) {
        armed = null;
        paintLists();
        void play(id, row.querySelector('b')?.textContent ?? '');
      } else {
        armed = id;
        paintLists();
      }
      return;
    }
    armed = null;
    paintLists();
    // The DM view's centre follows what is opened; the page's bus carries the ask.
    const Event = doc.defaultView?.CustomEvent ?? CustomEvent;
    doc.dispatchEvent(new Event(BUS_OPEN, { detail: { type: hostType(row.dataset.type ?? ''), id } }));
  });

  paintMic();
  paintLists();
  void refresh();

  return {
    update(next) {
      const before = props;
      props = parseProps(next);
      if (props.campaign === before.campaign) return;
      generation += 1;
      suggestions = parseSuggestions(undefined);
      forgetArrivals();
      armed = null;
      // Another campaign is another session: the microphone is not on there until the DM turns it on.
      stopWatching();
      mic = 'idle';
      say('', 'mic');
      paintMic();
      paintLists();
      void refresh();
    },
  };
}
