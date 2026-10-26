// VersionHistory (system campaign): the history of one element — who changed
// what, when, as a diff — and the way back. It reads
// `campaign.listEntityVersions` and triggers `campaign.restoreEntityVersion`,
// by identifier, through the shell it is handed: no network call of its own.
// Going back to a version is exact (`restore-exact`): the command names a
// version and no field. Undoing some changes of a version names the version
// before it and those changes. Either way what the server applies comes back as
// a new version, on top of the history, so it can be undone in turn
// (`every-change-versioned`).

import { LIST_ENTITY_VERSIONS, RESTORE_ENTITY_VERSION } from './ids.ts';
import {
  commandId,
  hasPrevious,
  isFinal,
  parseHistory,
  reasons,
  restoreBody,
  seenUpTo,
  settled,
  sourceLabel,
  topVersion,
  unseen,
  type Restore,
  type Settled,
} from './model.ts';
import { body, dialog, skeleton, trigger } from './render.ts';
import { STYLE } from './style.ts';
import type { History, Props, Shell, Version } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

/** What the shell keeps of the panel: it follows its props, and opens on demand. */
export interface Mounted {
  /** The props changed — another element, or the same one after a write: read its history again. */
  update(props: Props): void;
  /** Reads the element's history again, then opens it: what the DM opens is what the server holds now. */
  open(): Promise<void>;
}

interface Status {
  text: string;
  kind: 'ok' | 'error' | 'busy';
}

/** The modal while it is open: what it shows, and what it last said. */
interface Modal {
  el: HTMLElement;
  /** The number of the version whose diff is on show. */
  shown: number | undefined;
  status: Status | null;
  close(): void;
}

const EMPTY: History = { name: '', versions: [] };

