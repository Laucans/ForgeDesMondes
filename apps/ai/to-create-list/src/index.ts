// ToCreateList (system ai): the NPCs, places and events the DM's briefs left
// to create. It reads `campaign.listEntitiesToCreate` and, once an element is
// made, `campaign.getEntity` to show what the AI proposed; it triggers
// `ai.createFromBrief`, by identifier, through the shell it is handed: no
// network call of its own. « Créer depuis le brief » sends the entry alone;
// « Affiner » opens a field, written or dictated, whose text is sent with it.

import { CREATE_FROM_BRIEF, GET_ENTITY, LIST_ENTITIES_TO_CREATE } from './ids.ts';
import { commandBody, commandId, countLabel, inSession, isFinal, parseItems, parseProposal, reasons, refinementOf, settled, type Settled } from './model.ts';
import { clock, freshView, itemHTML, MIC_TEXTS, skeleton, type View } from './render.ts';
import { STYLE } from './style.ts';
import type { Dictation, Item, Shell, Timing } from './types.ts';

export type { Dictation, Shell, Timing } from './types.ts';

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="ToCreateList"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'ToCreateList';
  style.textContent = STYLE;
  doc.head.append(style);
}

const textOf = (props: Record<string, unknown>, name: string): string => (typeof props[name] === 'string' ? props[name] : '');

