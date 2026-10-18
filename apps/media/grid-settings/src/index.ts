// GridSettings (system media): a battlemap's hex grid on one panel — size,
// offset, orientation, colour, opacity — redrawn over the map as the DM changes
// it, a hex always 5 feet, and a save. It reads `media.getBattlemap` and
// triggers `media.saveGridSettings`, by identifier, through the shell it is
// handed: no network call of its own. Nothing is sent until « Enregistrer »,
// and a grid HexGrid@1 does not hold is never sent.

import { GET_BATTLEMAP, SAVE_GRID_SETTINGS } from './ids.ts';
import { commandBody, commandId, gridOf, initialForm, isFinal, parseBattlemap, payload, problem, reasons, settled, type Settled } from './model.ts';
import { mountPreview, type Preview } from './preview.ts';
import { fields, notice, result, skeleton, title } from './render.ts';
import { STYLE } from './style.ts';
import type { Battlemap, Form, Outcome, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

/** What the shell keeps: it hands the Micro-UI its props again when the screen's context moves. */
export interface Mounted {
  update(props: Record<string, unknown>): void;
}

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="GridSettings"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'GridSettings';
  style.textContent = STYLE;
  doc.head.append(style);
}

const battlemapOf = (props: Record<string, unknown>): string => (typeof props.battlemap === 'string' ? props.battlemap : '');

/** What the shell calls: fills `host` with the panel and wires it. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): Mounted {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`GridSettings: #${id} is not in the panel`);
    return found;
  };

  let map: Battlemap | null = null;
  let form: Form | null = null;
  let preview: Preview | null = null;
  let outcome: Outcome = { kind: 'none' };
  let busy = false;
  /** The battlemap asked for last: an answer that comes after another request is dropped. */
  let asked = 0;

  const paintResult = (): void => {
    part('grid-result').innerHTML = result(outcome);
  };
  const say = (next: Outcome): void => {
    outcome = next;
    paintResult();
  };
  /** While a battlemap loads or a command runs, the panel holds still: what is sent is what is on screen. */
  const lock = (locked: boolean): void => {
    for (const control of host.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLButtonElement>('#grid-form input, #grid-form select, #grid-form button')) {
      control.disabled = locked || form === null;
    }
  };
  const paintForm = (): void => {
    part('grid-fields').innerHTML = form === null ? '' : fields(form);
    lock(busy);
  };
  const paintMap = (): void => {
    part('grid-title').textContent = title(map);
    paintForm();
    if (map !== null && form !== null) preview = mountPreview(part('grid-preview'), map, gridOf(form));
  };
  const show = (words: string): void => {
    map = null;
    form = null;
    preview = null;
    part('grid-title').textContent = title(null);
    paintForm();
    part('grid-preview').innerHTML = notice(words);
  };

  const read = async (id: string): Promise<Battlemap | null> => parseBattlemap((await shell.capability(GET_BATTLEMAP, undefined, { id })).data);

  async function load(id: string): Promise<void> {
    const mine = ++asked;
    say({ kind: 'none' });
    if (id === '') return show('Choisissez une battlemap à régler.');
    show('Chargement de la battlemap…');
    try {
      const found = await read(id);
      if (mine !== asked) return;
      if (found === null) return show('Cette battlemap n’existe pas : seule une battlemap a une grille.');
      map = found;
      form = initialForm(found);
      paintMap();
    } catch {
      if (mine === asked) show('Impossible de lire la battlemap.');
    }
  }

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(id: string, body: Form): Promise<Settled> {
    const answer = await shell.command(commandBody(id, payload(body)));
    const command = commandId(answer);
    let state = settled(answer);
    for (let tries = 0; !isFinal(state.status) && command !== '' && tries < timing.polls; tries += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(command));
    }
    return state;
  }

  async function save(): Promise<void> {
    if (busy || map === null || form === null) return;
    const why = problem(form);
    if (why) return say({ kind: 'error', text: why });
    const id = map.id;
    const sent = form;
    const mine = asked;
    busy = true;
    lock(true);
    say({ kind: 'busy', text: 'Enregistrement…' });
    try {
      const state = await send(id, sent);
      // The screen handed the panel another battlemap meanwhile: what happened to this one is not this panel's to say.
      if (mine !== asked) return;
      if (state.status === 'rejected') return say({ kind: 'error', text: `Enregistrement refusé : ${reasons(state.violations)}.` });
      // Queued behind another command, or waiting: the server still holds it and will apply it.
      if (state.status !== 'applied' && !isFinal(state.status) && state.status !== '') return say({ kind: 'busy', text: `Enregistrement en attente (état : ${state.status}) : il s’appliquera dès que la file le permettra.` });
      if (state.status !== 'applied') return say({ kind: 'error', text: `L’enregistrement n’a pas abouti (état : ${state.status || 'inconnu'}).` });
      // What is kept is what the Capability answers now: the panel's baseline, for « Annuler les changements ».
      try {
        const saved = await read(id);
        if (mine !== asked) return;
        if (saved !== null && map !== null) map = saved;
      } catch {
        if (mine !== asked) return;
        if (map !== null) map = { ...map, grid: gridOf(sent) };
        return say({ kind: 'ok', text: 'Réglage enregistré, mais il n’a pas pu être relu.' });
      }
      say({ kind: 'ok', text: 'Réglage enregistré.' });
    } catch {
      if (mine === asked) say({ kind: 'error', text: 'L’enregistrement a échoué.' });
    } finally {
      busy = false;
      lock(false);
    }
  }

  host.addEventListener('submit', (event) => event.preventDefault());

  /** Slides, picks, changes: the form follows and the grid is redrawn at once. */
  const follow = (change: Partial<Form>): void => {
    if (form === null) return;
    form = { ...form, ...change };
    preview?.setGrid(gridOf(form));
    if (outcome.kind !== 'busy') say({ kind: 'none' });
  };

  host.addEventListener('input', (event) => {
    // Read by shape, not by class: the panel runs in whichever window the shell is.
    const input = event.target as HTMLInputElement | null;
    const name = input?.name;
    if (!input || (name !== 'size' && name !== 'ox' && name !== 'oy' && name !== 'opacity')) return;
    // The kit's Slider shows its value and its filled track.
    const out = input.closest('.kit-slider__field')?.querySelector('.kit-slider__out');
    if (out) out.textContent = input.value;
    const min = Number(input.min);
    const max = Number(input.max);
    input.style.setProperty('--fill', `${max === min ? 0 : ((Number(input.value) - min) / (max - min)) * 100}%`);
    follow({ [name]: Number(input.value) });
  });

  host.addEventListener('change', (event) => {
    const target = event.target as { name?: string; value?: string } | null;
    if (target?.name === 'grid-orient') follow({ orient: target.value === 'flat' ? 'flat' : 'pointy' });
    else if (target?.name === 'color') follow({ color: target.value ?? 'text' });
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes === SAVE_GRID_SETTINGS) void save();
    else if (button.id === 'grid-reset' && map !== null && !busy) {
      form = initialForm(map);
      say({ kind: 'none' });
      paintForm();
      preview?.setGrid(gridOf(form));
    }
  });

  let shown = battlemapOf(props);
  void load(shown);
  return {
    update(next) {
      const id = battlemapOf(next);
      if (id === shown) return;
      shown = id;
      void load(id);
    },
  };
}