/** One click is one command: a new key per click, the same for every attempt of it. */
let clicks = 0;
const keyOf = (entity: string, restore: Restore): string =>
  `VersionHistory:${entity}:${restore.version}:${(restore.changes ?? []).join(',')}:${Date.now().toString(36)}-${(clicks += 1)}`;

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="VersionHistory"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'VersionHistory';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the panel and wires it. */
export function mount(host: HTMLElement, props: Props, shell: Shell, timing: Timing = WAITING): Mounted {
  const doc = host.ownerDocument;
  ensureStyle(doc);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`VersionHistory: #${id} is not in the panel`);
    return found;
  };

  let current: Props = props;
  let history: History = EMPTY;
  let unreadable = false;
  /** Per element, up to which version the DM has looked: what comes after it from someone else is « Mis à jour ». */
  const looked = new Map<string, number>();
  let generation = 0;
  let busy = false;
  let modal: Modal | null = null;

  const entity = (): string | undefined => (current.entity ? current.entity : undefined);
  const unseenNow = (): Version[] => {
    const id = entity();
    return id === undefined ? [] : unseen(history, looked.get(id) ?? topVersion(history));
  };
  const lookAtAll = (): void => {
    const id = entity();
    if (id !== undefined) looked.set(id, topVersion(history));
  };

  const paintTrigger = (): void => {
    part('versions-trigger').innerHTML = trigger(unseenNow().length);
    const title = current.title ?? '';
    const head = part('versions-head');
    const note = part('versions-note');
    const top = history.versions[0];
    part('version-history').classList.toggle('is-panel', title !== '');
    part('versions-title').textContent = title;
    head.hidden = title === '';
    // In a titled panel the trigger sits in the head, else it stands alone.
    if (title === '') part('version-history').insertBefore(part('versions-trigger'), note);
    else head.append(part('versions-trigger'));
    note.classList.toggle('is-error', unreadable);
    note.hidden = !unreadable && (title === '' || top === undefined);
    note.textContent = unreadable
      ? 'Impossible de lire l’historique des versions.'
      : `${history.name || 'La fiche'} a été mise à jour : ${top ? sourceLabel(top) : ''}. Ouvrez l’historique pour revenir à une version antérieure.`;
  };

  const showStatus = (): void => {
    const status = modal?.el.querySelector<HTMLElement>('#versions-status');
    if (!status || !modal?.status) return;
    status.textContent = modal.status.text;
    status.className = `mf-versions__status${modal.status.kind === 'busy' ? '' : ` is-${modal.status.kind}`}`;
    status.hidden = false;
  };

  const say = (text: string, kind: Status['kind']): void => {
    if (!modal) return;
    modal.status = { text, kind };
    showStatus();
  };

  /** While a restore runs, what is on screen is what is sent: the buttons hold still. */
  const lock = (locked: boolean): void => {
    for (const button of modal?.el.querySelectorAll<HTMLButtonElement>('#versions-body button, #versions-revert') ?? []) button.disabled = locked;
    if (!locked) paintRevert();
  };

  /** « Annuler les changements cochés » is for a diff that has a version before it. */
  function paintRevert(): void {
    const button = modal?.el.querySelector<HTMLButtonElement>('#versions-revert');
    const target = history.versions.find((v) => v.v === modal?.shown);
    if (button) button.disabled = busy || !target || !hasPrevious(history, target);
  }

  /** The modal shows `target`'s diff above the whole history. */
  function paintModal(target: Version | undefined): void {
    if (!modal) return;
    modal.shown = target?.v;
    const inner = modal.el.querySelector('#versions-body');
    if (inner) inner.innerHTML = body(history, target, new Set(unseenNow().map((v) => v.v)));
    showStatus();
    paintRevert();
    lookAtAll();
    paintTrigger();
  }

  /** Reads the history of the element again; the answer of an element left behind is dropped. */
  async function fetchHistory(): Promise<boolean> {
    const id = entity();
    const mine = (generation += 1);
    if (id === undefined) {
      history = EMPTY;
      unreadable = false;
      paintTrigger();
      return true;
    }
    try {
      const read = parseHistory((await shell.capability(LIST_ENTITY_VERSIONS, undefined, { entity: id })).data);
      if (mine !== generation) return true;
      history = read;
      unreadable = false;
      if (!looked.has(id)) looked.set(id, seenUpTo(read));
      paintTrigger();
      // A history that moved under an open dialog keeps the diff the DM chose, and what the dialog said.
      if (modal && !busy) paintModal(read.versions.find((v) => v.v === modal?.shown) ?? read.versions[0]);
      return true;
    } catch {
      if (mine === generation) {
        unreadable = true;
        paintTrigger();
      }
      return false;
    }
  }
  const load = (): Promise<boolean> => fetchHistory();

  /** The trigger reads again, not trusting the last read: a save may be newer than the shell's next word. */
  async function openFresh(): Promise<void> {
    const id = entity();
    await load();
    // The panel moved on to another element while it read: that one's answer is not this dialog's.
    if (id === entity()) open();
  }

  function close(): void {
    modal?.close();
  }

  function open(focus?: Version): void {
    close();
    const first = focus ?? unseenNow()[0] ?? history.versions[0];
    const wrapper = doc.createElement('div');
    wrapper.innerHTML = dialog(history.name || 'Nouvelle fiche', body(history, first, new Set(unseenNow().map((v) => v.v))));
    const el = wrapper.firstElementChild as HTMLElement;
    doc.body.append(el);
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') close();
    };
    const mine: Modal = {
      el,
      shown: first?.v,
      status: null,
      close() {
        el.remove();
        doc.removeEventListener('keydown', onKey);
        if (modal === mine) modal = null;
      },
    };
    modal = mine;
    doc.addEventListener('keydown', onKey);
    el.addEventListener('click', onDialogClick);
    el.addEventListener('change', (event) => {
      // The kit's DiffView marks a change to undo; its own script is not on the shell's page.
      const box = event.target as HTMLInputElement | null;
      if (box?.classList.contains('kit-diffview__check')) box.closest('.kit-diffview__row')?.classList.toggle('kit-diffview__row--reverted', box.checked);
    });
    paintRevert();
    lookAtAll();
    paintTrigger();
  }

  function onDialogClick(event: Event): void {
    const target = event.target as Element | null;
    if (!target || !modal) return;
    if (target.classList.contains('kit-dialog__veil') || target.closest('.kit-dialog__close, #versions-close')) return close();
    const button = target.closest<HTMLButtonElement>('button[data-action]');
    if (!button || busy) return;
    const [act, number] = (button.dataset.action ?? '').split(':');
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes !== RESTORE_ENTITY_VERSION && act !== 'show') return;
    if (act === 'show') {
      const version = history.versions.find((v) => v.v === Number(number));
      if (modal && version) {
        modal.status = null;
        paintModal(version);
      }
    } else if (act === 'restore') void restore({ version: Number(number) }, `La v${number} est restaurée`);
    else if (act === 'revert') void revert();
  }

  /** The changes ticked in the diff on show are undone: the version before it, those changes. */
  async function revert(): Promise<void> {
    const shown = history.versions.find((v) => v.v === modal?.shown);
    if (!modal || !shown) return;
    const ticked = [...modal.el.querySelectorAll<HTMLElement>('#versions-diff .kit-diffview__row')]
      .filter((row) => row.querySelector<HTMLInputElement>('.kit-diffview__check')?.checked)
      .map((row) => Number(row.dataset.index));
    if (ticked.length === 0) return say('Cochez les changements à annuler.', 'error');
    await restore({ version: shown.v - 1, changes: ticked }, `${ticked.length} changement(s) de la v${shown.v} annulé(s)`);
  }

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(restore: Restore, id: string): Promise<Settled> {
    const answer = await shell.command(restoreBody(RESTORE_ENTITY_VERSION, id, restore, keyOf(id, restore)));
    const commandRef = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && commandRef !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(commandRef));
    }
    return state;
  }

  async function restore(what: Restore, done: string): Promise<void> {
    const id = entity();
    if (busy || id === undefined) return;
    busy = true;
    lock(true);
    say('Restauration en cours…', 'busy');
    try {
      const state = await send(what, id);
      if (state.status === 'rejected') {
        const why = reasons(state.violations);
        return say(why === '' ? 'Restauration refusée.' : `Restauration refusée : ${why}.`, 'error');
      }
      if (state.status !== 'applied') return say(`La restauration n’a pas abouti (état : ${state.status || 'inconnu'}).`, 'error');
      const read = await load();
      // The restoration is a version like any other: it opens the history, as the newest.
      if (read && id === entity()) {
        paintModal(history.versions[0]);
        say(`${done} : la v${topVersion(history)} est en tête de l’historique.`, 'ok');
      } else say(`${done}, mais l’historique n’a pas pu être relu.`, 'ok');
    } catch {
      say('La restauration a échoué.', 'error');
    } finally {
      busy = false;
      lock(false);
    }
  }

  host.addEventListener('click', (event) => {
    if (!(event.target as Element | null)?.closest('#versions-updated, #versions-open')) return;
    // What is on the button is the element's history as the server holds it now: read again, then open.
    void openFresh();
  });

  // The data moved under the panel (the AI, D&D Beyond, an editor saved): its history may have a new version.
  let first = true;
  const poll: { stop?: () => void } = {};
  poll.stop = shell.onDataVersion?.(() => {
    // A panel the shell took off the page lets go of the poll and of its dialog.
    if (!host.isConnected) {
      poll.stop?.();
      return close();
    }
    // The first word of the shell only says where the data stands, which the first read has.
    if (first) first = false;
    else if (!busy) void load();
  });

  paintTrigger();
  void load();

  return {
    update(next) {
      if (next.entity !== current.entity) {
        // Another element: what is on screen was the other's.
        close();
        history = EMPTY;
        current = next;
        paintTrigger();
      } else current = next;
      void load();
    },
    open: openFresh,
  };
}
