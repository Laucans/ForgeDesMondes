// OralBrief (system ai): the DM tells the session out loud, or types it, and
// the AI cuts it into ordered scenes, recognises the elements the campaign
// already holds and lists the ones it lacks. It reads `session.getLastBrief`
// and triggers `ai.analyzeBrief`, by identifier, through the shell it is
// handed: no network call of its own. Nothing is created here
// (brief-no-auto-creation): the missing elements are only listed, and only
// text reaches the llm (text-only-no-audio): the payload is the typed or
// transcribed text, or — when the host records and nothing was typed — the
// dictation, which the speech-to-text port turns into text on the machine.

import { ANALYZE_BRIEF, GET_LAST_BRIEF } from './ids.ts';
import { commandBody, commandId, isFinal, lastRunLabel, parseBrief, reasons, settled, type Settled } from './model.ts';
import { MIC_TEXTS, result as resultHtml, skeleton, status, type MicState } from './render.ts';
import { STYLE } from './style.ts';
import type { Brief, Dictation, Outcome, Shell, Timing } from './types.ts';

export type { Dictation, Shell, Timing } from './types.ts';

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

const NO_BRIEF: Brief = { lastRun: null, duration: null, transcript: [], scenes: [], recognized: [], missing: [] };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="OralBrief"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'OralBrief';
  style.textContent = STYLE;
  doc.head.append(style);
}

const sessionOf = (props: Record<string, unknown>): string => (typeof props.session === 'string' ? props.session : '');

