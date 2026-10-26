// SoundLibrary (system soundbox): the DM's sounds for the campaign — a list
// to search by name and filter by tag, a drop zone to add a sound, a form to
// edit its name, tags and volume, a removal. It reads `soundbox.listSounds`
// and triggers `soundbox.addSound`, `soundbox.updateSound` and
// `soundbox.removeSound`, by identifier, through the shell it is handed: no
// network call of its own. The search and the tag filter read the name and
// the tags only (tags-normalized). With no campaign given, the library is the
// one of the screen's context or, failing that, of the running session
// (`session.getLiveSession`), and publishes it to the context for the
// Micro-UIs that follow it (LiveSoundbox).

import { ADD_SOUND, CAMPAIGN_CONTEXT, GET_LIVE_SESSION, LIST_SOUNDS, REMOVE_SOUND, UPDATE_SOUND } from './ids.ts';
import {
  addPayload,
  addProblem,
  campaignOf,
  commandBody,
  commandId,
  editProblem,
  filterSounds,
  isFinal,
  isSoundFile,
  nameFromFile,
  norm,
  parseSounds,
  reasons,
  settled,
  summary,
  updatePayload,
  type Settled,
} from './model.ts';
import { editFields, empty, filter, result, skeleton, table, unreadable } from './render.ts';
import { STYLE } from './style.ts';
import type { AddForm, EditForm, Outcome, Shell, Sound } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="SoundLibrary"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'SoundLibrary';
  style.textContent = STYLE;
  doc.head.append(style);
}

const size = (bytes: number): string =>
  bytes < 1024 ? `${bytes} o` : bytes < 1048576 ? `${(bytes / 1024).toFixed(1).replace('.', ',')} Ko` : `${(bytes / 1048576).toFixed(1).replace('.', ',')} Mo`;

