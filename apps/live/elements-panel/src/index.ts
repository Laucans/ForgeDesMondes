// ElementsPanel (system live): the DM view's left panel. The current session
// first, scene by scene, with the places, NPCs, events and media each scene
// links; then the whole campaign by type. A search box filters on names and
// aliases at every keystroke, terms normalised per AliasMatch@1 (case,
// accents and surrounding spaces ignored). A click opens an element at the
// centre — the panel tells the screen (`fdm:open`, `fdm:open-map`), which
// routes it — and "Envoyer" shows a battlemap or an illustration to the
// players: `live.showOnPlayerView`, and `live.showWindowOnPlayerView` for an
// illustration in the floating window, submitted by identifier through the
// shell it is handed. It reads `session.listScenes`, `campaign.listCharacters`,
// `campaign.listPlaces`, `campaign.listEvents` and `media.listMedia`, for the
// session and the campaign the screen's context names; no network call of its own.
// The players receive only what the DM sends: nothing here reaches them otherwise.

import {
  CAMPAIGN_CONTEXT,
  CHARACTER_DRAG_TYPE,
  LIST_CHARACTERS,
  LIST_EVENTS,
  LIST_MEDIA,
  LIST_PLACES,
  LIST_SCENES,
  OPEN_EVENT,
  OPEN_MAP_EVENT,
  SCENE_EVENT,
  SESSION_CONTEXT,
  SHOW_ON_PLAYER_VIEW,
  SHOW_WINDOW_ON_PLAYER_VIEW,
} from './ids.ts';
import {
  buildIndex,
  campaignGroups,
  commandBody,
  commandId,
  isFinal,
  normalize,
  parseCharacters,
  parseEvents,
  parseLibrary,
  parsePlaces,
  parseScenes,
  reasons,
  sceneGroups,
  search,
  settled,
  type Settled,
} from './model.ts';
import { groups, results, scenes, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Item, Shell, World } from './types.ts';

