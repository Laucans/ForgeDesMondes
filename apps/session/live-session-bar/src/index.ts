// LiveSessionBar (system session): the running session on one bar — its
// number, title and start, its scenes, what the players see — and, from it,
// the player view put back on the campaign cover, its floating window closed,
// and the session closed with a recap of what was kept. It reads
// `session.getLiveSession`, `session.listScenes` and `live.getDiffusion`, and
// publishes to the screen's context the live session, its campaign, the
// current scene and the battlemap of the live scene (nothing when there is no
// session: no id is made up), and triggers
// `live.showCover`, `live.closeWindow` and `session.closeSession`, by
// identifier, through the shell it is handed: no network call of its own.
// Once the session is closed it reads `session.getSessionJournal` and
// `session.getPreviousSessionSummary` for the recap. A closed session is
// never reopened from the bar.

import {
  BATTLEMAP_CONTEXT,
  CAMPAIGN_CONTEXT,
  CLOSE_SESSION,
  CLOSE_WINDOW,
  GET_DIFFUSION,
  GET_LIVE_SESSION,
  GET_PREVIOUS_SESSION_SUMMARY,
  GET_SESSION_JOURNAL,
  LIST_SCENES,
  SCENE_CONTEXT,
  SESSION_AGGREGATE,
  SESSION_CONTEXT,
  SHOW_COVER,
} from './ids.ts';
import {
  battlemapOf,
  commandBody,
  commandId,
  durationSince,
  isFinal,
  parseDiffusion,
  parseLive,
  parseRecap,
  parseScenes,
  reasons,
  settled,
  withScenes,
  type Settled,
} from './model.ts';
import { badges, broadcastActions, confirmDialog, heading, meta, recapDialog, scenes, showingHtml, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Diffusion, Live, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
  /** The time the session is closed at, for its duration. */
  now(): Date;
}

