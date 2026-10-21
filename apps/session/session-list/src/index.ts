// SessionList (system session): a campaign's sessions in order, with their
// summary and the PCs present, and the form that adds the next one with a
// title, a date and the PCs ticked. It reads `session.listSessions` and
// `campaign.listCharacters` and triggers `session.createSession`, by
// identifier, through the shell it is handed: no network call of its own.
// Only the PCs are offered for ticking (`present-only-pj`).

import { CREATE_SESSION, LIST_CHARACTERS, LIST_SESSIONS } from './ids.ts';
import { commandBody, commandId, initialForm, isFinal, newKey, parseCharacters, parseSessions, payload, reasons, settled, type Settled } from './model.ts';
import { dialog, items, notice, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Character, Form, Notice, Reading, Session, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="SessionList"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'SessionList';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the list and wires it; the object it answers follows the props. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): { update(props: Record<string, unknown>): void } {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`SessionList: #${id} is not in the list`);
    return found;
  };

  const campaignOf = (raw: Record<string, unknown>): string => (typeof raw.campaign === 'string' ? raw.campaign : '');
  let campaign = campaignOf(props);
  let sessions: Session[] = [];
  let characters: Character[] = [];
  let reading: Reading = 'loading';
  let charactersRead = false;
  let form: Form = initialForm(sessions, characters);
  let opened = false;
  let busy = false;
  let loading = 0;
  /** The key of the command the open form sends: sent again, it creates nothing more. */
  let key = '';

  /** The form needs the sessions, to number the next one, and the characters, to offer the PCs: it opens once both are read. */
  const paintList = (): void => {
    part('sessions-items').innerHTML = items(sessions, characters, reading);
    const opener = part('sessions-new') as HTMLButtonElement;
    opener.disabled = reading !== 'ready' || !charactersRead;
    opener.title = reading === 'ready' && !charactersRead ? 'Impossible de lire les PJ de la campagne.' : '';
  };

  /** Both reads at once; a later read wins over an earlier one that answers after it. */
  async function load(): Promise<void> {
    const mine = (loading += 1);
    const variables = { campaign };
    const [read, people] = await Promise.allSettled([shell.capability(LIST_SESSIONS, undefined, variables), shell.capability(LIST_CHARACTERS, undefined, variables)]);
    if (mine !== loading) return;
    if (read.status === 'fulfilled') {
      sessions = parseSessions(read.value.data);
      reading = 'ready';
    } else if (reading !== 'ready') reading = 'failed';
    // The sessions already on screen stay when a read fails after a creation; the characters, then, are the last read.
    if (people.status === 'fulfilled') {
      characters = parseCharacters(people.value.data);
      charactersRead = true;
    }
    paintList();
  }

  /** An answer that comes after the form is gone (another campaign closed it) has no one to tell. */
  const say = (next: Notice): void => {
    const target = host.querySelector('#session-create-notice');
    if (target) target.innerHTML = notice(next);
  };

  /** While a command runs, the form holds still: what is sent is what is on screen. */
  const lock = (locked: boolean): void => {
    for (const control of host.querySelectorAll<HTMLInputElement | HTMLButtonElement>('#session-create-dialog input, #session-create-dialog button')) control.disabled = locked;
  };

  function open(): void {
    if (opened) return;
    opened = true;
    form = initialForm(sessions, characters);
    key = newKey();
    part('sessions-dialog').innerHTML = dialog(form, characters);
    host.querySelector<HTMLInputElement>('#session-create-title')?.focus();
  }

  function close(): void {
    if (!opened || busy) return;
    opened = false;
    part('sessions-dialog').innerHTML = '';
  }

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(): Promise<Settled> {
    const answer = await shell.command(commandBody(CREATE_SESSION, payload(campaign, form, sessions), key));
    const id = commandId(answer);
    let result = settled(answer);
    for (let asked = 0; !isFinal(result.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      result = settled(await shell.commandResult(id));
    }
    return result;
  }

  async function create(): Promise<void> {
    if (busy || !opened) return;
    busy = true;
    lock(true);
    say({ kind: 'busy' });
    let created = false;
    try {
      const result = await send();
      if (result.status === 'applied') created = true;
      else if (result.status === 'rejected') {
        // A refusal is the answer to that key: the DM's next try is another command.
        key = newKey();
        say({ kind: 'error', text: `Création refusée : ${reasons(result.violations)}.` });
      } else say({ kind: 'error', text: `La création n’a pas abouti (état : ${result.status || 'inconnu'}).` });
    } catch {
      say({ kind: 'error', text: 'La création a échoué.' });
    } finally {
      busy = false;
      lock(false);
    }
    if (!created) return;
    close();
    await load();
  }

  host.addEventListener('submit', (event) => event.preventDefault());

  // The form holds the focus while it is open, so its keys reach the list's own host.
  host.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') close();
  });

  host.addEventListener('input', (event) => {
    const target = event.target as { id?: string; value?: string } | null;
    if (target?.id === 'session-create-title') form = { ...form, title: target.value ?? '' };
    else if (target?.id === 'session-create-date') form = { ...form, date: target.value ?? '' };
    else return;
    key = newKey();
  });

  host.addEventListener('change', (event) => {
    // Read by shape, not by class: the list runs in whichever window the shell is.
    const target = event.target as { name?: string; value?: string; checked?: boolean } | null;
    if (target?.name !== 'present') return;
    const id = target.value ?? '';
    form = { ...form, present: target.checked ? [...form.present.filter((p) => p !== id), id] : form.present.filter((p) => p !== id) };
    key = newKey();
  });

  host.addEventListener('click', (event) => {
    const target = event.target as Element | null;
    if (target?.classList.contains('kit-dialog__veil')) return close();
    const button = target?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers; the opener only shows the form.
    if (button.id === 'sessions-new') open();
    else if (button.dataset.writes === CREATE_SESSION) void create();
    else if (button.id === 'session-create-cancel' || button.classList.contains('kit-dialog__close')) close();
    // The shell has no router: a button that names a page takes the DM there itself.
    else if (button.dataset.href) host.ownerDocument.defaultView?.location.assign(button.dataset.href);
  });

  paintList();
  void load();

  return {
    update(next) {
      const following = campaignOf(next);
      if (following === campaign) return;
      // Another campaign: nothing of the last one stays, neither its sessions nor a form half filled with its PCs.
      campaign = following;
      busy = false;
      close();
      sessions = [];
      characters = [];
      reading = 'loading';
      charactersRead = false;
      paintList();
      void load();
    },
  };
}