/** What the shell calls: fills `host` with the screen and wires it. Answers how to follow new props. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, dictation?: Dictation): { update(next: Record<string, unknown>): void } {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`ToCreateList: #${id} is not on the screen`);
    return found;
  };

  let campaign = textOf(props, 'campaign');
  let session = textOf(props, 'session');
  let all: Item[] = [];
  let views = new Map<string, View>();
  let busy = false;
  let loading = 0;
  /** Moves with the campaign: what a command of the last one answers is no longer the screen's. */
  let generation = 0;
  const timers = new Map<string, ReturnType<typeof setInterval>>();

  const viewOf = (id: string): View => {
    let view = views.get(id);
    if (!view) {
      view = freshView();
      views.set(id, view);
    }
    return view;
  };
  const shown = (): Item[] => inSession(all, session);
  const element = (id: string): HTMLElement | null => Array.from(host.querySelectorAll<HTMLElement>('li[data-id]')).find((li) => li.dataset.id === id) ?? null;

  const fail = (text: string | null): void => {
    const line = part('to-create-error');
    line.hidden = text === null;
    line.textContent = text ?? '';
  };
  const paintItem = (item: Item): void => {
    const li = element(item.id);
    if (li) li.outerHTML = itemHTML(item, viewOf(item.id), busy);
  };
  const paint = (): void => {
    part('to-create-items').innerHTML = shown()
      .map((item) => itemHTML(item, viewOf(item.id), busy))
      .join('');
    part('to-create-count').textContent = countLabel(shown());
  };

  const read = async (): Promise<Item[]> => parseItems((await shell.capability(LIST_ENTITIES_TO_CREATE, undefined, { campaign })).data);
  /** Reads the list again; false when it cannot be read. */
  const refresh = async (): Promise<boolean> => {
    const mine = (loading += 1);
    try {
      const items = await read();
      if (mine !== loading) return true;
      all = items;
      fail(null);
      paint();
      return true;
    } catch {
      return false;
    }
  };
  const load = (): void => {
    if (campaign === '') return;
    void refresh().then((ok) => {
      if (!ok) fail('Impossible de lire les éléments à créer.');
    });
  };

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(item: Item, refinement: string | null): Promise<Settled> {
    const answer = await shell.command(commandBody(CREATE_FROM_BRIEF, item, refinement));
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  const stopMic = (id: string): void => {
    const timer = timers.get(id);
    if (timer !== undefined) clearInterval(timer);
    timers.delete(id);
  };

  /** One creation at a time: the screen holds still until its end, so what is sent is what is on screen. */
  async function create(item: Item, refinement: string | null): Promise<void> {
    if (item.status === 'created') return;
    if (campaign === '') return fail('Aucune campagne choisie.');
    const view = viewOf(item.id);
    busy = true;
    const mine = generation;
    stopMic(item.id);
    view.mic = 'idle';
    view.seconds = 0;
    view.outcome = { kind: 'busy', text: refinement === null ? 'Création depuis le brief…' : 'Proposition en cours…' };
    paint();
    try {
      const state = await send(item, refinement);
      if (mine !== generation) return;
      if (state.status === 'rejected') {
        view.outcome = { kind: 'error', text: `Création refusée : ${reasons(state.violations)}.` };
      } else if (state.status !== 'applied') {
        view.outcome = { kind: 'error', text: `La création n’a pas abouti (état : ${state.status || 'inconnu'}).` };
      } else {
        view.outcome = null;
        view.refining = false;
        view.typed = '';
        const read = await refresh();
        if (mine !== generation) return;
        if (!read) view.outcome = { kind: 'error', text: 'L’élément a été créé, mais la liste n’a pas pu être relue.' };
        const made = all.find((candidate) => candidate.id === item.id);
        if (made?.entity) {
          // The proposal is what the created element holds; an element that cannot be read is still created.
          try {
            const proposed = parseProposal((await shell.capability(GET_ENTITY, undefined, { id: made.entity })).data);
            if (mine === generation) view.proposal = proposed;
          } catch {
            view.proposal = null;
          }
        }
      }
    } catch {
      view.outcome = { kind: 'error', text: 'La création a échoué.' };
    } finally {
      busy = false;
      paint();
    }
  }

  /** A click starts the dictation, a click pauses it, and the pause hands the transcript to the field. */
  function dictate(item: Item): void {
    const view = viewOf(item.id);
    if (view.mic !== 'recording') {
      view.mic = 'recording';
      timers.set(
        item.id,
        setInterval(() => {
          const li = element(item.id);
          // A screen that left the page stops listening.
          if (!li || !host.isConnected) return stopMic(item.id);
          view.seconds += 1;
          const timer = li.querySelector<HTMLElement>('.kit-micbutton__timer');
          if (timer) timer.textContent = clock(view.seconds);
          li.querySelector<HTMLElement>('.kit-micbutton')?.style.setProperty('--kit-level', (Math.random() * 0.9 + 0.1).toFixed(2));
        }, 1000),
      );
    } else {
      view.mic = 'paused';
      stopMic(item.id);
      const heard = dictation?.transcript() ?? '';
      if (heard !== '') view.typed = view.typed === '' ? heard : `${view.typed}\n${heard}`;
    }
    paintItem(item);
    const box = element(item.id)?.querySelector<HTMLElement>('.kit-micbutton__state');
    if (box) box.textContent = MIC_TEXTS[view.mic];
  }

  host.addEventListener('input', (event) => {
    const field = event.target as HTMLTextAreaElement | null;
    const id = field?.closest<HTMLElement>('li[data-id]')?.dataset.id;
    if (field?.tagName === 'TEXTAREA' && id !== undefined) viewOf(id).typed = field.value;
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest<HTMLButtonElement>('button[data-action]');
    const id = button?.closest<HTMLElement>('li[data-id]')?.dataset.id;
    const item = all.find((candidate) => candidate.id === id);
    if (!button || !item || busy) return;
    const view = viewOf(item.id);
    switch (button.dataset.action) {
      case 'from-brief':
        // « Créer depuis le brief »: the entry alone, no refinement.
        void create(item, null);
        break;
      case 'refine':
        view.refining = true;
        paintItem(item);
        break;
      case 'cancel':
        stopMic(item.id);
        views.set(item.id, freshView());
        paintItem(item);
        break;
      case 'propose-refined':
        // « Proposer »: the entry with the sentences the DM added; a blank field is no refinement.
        void create(item, refinementOf(view.typed));
        break;
      case 'dictate':
        dictate(item);
        break;
    }
  });

  paint();
  load();
  const unsubscribe = shell.onDataVersion?.(() => {
    // A screen that left the page stops following the data.
    if (!host.isConnected) return unsubscribe?.();
    // What a brief's analysis added shows without a reload; a creation under way refreshes itself.
    if (!busy) load();
  });

  return {
    update(next) {
      const chosen = textOf(next, 'campaign');
      session = textOf(next, 'session');
      if (chosen !== campaign) {
        // Another campaign: nothing of the last one stays.
        campaign = chosen;
        generation += 1;
        all = [];
        views = new Map();
        for (const id of [...timers.keys()]) stopMic(id);
        load();
      }
      paint();
    },
  };
}