/** How a command is followed until it settles. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60 };

const EMPTY: World = { scenes: [], characters: [], places: [], events: [], library: { battlemaps: [], illustrations: [] } };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="ElementsPanel"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'ElementsPanel';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the panel and wires it. */
export function mount(host: HTMLElement, _raw: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): void {
  const doc = host.ownerDocument;
  ensureStyle(doc);

  let world: World = EMPTY;
  let index: Item[] = [];
  let generation = 0;

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (found === null) throw new Error(`the panel has no #${id}`);
    return found;
  };
  const named = (name: string): string => {
    const value = shell.context?.get(name);
    return typeof value === 'string' ? value : '';
  };
  const session = (): string => named(SESSION_CONTEXT);
  const campaign = (): string => named(CAMPAIGN_CONTEXT);

  function say(message: string, failed = false): void {
    const box = part('elements-status');
    box.textContent = message;
    box.hidden = message === '';
    box.classList.toggle('is-error', failed);
  }

  /** What the screen routes: the centre shows an element's sheet, or the battlemap. */
  function emit(name: string, detail: Record<string, unknown>): void {
    const view = doc.defaultView;
    if (view) doc.dispatchEvent(new view.CustomEvent(name, { detail }));
  }

  // ---------- painting ----------
  function paintSearch(): void {
    const query = part<HTMLInputElement>('elements-search-input').value;
    const asked = normalize(query) !== '';
    part('elements-results').hidden = !asked;
    part('elements-session').hidden = asked;
    part('elements-campaign').hidden = asked;
    if (!asked) return;
    const found = search(index, query);
    part('elements-results-title').textContent = found.length > 0 ? `${found.length} résultat${found.length > 1 ? 's' : ''} pour « ${query.trim()} »` : `Rien pour « ${query.trim()} »`;
    part('elements-results-list').innerHTML = results(found);
  }

  function paint(): void {
    part('elements-scenes').innerHTML = scenes(world.scenes, (scene) => sceneGroups(scene, world, index));
    part('elements-groups').innerHTML = groups(campaignGroups(index));
    paintSearch();
  }

  // ---------- reading ----------
  /** The five reads, for the session and the campaign the screen names; an older answer never replaces a newer one. */
  async function start(): Promise<void> {
    generation += 1;
    const mine = generation;
    const [asked, forCampaign] = [session() === '' ? {} : { session: session() }, campaign() === '' ? {} : { campaign: campaign() }];
    const read = (id: string, variables: Record<string, unknown>): Promise<unknown> => shell.capability(id, undefined, variables).then((answer) => answer.data);
    const answers = await Promise.allSettled([
      read(LIST_SCENES, asked),
      read(LIST_CHARACTERS, forCampaign),
      read(LIST_PLACES, forCampaign),
      read(LIST_EVENTS, forCampaign),
      read(LIST_MEDIA, forCampaign),
    ]);
    if (mine !== generation) return;
    const [s, c, p, e, m] = answers.map((a) => (a.status === 'fulfilled' ? a.value : undefined));
    world = { scenes: parseScenes(s), characters: parseCharacters(c), places: parsePlaces(p), events: parseEvents(e), library: parseLibrary(m) };
    index = buildIndex(world);
    say(answers.some((a) => a.status === 'rejected') ? 'Une partie du panneau n’a pas pu être lue.' : '', true);
    paint();
  }

  // ---------- commands ----------
  /** The command is sent, then followed until the server has applied or refused it. */
  async function submit(body: unknown): Promise<Settled> {
    const answer = await shell.command(body);
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  async function send(dataCapability: string, payload: Record<string, unknown>, sent: string): Promise<void> {
    say('Envoi aux joueurs…');
    try {
      const state = await submit(commandBody(dataCapability, campaign() === '' ? payload : { ...payload, campaign: campaign() }, session()));
      if (state.status === 'applied') say(sent);
      else if (state.status === 'rejected') say(`Pas envoyé : ${reasons(state.violations)}.`, true);
      else say(`L’envoi n’a pas abouti (état : ${state.status || 'inconnu'}).`, true);
    } catch {
      say('L’envoi n’a pas pu être fait.', true);
    }
  }

  const nameOf = (type: Item['type'], id: string): string => index.find((item) => item.type === type && item.id === id)?.name ?? id;

  // ---------- the DM's gestures ----------
  host.addEventListener('input', (event) => {
    if ((event.target as Element | null)?.id === 'elements-search-input') paintSearch();
  });

  host.addEventListener('dragstart', (event: DragEvent) => {
    const row = (event.target as Element | null)?.closest<HTMLElement>('[data-char]');
    const transfer = event.dataTransfer;
    if (!row || !transfer) return;
    const id = row.dataset.char ?? '';
    transfer.setData(CHARACTER_DRAG_TYPE, id);
    transfer.setData('text/plain', nameOf('character', id));
    transfer.effectAllowed = 'copy';
    // The map is where it is dropped: the centre shows it.
    emit(OPEN_MAP_EVENT, {});
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest<HTMLButtonElement>('button');
    if (!button || button.disabled) return;
    const { open, openMap, sendMap, sendScene, sendWindow } = button.dataset;
    if (open !== undefined) {
      const cut = open.indexOf(':');
      emit(OPEN_EVENT, { type: open.slice(0, cut), id: open.slice(cut + 1) });
    }
    if (openMap !== undefined) emit(OPEN_MAP_EVENT, { id: openMap });
    if (sendMap !== undefined) void send(SHOW_ON_PLAYER_VIEW, { kind: 'battlemap', instance: sendMap }, 'Battlemap envoyée aux joueurs.');
    if (sendScene !== undefined) void send(SHOW_ON_PLAYER_VIEW, { kind: 'illustration', media: sendScene }, `« ${nameOf('illustration', sendScene)} » envoyée aux joueurs en scène.`);
    if (sendWindow !== undefined) void send(SHOW_WINDOW_ON_PLAYER_VIEW, { media: sendWindow }, `« ${nameOf('illustration', sendWindow)} » envoyée aux joueurs en fenêtre.`);
  });

  // The scene the session is at opens; the others fold.
  doc.addEventListener(SCENE_EVENT, (event) => {
    const detail = (event as CustomEvent<{ index?: number }>).detail;
    host.querySelectorAll<HTMLDetailsElement>('#elements-scenes details').forEach((d) => {
      const current = Number(d.dataset.sceneIndex) === detail.index;
      d.open = current;
      d.classList.toggle('is-current', current);
    });
  });

  // The screen sets the session and the campaign once it knows them: the panel reads for them then.
  shell.context?.on?.((name) => {
    if (name === SESSION_CONTEXT || name === CAMPAIGN_CONTEXT) void start();
  });

  host.innerHTML = skeleton();
  paint();
  void start();
}
