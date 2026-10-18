// SessionList (system session): a campaign's sessions in order, each with its
// summary and the PCs who were there, and a dialog that creates the next one
// with a title, a date and the PCs ticked. It reads `session.listSessions` and
// `campaign.listCharacters` and triggers `session.createSession`, by
// identifier, through the shell it is handed: no network call of its own.
// Only PCs are offered for ticking (`present-only-pj`); a closed session's
// summary shows in the list (`previous-summary-visible`).

import { CREATE_SESSION, LIST_CHARACTERS, LIST_SESSIONS } from './ids.ts';
import { commandBody, commandId, defaultTitle, isFinal, parseCharacters, parseSessions, payload, pcs, problem, reasons, settled, type Settled } from './model.ts';
import { dialog, items, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Character, Form, Session, Shell } from './types.ts';

/** What the list leans on outside itself: the clock, and how a command is followed until it settles. */
export interface Env {
  /** Today, `YYYY-MM-DD`: the date the form opens on. */
  today(): string;
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying the command has not settled. */
  polls: number;
}

const ENV: Env = {
  today: () => {
    const now = new Date();
    return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  },
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  everyMs: 400,
  polls: 120,
};

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="SessionList"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'SessionList';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the list and wires it. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, env: Env = ENV): void {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const campaign = typeof props.campaign === 'string' ? props.campaign : '';
  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`SessionList: #${id} is not in the list`);
    return found;
  };

  let sessions: Session[] = [];
  let characters: Character[] = [];

  const paint = (): void => {
    part('sessions-items').innerHTML = items(sessions, characters);
  };
  const say = (text: string): void => {
    part('sessions-status').textContent = text;
  };

  /** Reads both Capabilities; what cannot be read is said, and the other still shows. */
  async function refresh(): Promise<boolean> {
    const [listed, known] = await Promise.allSettled([shell.capability(LIST_SESSIONS, undefined, { campaign }), shell.capability(LIST_CHARACTERS, undefined, { campaign })]);
    if (listed.status === 'fulfilled') sessions = parseSessions(listed.value.data);
    if (known.status === 'fulfilled') characters = parseCharacters(known.value.data);
    paint();
    const ok = listed.status === 'fulfilled' && known.status === 'fulfilled';
    say(ok ? '' : listed.status === 'rejected' ? 'Impossible de lire les séances.' : 'Impossible de lire les personnages.');
    return ok;
  }

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(body: unknown): Promise<Settled> {
    const answer = await shell.command(body);
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < env.polls; asked += 1) {
      await env.sleep(env.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  let open: HTMLElement | null = null;
  let busy = false;

  const close = (): void => {
    open?.remove();
    open = null;
  };

  const field = (id: string): HTMLInputElement => {
    const found = open?.querySelector<HTMLInputElement>(`#${id}`);
    if (!found) throw new Error(`SessionList: #${id} is not in the dialog`);
    return found;
  };
  const ask = (): Form => ({
    title: field('session-create-title').value,
    plannedOn: field('session-create-date').value,
    present: Array.from(open?.querySelectorAll<HTMLInputElement>('input[name="present"]:checked') ?? []).map((box) => box.value),
  });
  const complain = (text: string): void => {
    const area = open?.querySelector('#session-create-error');
    if (area) area.textContent = text;
  };

  /** The dialog, over the list: PCs only, all ticked to begin with. */
  function openDialog(): void {
    if (open) return;
    const players = pcs(characters);
    const wrapper = host.ownerDocument.createElement('div');
    wrapper.innerHTML = dialog({ title: defaultTitle(sessions), plannedOn: env.today(), pcs: players, present: players.map((pc) => pc.id) });
    open = wrapper.firstElementChild as HTMLElement;
    host.append(open);
    open.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') close();
    });
    field('session-create-title').focus();
  }

  async function create(): Promise<void> {
    if (busy || !open) return;
    const form = ask();
    const why = problem(form);
    if (why) return complain(why);
    busy = true;
    const submit = part('session-create-submit') as HTMLButtonElement;
    submit.disabled = true;
    complain('');
    try {
      // The identifier the actuator names is what is sent, and nothing else.
      const state = await send(commandBody(submit.dataset.writes ?? CREATE_SESSION, payload(campaign, form, characters)));
      if (state.status === 'rejected') return complain(`Création refusée : ${reasons(state.violations)}.`);
      if (state.status !== 'applied') return complain(`La création n’a pas abouti (état : ${state.status || 'inconnu'}).`);
      close();
      // The new session is read back, at the end of the list; if it cannot be, the list says so.
      if (!(await refresh())) say('La séance est créée, mais la liste n’a pas pu être relue.');
    } catch {
      complain('La création a échoué.');
    } finally {
      busy = false;
      submit.disabled = false;
    }
  }

  host.addEventListener('click', (event) => {
    const target = event.target as Element | null;
    const button = target?.closest('button');
    if (button?.dataset.writes === CREATE_SESSION && button.id === 'sessions-new') openDialog();
    else if (button?.dataset.writes === CREATE_SESSION) void create();
    else if (button?.id === 'session-create-cancel' || button?.id === 'session-create-close') close();
    else if (target?.classList.contains('kit-dialog__veil')) close();
  });

  paint();
  void refresh();
}
