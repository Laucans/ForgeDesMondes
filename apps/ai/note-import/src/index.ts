// NoteImport (system ai): the DM types, pastes or dictates free-text notes,
// the AI extracts the characters, places and events, and the DM ticks the
// list before anything is created. It reads `campaign.listImportedNotes`
// and triggers `ai.extractFromNotes` and `campaign.createEntitiesFromNote`,
// by identifier, through the shell it is handed: no network call of its own.
// Nothing is created before « Créer la sélection » (no-creation-before-
// validation): extracting sends one command, validating sends another, with
// exactly the ids that are checked.

import { CREATE_ENTITIES_FROM_NOTE, EXTRACT_FROM_NOTES, LIST_IMPORTED_NOTES } from './ids.ts';
import { commandBody, commandId, createLabel, isFinal, parseNotes, pending, reasons, settled, type Settled } from './model.ts';
import { history, MIC_TEXTS, proposals as proposalsHtml, skeleton, status, type MicState } from './render.ts';
import { STYLE } from './style.ts';
import type { Dictation, Notes, Outcome, Proposal, Shell, Timing } from './types.ts';

export type { Dictation, Shell, Timing } from './types.ts';

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="NoteImport"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'NoteImport';
  style.textContent = STYLE;
  doc.head.append(style);
}

const campaignOf = (props: Record<string, unknown>): string => (typeof props.campaign === 'string' ? props.campaign : '');