/** What the shell calls: fills `host` with the screen and wires it. Answers how to follow new props. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, dictation?: Dictation): { update(next: Record<string, unknown>): void } {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`OralBrief: #${id} is not on the screen`);
    return found;
  };

  let session = sessionOf(props);
  let busy = false;
  // The audio of the dictation just paused, held until the brief is sent or the session changes.
  let take: string | Promise<string> = '';

  const say = (outcome: Outcome): void => {
    part('brief-status').innerHTML = status(outcome);
  };
  const paint = (brief: Brief): void => {
    part('brief-last').textContent = lastRunLabel(brief.lastRun, brief.duration);
    part('brief-result').innerHTML = resultHtml(brief);
  };

  const field = (): HTMLTextAreaElement => part<HTMLTextAreaElement>('brief-text');

  // The microphone: a click starts the dictation, a click pauses it, and the pause hands the transcript to the text area.
  const mic = part('brief-mic-box');
  const micState = mic.querySelector<HTMLElement>('.kit-micbutton__state');
  const micTimer = mic.querySelector<HTMLElement>('.kit-micbutton__timer');
  let state: MicState = 'idle';
  let seconds = 0;
  let tick: ReturnType<typeof setInterval> | null = null;
  let pulse: ReturnType<typeof setInterval> | null = null;
  const clock = (): string => `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
  const stopTimers = (): void => {
    if (tick !== null) clearInterval(tick);
    if (pulse !== null) clearInterval(pulse);
    tick = pulse = null;
    mic.style.setProperty('--kit-level', '0');
  };

  /** « Terminer et analyser » waits for a text, or for a dictation that may give one. */
  const count = (): void => {
    const analyse = host.querySelector<HTMLButtonElement>('#brief-analyse');
    if (!analyse) return;
    analyse.disabled = busy || (field().value.trim() === '' && state !== 'recording' && take === '');
  };

  const setMic = (next: MicState): void => {
    state = next;
    stopTimers();
    if (next === 'recording') {
      dictation?.start?.();
      tick = setInterval(() => {
        // A screen that left the page stops listening.
        if (!mic.isConnected) return stopTimers();
        seconds += 1;
        if (micTimer) micTimer.textContent = clock();
      }, 1000);
      pulse = setInterval(() => mic.style.setProperty('--kit-level', (Math.random() * 0.9 + 0.1).toFixed(2)), 140);
    }
    mic.setAttribute('data-state', next);
    if (micState) micState.textContent = MIC_TEXTS[next];
    if (micTimer) micTimer.textContent = clock();
    const button = part('brief-mic');
    button.setAttribute('aria-pressed', String(next === 'recording'));
    button.setAttribute('aria-label', `Brief oral — ${MIC_TEXTS[next]}`);
    count();
  };

  /** Ends a dictation in progress: what was heard joins the text area. */
  const hear = (): void => {
    if (state !== 'recording') return;
    setMic('paused');
    const recorded = dictation?.audio() ?? '';
    if (recorded !== '') take = recorded;
    const heard = dictation?.transcript() ?? '';
    count();
    if (heard === '') return;
    const area = field();
    area.value = area.value === '' ? heard : `${area.value}\n${heard}`;
    count();
  };
  const dictate = (): void => (state === 'recording' ? hear() : setMic('recording'));

  /** While a command runs, the screen holds still: what is sent is what is on screen. */
  const lock = (locked: boolean): void => {
    busy = locked;
    for (const control of host.querySelectorAll<HTMLTextAreaElement | HTMLButtonElement>('#brief-text, #brief-mic')) control.disabled = locked;
    count();
  };

  const read = async (): Promise<Brief> => parseBrief((await shell.capability(GET_LAST_BRIEF, undefined, { session })).data);

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, payload: Record<string, unknown>): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, payload));
    const id = commandId(answer);
    let outcome = settled(answer);
    for (let asked = 0; !isFinal(outcome.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      outcome = settled(await shell.commandResult(id));
    }
    return outcome;
  }

  const analyse = async (): Promise<void> => {
    if (busy) return;
    hear();
    const text = field().value.trim();
    // An empty brief sends nothing: the DM is brought back to the field.
    if (text === '' && take === '') {
      field().focus();
      return;
    }
    if (session === '') return say({ kind: 'error', text: 'Aucune séance choisie.' });
    // The brief belongs to the session it was sent for: a page that moves on meanwhile is not shown its result.
    const target = session;
    lock(true);
    try {
      say({ kind: 'busy', text: 'Analyse du brief en cours…' });
      // What was typed or heard as words is sent as text; a take with no words is sent as audio, for the port to hear.
      const audio = text === '' ? await take : '';
      if (text === '' && audio === '') return say({ kind: 'error', text: 'Aucun son n’a été enregistré.' });
      const outcome = await send(ANALYZE_BRIEF, text === '' ? { session: target, dictation: audio } : { session: target, text });
      if (outcome.status === 'rejected') return say({ kind: 'error', text: `Analyse refusée : ${reasons(outcome.violations)}.` });
      // A command still waiting its turn keeps its text: it applies on its own clock, and sending it again would analyse the brief twice.
      if (!isFinal(outcome.status) && outcome.status !== '') return say({ kind: 'error', text: `L’analyse est toujours en attente (état : ${outcome.status}) : elle s’appliquera à son tour, ne la relancez pas.` });
      if (outcome.status !== 'applied') return say({ kind: 'error', text: `L’analyse n’a pas abouti (état : ${outcome.status || 'inconnu'}).` });
      if (target !== session) return;
      // The brief is analysed: the text area is spent, and the view shows what the analysis left.
      field().value = '';
      seconds = 0;
      take = '';
      setMic('idle');
      try {
        const brief = await read();
        if (target !== session) return;
        paint(brief);
        say({ kind: 'ok', text: `Brief analysé · ${brief.scenes.length} scène${brief.scenes.length > 1 ? 's' : ''}, ${brief.missing.length} à créer.` });
      } catch {
        say({ kind: 'error', text: 'Le brief a été analysé, mais son résultat n’a pas pu être relu.' });
      }
    } catch {
      say({ kind: 'error', text: 'L’analyse a échoué.' });
    } finally {
      lock(false);
    }
  };

  host.addEventListener('input', (event) => {
    if ((event.target as { id?: string } | null)?.id === 'brief-text') count();
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes === ANALYZE_BRIEF) void analyse();
    else if (button.id === 'brief-mic') dictate();
    else if (button.dataset.diff !== undefined) {
      // The event is made by the page's own window, the one that dispatches it.
      const Event = host.ownerDocument.defaultView?.CustomEvent ?? CustomEvent;
      host.dispatchEvent(new Event('open-versions', { bubbles: true, detail: { id: button.dataset.diff } }));
    }
  });

  setMic('idle');
  paint(NO_BRIEF);
  const load = (): void => {
    if (session === '') return;
    const target = session;
    // An answer for a session the page has left is not shown.
    read().then(
      (brief) => {
        if (target === session) paint(brief);
      },
      () => {
        if (target === session) say({ kind: 'error', text: 'Impossible de lire le dernier brief.' });
      },
    );
  };
  load();

  return {
    update(next) {
      const chosen = sessionOf(next);
      if (chosen === session) return;
      session = chosen;
      // A recorder still listening is stopped and its sound thrown away: it would otherwise be sent as the next session's brief.
      if (state === 'recording') void Promise.resolve(dictation?.audio?.()).catch(() => undefined);
      // Another session starts with a quiet microphone and an empty text area.
      seconds = 0;
      take = '';
      setMic('idle');
      field().value = '';
      paint(NO_BRIEF);
      say({ kind: 'none' });
      load();
    },
  };
}