export const DEFAULT_TIMING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120, now: () => new Date() };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="LiveSessionBar"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'LiveSessionBar';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the bar and wires it. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = DEFAULT_TIMING): void {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`LiveSessionBar: #${id} is not in the bar`);
    return found;
  };

  // The page may name the session; else the Capability answers the one running.
  const requested = typeof props.session === 'string' && props.session !== '' ? { id: props.session } : undefined;
  let live: Live | null = null;
  let diffusion: Diffusion = parseDiffusion(undefined);
  let closed = false;
  let busy = false;

  const paintLive = (): void => {
    part('session-bar-title').textContent = live === null ? 'Aucune séance en cours' : heading(live);
    part('session-bar-meta').textContent = live === null ? '' : meta(live);
    part('session-bar-badges').innerHTML = live === null ? '' : badges(live, closed);
    part('session-bar-scenes').innerHTML = live === null ? '' : scenes(live);
    (part('session-bar-close') as HTMLButtonElement).disabled = live === null || closed;
  };
  const paintDiffusion = (): void => {
    part('session-bar-showing').innerHTML = showingHtml(diffusion);
    part('session-bar-broadcast-actions').innerHTML = broadcastActions(diffusion);
  };

  /** The scene the session is at, and the battlemap of the live scene, for the Micro-UIs that follow them. */
  const publishScene = (session: Live): void => {
    shell.context.set(SCENE_CONTEXT, session.currentScene);
    shell.context.set(BATTLEMAP_CONTEXT, battlemapOf(session));
  };

  const readLive = async (): Promise<void> => {
    const answered = parseLive((await shell.capability(GET_LIVE_SESSION, undefined, requested)).data);
    live = answered;
    paintLive();
    // No session under way: the screen's context is left as it is, no id is made up.
    if (answered === null) return;
    // The session and its campaign are the answer's own, set before the scenes are read: the panel needs no more.
    shell.context.set(SESSION_CONTEXT, answered.id);
    shell.context.set(CAMPAIGN_CONTEXT, answered.campaign === '' ? undefined : answered.campaign);
    // A scenes read that fails leaves the bar without scenes, not without its session.
    let scenesLost = false;
    const read = await shell.capability(LIST_SCENES, undefined, { session: answered.id }).then(
      (answer) => parseScenes(answer.data),
      () => {
        scenesLost = true;
        return [];
      },
    );
    // `live` may have moved while the scenes were read: it is the base, the answer only brings the scenes.
    live = withScenes(live ?? answered, read);
    paintLive();
    if (scenesLost) part('session-bar-scenes').textContent = 'Impossible de lire les scènes';
    publishScene(live);
  };
  const readDiffusion = async (): Promise<void> => {
    diffusion = parseDiffusion((await shell.capability(GET_DIFFUSION, undefined)).data);
    paintDiffusion();
  };

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, target?: { aggregate: string; id: string }): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, `${dataCapability}:${crypto.randomUUID()}`, target));
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  /** What the players see changes by a command; the bar then reads it again. */
  async function broadcast(dataCapability: string): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      const state = await send(dataCapability);
      if (state.status === 'rejected') part('session-bar-showing').textContent = `Refusé : ${reasons(state.violations)}.`;
      else if (state.status !== 'applied') part('session-bar-showing').textContent = `La commande n’a pas abouti (état : ${state.status || 'inconnu'}).`;
      else await readDiffusion();
    } catch {
      part('session-bar-showing').textContent = 'La commande a échoué.';
    } finally {
      busy = false;
    }
  }

  const modal = (): HTMLElement => part('session-close-modal');
  const show = (html: string): void => {
    modal().innerHTML = html;
    modal().hidden = false;
    // The dialog takes the focus, so the keyboard stays in it.
    (modal().querySelector<HTMLElement>('.kit-dialog__footer button:not(:disabled)') ?? modal().querySelector<HTMLElement>('button'))?.focus();
  };
  const hide = (): void => {
    modal().hidden = true;
    modal().innerHTML = '';
  };

  /** The recap is read once the session is closed: its journal, its summary and the time it ran. */
  async function recap(session: Live): Promise<void> {
    const duration = durationSince(session.startedAt, timing.now());
    const [journal, summary] = await Promise.allSettled([
      shell.capability(GET_SESSION_JOURNAL, undefined, { session: session.id }),
      shell.capability(GET_PREVIOUS_SESSION_SUMMARY, undefined, { session: session.id }),
    ]);
    const lost = journal.status === 'rejected' || summary.status === 'rejected';
    show(
      recapDialog(
        session,
        parseRecap(journal.status === 'fulfilled' ? journal.value.data : undefined, summary.status === 'fulfilled' ? summary.value.data : undefined, duration),
        lost ? 'La séance est close, mais son bilan n’a pas pu être lu en entier.' : '',
      ),
    );
  }

  async function closeSession(): Promise<void> {
    const session = live;
    if (busy || session === null || closed) return;
    busy = true;
    show(confirmDialog(session, '', true));
    try {
      const state = await send(CLOSE_SESSION, { aggregate: SESSION_AGGREGATE, id: session.id });
      if (state.status === 'rejected') return show(confirmDialog(session, `Clôture refusée : ${reasons(state.violations)}.`));
      if (state.status !== 'applied') return show(confirmDialog(session, `La clôture n’a pas abouti (état : ${state.status || 'inconnu'}).`));
    } catch {
      return show(confirmDialog(session, 'La clôture a échoué.'));
    } finally {
      busy = false;
    }
    // Applied: the session is closed whatever happens to its recap.
    closed = true;
    paintLive();
    await recap(session);
  }

  host.addEventListener('click', (event) => {
    const target = event.target as Element | null;
    const button = target?.closest('button');
    if (!button || button.disabled) return;
    // A button that opens a view says where (the shell has no page-wide handler for it); one that writes says
    // which DataCapability, and that, and nothing else, is what it sends.
    if (button.dataset.href !== undefined) {
      event.stopPropagation();
      if (button.dataset.target === '_blank') host.ownerDocument.defaultView?.open(button.dataset.href, '_blank', 'noopener');
      else host.ownerDocument.defaultView?.location.assign(button.dataset.href);
    } else if (button.dataset.scene !== undefined && live !== null) {
      live = { ...live, currentScene: Number(button.dataset.scene) };
      paintLive();
      publishScene(live);
    } else if (button.dataset.writes === SHOW_COVER || button.dataset.writes === CLOSE_WINDOW) void broadcast(button.dataset.writes);
    // « Clore la séance » on the bar only asks; the dialog's own button is what sends the command.
    else if (button.id === 'session-bar-close' && live !== null) show(confirmDialog(live));
    else if (button.id === 'session-close-submit') void closeSession();
    else if (button.id === 'session-close-cancel' || button.id === 'session-close-x') hide();
  });

  host.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !modal().hidden && !busy) hide();
  });

  paintLive();
  paintDiffusion();
  part('session-bar-title').textContent = 'Lecture de la séance…';
  void readLive().catch(() => {
    part('session-bar-title').textContent = 'Impossible de lire la séance.';
  });
  void readDiffusion().catch(() => {
    part('session-bar-showing').textContent = 'Impossible de lire ce que voient les joueurs.';
  });
  // The data moved elsewhere (the DM view, another tab): what the players see is read again.
  shell.onDataVersion(() => {
    if (!busy) void readDiffusion().catch(() => undefined);
  });
}