/** What the shell calls: fills `host` with the screen and wires it. Answers how to follow new props. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, dictation?: Dictation): { update(next: Record<string, unknown>): void } {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`NoteImport: #${id} is not on the screen`);
    return found;
  };

  let campaign = campaignOf(props);
  let notes: Notes = { imports: [], proposals: [] };
  /** The proposals the DM has validated or rejected here: they are not offered again. */
  let done = new Set<string>();
  let shown: Proposal[] = [];
  let busy = false;

  const say = (outcome: Outcome): void => {
    part('notes-status').innerHTML = status(outcome);
  };
  const checkedIds = (): string[] => Array.from(host.querySelectorAll<HTMLInputElement>('input[name="proposal"]:checked')).map((input) => input.value);
  const count = (): void => {
    const create = host.querySelector<HTMLButtonElement>('#notes-create');
    if (!create) return;
    const checked = checkedIds().length;
    create.textContent = createLabel(checked);
    create.disabled = busy || checked === 0;
  };
  const paintProposals = (): void => {
    const box = part('notes-proposals');
    box.hidden = shown.length === 0;
    box.innerHTML = shown.length === 0 ? '' : proposalsHtml(shown);
    // A list painted while a command runs holds still like the rest.
    const cancel = host.querySelector<HTMLButtonElement>('#notes-cancel');
    if (cancel) cancel.disabled = busy;
  };
  const paint = (): void => {
    part('notes-history-list').innerHTML = history(notes.imports);
    shown = pending(notes, done);
    paintProposals();
  };

  /** While a command runs, the screen holds still: what is sent is what is on screen. */
  const lock = (locked: boolean): void => {
    busy = locked;
    for (const control of host.querySelectorAll<HTMLTextAreaElement | HTMLButtonElement>('#notes-text, #notes-extract, #notes-mic, #notes-cancel')) control.disabled = locked;
    count();
  };

  const read = async (): Promise<Notes> => parseNotes((await shell.capability(LIST_IMPORTED_NOTES, undefined, { campaign })).data);
  /** Reads the notes again after a command; false when they cannot be read, the command having applied all the same. */
  const refresh = async (): Promise<boolean> => {
    try {
      notes = await read();
      paint();
      return true;
    } catch {
      return false;
    }
  };

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, payload: Record<string, unknown>): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, payload));
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  /** One command at a time: the screen is locked until its end. */
  async function act(wrong: string, run: () => Promise<void>): Promise<void> {
    if (busy) return;
    if (campaign === '') return say({ kind: 'error', text: 'Aucune campagne choisie.' });
    lock(true);
    try {
      await run();
    } catch {
      say({ kind: 'error', text: wrong });
    } finally {
      lock(false);
    }
  }

  const extract = (): Promise<void> => {
    const field = part<HTMLTextAreaElement>('notes-text');
    // An empty note sends nothing: the DM is brought back to the field.
    if (field.value.trim() === '') {
      field.focus();
      return Promise.resolve();
    }
    const text = field.value;
    return act('L’extraction a échoué.', async () => {
      say({ kind: 'busy', text: 'Extraction en cours…' });
      const state = await send(EXTRACT_FROM_NOTES, { campaign, text });
      if (state.status === 'rejected') return say({ kind: 'error', text: `Extraction refusée : ${reasons(state.violations)}.` });
      if (state.status !== 'applied') return say({ kind: 'error', text: `L’extraction n’a pas abouti (état : ${state.status || 'inconnu'}).` });
      // This analysis' proposals are all awaiting, whatever ids an earlier one had.
      done = new Set();
      if (!(await refresh())) return say({ kind: 'error', text: 'La note a été analysée, mais ses propositions n’ont pas pu être lues.' });
      say(shown.length === 0 ? { kind: 'ok', text: 'Aucun élément trouvé dans cette note · note brute conservée.' } : { kind: 'none' });
    });
  };

  const create = (): Promise<void> => {
    const accepted = checkedIds();
    if (accepted.length === 0) return Promise.resolve();
    return act('La création a échoué.', async () => {
      say({ kind: 'busy', text: 'Création en cours…' });
      const state = await send(CREATE_ENTITIES_FROM_NOTE, { campaign, accepted });
      if (state.status === 'rejected') return say({ kind: 'error', text: `Création refusée : ${reasons(state.violations)}.` });
      if (state.status !== 'applied') return say({ kind: 'error', text: `La création n’a pas abouti (état : ${state.status || 'inconnu'}).` });
      // The whole list is settled: the ticked became elements, the others are left out.
      for (const proposal of shown) done.add(proposal.id);
      const read = await refresh();
      if (!read) {
        shown = [];
        paintProposals();
      }
      const n = accepted.length;
      say({ kind: 'ok', text: `${n} élément${n > 1 ? 's' : ''} créé${n > 1 ? 's' : ''} · note brute conservée.${read ? '' : ' La liste des imports n’a pas pu être relue.'}` });
    });
  };

  const reject = (): void => {
    if (busy) return;
    for (const proposal of shown) done.add(proposal.id);
    shown = [];
    paintProposals();
    say({ kind: 'none' });
  };

  // The microphone: a click starts the dictation, a click pauses it, and the pause hands the transcript to the text area.
  const mic = part('notes-mic-box');
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
  const setMic = (next: MicState): void => {
    state = next;
    stopTimers();
    if (next === 'recording') {
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
    const button = part('notes-mic');
    button.setAttribute('aria-pressed', String(next === 'recording'));
    button.setAttribute('aria-label', `Dicter — ${MIC_TEXTS[next]}`);
  };
  const dictate = (): void => {
    if (state !== 'recording') return setMic('recording');
    setMic('paused');
    const heard = dictation?.transcript() ?? '';
    if (heard === '') return;
    const field = part<HTMLTextAreaElement>('notes-text');
    field.value = field.value === '' ? heard : `${field.value}\n${heard}`;
  };

  host.addEventListener('change', (event) => {
    if ((event.target as { name?: string } | null)?.name === 'proposal') count();
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes === EXTRACT_FROM_NOTES) void extract();
    else if (button.dataset.writes === CREATE_ENTITIES_FROM_NOTE) void create();
    else if (button.id === 'notes-cancel') reject();
    else if (button.id === 'notes-mic') dictate();
  });

  setMic('idle');
  const load = (): void => {
    if (campaign === '') return;
    read().then(
      (loaded) => {
        notes = loaded;
        paint();
      },
      () => say({ kind: 'error', text: 'Impossible de lire les notes importées.' }),
    );
  };
  load();

  return {
    update(next) {
      const chosen = campaignOf(next);
      if (chosen === campaign) return;
      campaign = chosen;
      done = new Set();
      notes = { imports: [], proposals: [] };
      paint();
      say({ kind: 'none' });
      load();
    },
  };
}