/** What the shell calls: fills `host` with the library and wires it. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): void {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();
  /** The campaign the screen names, else the one found below. */
  let campaign = typeof props.campaign === 'string' ? props.campaign : '';

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`SoundLibrary: #${id} is not in the card`);
    return found;
  };
  const field = (id: string): string => (part(id) as HTMLInputElement).value;

  let sounds: Sound[] = [];
  let loaded = false;
  let unread = false;
  let search = '';
  let tag: string | null = null;
  let add: AddForm = { file: '', name: '', tags: '' };
  /** The name the last dropped file suggested: replaced by the next file's, never over what the DM typed. */
  let suggested = '';
  let editing: EditForm | null = null;

  const say = (id: string, outcome: Outcome): void => {
    part(id).innerHTML = result(outcome);
  };

  const paintList = (): void => {
    part('sounds-count').textContent = loaded ? summary(sounds) : '';
    part('sounds-filter').innerHTML = filter(tag);
    const shown = filterSounds(sounds, search, tag);
    part('sounds-table').innerHTML = unread ? unreadable() : !loaded ? '<p class="muted">Chargement des sons…</p>' : shown.length === 0 ? empty(sounds.length === 0) : table(shown, tag);
  };

  const lock = (locked: boolean): void => {
    for (const control of host.querySelectorAll<HTMLInputElement | HTMLButtonElement>('#sounds-upload input, #sounds-upload button, #sounds-edit input, #sounds-edit button')) {
      control.disabled = locked;
    }
  };

  /** Finds the campaign when none was given: the screen's context first, the running session's then, which it publishes for the others. */
  const locate = async (): Promise<void> => {
    if (campaign !== '') return;
    // Whatever goes wrong here leaves the library to its plain read: the sounds are listed all the same.
    try {
      const named = shell.context.get(CAMPAIGN_CONTEXT);
      if (typeof named === 'string' && named !== '') {
        campaign = named;
        return;
      }
      campaign = campaignOf((await shell.capability(GET_LIVE_SESSION, undefined)).data);
      if (campaign === '') return;
      part('sounds-campaign').textContent = `Campagne : ${campaign}`;
      shell.context.set(CAMPAIGN_CONTEXT, campaign);
    } catch {
      // The next read asks again while no campaign is known.
    }
  };
  let located: Promise<void> | null = null;
  /** Waits for the campaign; asked again by the next read while no session was found, since one may start. */
  const campaignKnown = (): Promise<void> => {
    located ??= locate().then(() => {
      if (campaign === '') located = null;
    });
    return located;
  };

  /** The newest read asked for: an older answer that arrives after it is dropped. */
  let latest = 0;

  /** Reads the sounds again; false when they cannot be read. */
  const refresh = async (): Promise<boolean> => {
    await campaignKnown();
    const mine = (latest += 1);
    let fresh: Sound[];
    try {
      fresh = parseSounds((await shell.capability(LIST_SOUNDS, undefined, campaign === '' ? {} : { campaign })).data);
    } catch {
      if (mine === latest) unread = !loaded;
      return false;
    }
    if (mine !== latest) return true;
    sounds = fresh;
    loaded = true;
    unread = false;
    // A form open on a sound that is no longer there closes.
    if (editing !== null && !sounds.some((s) => s.id === editing?.id)) {
      editing = null;
      part('sounds-edit').hidden = true;
    }
    paintList();
    return true;
  };

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

  let busy = false;

  /** One command at a time: the forms are locked until its end. `run` answers what the area tells. */
  async function act(area: string, wrong: string, run: () => Promise<Outcome>): Promise<void> {
    if (busy) return;
    busy = true;
    lock(true);
    try {
      say(area, await run());
    } catch {
      say(area, { kind: 'error', text: wrong });
      // The command may have applied all the same: what the DM sees is what is stored.
      await refresh();
    } finally {
      busy = false;
      lock(false);
    }
  }

  /** The outcome of a settled command that did not apply, or null when it did. */
  const failure = (refused: string, unfinished: string, state: Settled): Outcome | null => {
    if (state.status === 'applied') return null;
    if (state.status === 'rejected') return { kind: 'error', text: `${refused} : ${reasons(state.violations)}.` };
    return { kind: 'error', text: `${unfinished} n’a pas abouti (état : ${state.status || 'inconnu'}).` };
  };

  const clearDrop = (): void => {
    part('sounds-drop-field').classList.remove('has-file', 'has-error', 'is-done');
    (part('sounds-file') as HTMLInputElement).value = '';
    add = { file: '', name: '', tags: '' };
    suggested = '';
  };

  /** A file chosen or dropped: shown in the FileDrop if it is a sound, refused otherwise. */
  const takeFile = (file: { name: string; size: number }): void => {
    const drop = part('sounds-drop-field');
    if (!isSoundFile(file.name)) {
      drop.classList.remove('has-file', 'is-done');
      drop.classList.add('has-error');
      add = { ...add, file: '' };
      return;
    }
    drop.classList.remove('has-error');
    drop.classList.add('has-file', 'is-done');
    const named = (cls: string): HTMLElement => {
      const found = drop.querySelector<HTMLElement>(`.${cls}`);
      if (!found) throw new Error(`SoundLibrary: .${cls} is not in the drop zone`);
      return found;
    };
    named('kit-filedrop__name').textContent = file.name;
    named('kit-filedrop__size').textContent = size(file.size);
    named('kit-filedrop__thumb').textContent = (file.name.split('.').pop() ?? '').toUpperCase();
    named('kit-filedrop__fill').style.width = '100%';
    add = { ...add, file: file.name };
    const name = part('sounds-new-name') as HTMLInputElement;
    if (name.value === '' || name.value === suggested) {
      suggested = nameFromFile(file.name);
      name.value = suggested;
    }
  };

  const addSound = (): Promise<void> => {
    add = { file: add.file, name: field('sounds-new-name'), tags: field('sounds-new-tags') };
    const why = addProblem(add);
    if (why) {
      say('sounds-upload-result', { kind: 'error', text: why });
      return Promise.resolve();
    }
    const form = add;
    return act('sounds-upload-result', 'L’ajout a échoué.', async () => {
      say('sounds-upload-result', { kind: 'busy', text: 'Ajout en cours…' });
      await campaignKnown();
      const state = await send(ADD_SOUND, addPayload(form, campaign));
      const wrong = failure('Ajout refusé', 'L’ajout', state);
      if (wrong) return wrong;
      clearDrop();
      (part('sounds-new-name') as HTMLInputElement).value = '';
      (part('sounds-new-tags') as HTMLInputElement).value = '';
      const read = await refresh();
      return { kind: 'ok', text: read ? `« ${form.name.trim()} » est dans la soundbox.` : 'Le son est ajouté, mais la liste n’a pas pu être relue.' };
    });
  };

  const openEdit = (id: string): void => {
    const sound = sounds.find((s) => s.id === id);
    if (!sound) return;
    editing = { id: sound.id, name: sound.name, tags: sound.tags.join(', '), volume: sound.volume };
    part('sounds-edit').hidden = false;
    part('sounds-edit-fields').innerHTML = editFields(editing);
    say('sounds-edit-result', { kind: 'none' });
    // A command in flight holds the new form still too.
    if (busy) lock(true);
    const form = part('sounds-edit');
    if (typeof form.scrollIntoView === 'function') form.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  };

  const readEdit = (current: EditForm): EditForm => ({
    id: current.id,
    name: field('sounds-edit-name'),
    tags: field('sounds-edit-tags'),
    volume: Number(field('sounds-edit-volume')),
  });

  const saveSound = (): Promise<void> => {
    if (editing === null) return Promise.resolve();
    const form = readEdit(editing);
    const why = editProblem(form);
    if (why) {
      say('sounds-edit-result', { kind: 'error', text: why });
      return Promise.resolve();
    }
    return act('sounds-edit-result', 'L’enregistrement a échoué.', async () => {
      say('sounds-edit-result', { kind: 'busy', text: 'Enregistrement…' });
      const state = await send(UPDATE_SOUND, updatePayload(form), form.id);
      const wrong = failure('Enregistrement refusé', 'L’enregistrement', state);
      if (wrong) return wrong;
      editing = form;
      const read = await refresh();
      return { kind: 'ok', text: read ? 'Son enregistré.' : 'Son enregistré, mais la liste n’a pas pu être relue.' };
    });
  };

  const removeSound = (): Promise<void> => {
    if (editing === null) return Promise.resolve();
    const form = editing;
    const name = sounds.find((s) => s.id === form.id)?.name ?? form.name;
    return act('sounds-status', 'La suppression a échoué.', async () => {
      say('sounds-status', { kind: 'busy', text: 'Suppression…' });
      const state = await send(REMOVE_SOUND, {}, form.id);
      const wrong = failure('Suppression refusée', 'La suppression', state);
      if (wrong) return wrong;
      // The form closes with the sound it was open on, not over another one opened since.
      if (editing?.id === form.id) {
        editing = null;
        part('sounds-edit').hidden = true;
      }
      const read = await refresh();
      return { kind: 'ok', text: read ? `« ${name} » est supprimé.` : `« ${name} » est supprimé, mais la liste n’a pas pu être relue.` };
    });
  };

  host.addEventListener('submit', (event) => event.preventDefault());

  host.addEventListener('input', (event) => {
    const target = event.target as { id?: string; value?: string } | null;
    if (target?.id === 'sounds-search-input') {
      search = target.value ?? '';
      paintList();
    } else if (target?.id === 'sounds-edit-volume') {
      // The kit's Slider shows its value and fills its track as it moves.
      const slider = event.target as HTMLInputElement;
      const out = slider.closest('.kit-slider__field')?.querySelector('.kit-slider__out');
      if (out) out.textContent = slider.value;
      slider.style.setProperty('--fill', `${slider.value}%`);
    }
  });

  host.addEventListener('change', (event) => {
    const target = event.target as HTMLInputElement | null;
    if (target?.id !== 'sounds-file') return;
    const file = target.files?.[0];
    if (file) takeFile(file);
  });

  // The kit's FileDrop: a file dropped on the zone is taken as one chosen.
  host.addEventListener('dragover', (event) => {
    if ((event.target as Element | null)?.closest('#sounds-drop')) {
      event.preventDefault();
      part('sounds-drop').classList.add('is-over');
    }
  });
  host.addEventListener('dragleave', (event) => {
    if ((event.target as Element | null)?.closest('#sounds-drop')) part('sounds-drop').classList.remove('is-over');
  });
  host.addEventListener('drop', (event) => {
    if (!(event.target as Element | null)?.closest('#sounds-drop')) return;
    event.preventDefault();
    part('sounds-drop').classList.remove('is-over');
    const file = event.dataTransfer?.files[0];
    if (file) takeFile(file);
  });
  host.addEventListener('keydown', (event) => {
    if ((event.target as Element | null)?.id === 'sounds-drop' && (event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      part('sounds-file').click();
    }
  });

  host.addEventListener('click', (event) => {
    const clicked = event.target as Element | null;
    const button = clicked?.closest('button');
    if (!button) return;
    if (button.dataset.tag !== undefined) {
      // A tag chosen filters the list by it; chosen again, it lets go.
      tag = tag !== null && norm(tag) === norm(button.dataset.tag) ? null : button.dataset.tag;
      paintList();
    } else if (button.dataset.edit !== undefined) openEdit(button.dataset.edit);
    else if (button.id === 'sounds-filter-clear') {
      tag = null;
      paintList();
    } else if (button.id === 'sounds-new-submit') void addSound();
    else if (button.id === 'sounds-edit-save') void saveSound();
    else if (button.id === 'sounds-edit-delete') void removeSound();
    else if (button.id === 'sounds-edit-cancel') {
      editing = null;
      part('sounds-edit').hidden = true;
    }
  });

  paintList();
  void refresh().then((read) => {
    if (!read) paintList();
  });
  // The first notice only tells the version the screen opened on: the list was just read.
  let first = true;
  shell.onDataVersion?.(() => {
    if (first) first = false;
    else void refresh();
  });
}
