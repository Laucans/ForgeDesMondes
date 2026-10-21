// SceneBoard (system session): the session's preparation on one board — the
// PCs present, the scenes in their order, and for each scene its battlemap
// instance or the campaign's battlemaps to make one from. It reads
// `session.getSession`, `session.listScenes`, `campaign.listCharacters`,
// `media.listMedia` and `table.getBattlemapInstance`, and triggers
// `session.addPresentCharacter`, `session.removePresentCharacter`,
// `session.addScene`, `session.reorderScenes` and
// `table.createBattlemapInstance`, by identifier, through the shell it is
// handed: no network call of its own. It does not show or drive the map.

import {
  ADD_PRESENT_CHARACTER,
  ADD_SCENE,
  CREATE_BATTLEMAP_INSTANCE,
  GET_BATTLEMAP_INSTANCE,
  GET_SESSION,
  LIST_CHARACTERS,
  LIST_MEDIA,
  LIST_SCENES,
  REMOVE_PRESENT_CHARACTER,
  REORDER_SCENES,
} from './ids.ts';
import { commandBody, commandId, isFinal, moved, parseCharacters, parseInstance, parseMedias, parseScenes, parseSession, ranks, reasons, sessionVariables, settled, type Settled } from './model.ts';
import { presentAdd, presentCount, presentList, sceneList, sceneTitle, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Board, Instance, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="SceneBoard"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'SceneBoard';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the board and wires it. */
export function mount(host: HTMLElement, _props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): void {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`SceneBoard: #${id} is not in the board`);
    return found;
  };

  let board: Board | null = null;
  let busy = false;

  const say = (text: string): void => {
    part('scenes-notice').textContent = text;
  };

  const paint = (): void => {
    if (board === null) return;
    part('scenes-title').textContent = sceneTitle(board);
    part('scenes-present-count').textContent = presentCount(board);
    part('scenes-present-list').innerHTML = presentList(board);
    part('scenes-present-add').innerHTML = presentAdd(board);
    part('scenes-list').innerHTML = sceneList(board);
  };

  /** The five reads the board shows: the session first, which names the campaign the others are read for. */
  async function read(): Promise<Board> {
    const screenSession = shell.context?.get('session');
    const asked = sessionVariables(typeof screenSession === 'string' ? screenSession : undefined);
    const session = parseSession((await shell.capability(GET_SESSION, undefined, asked)).data);
    const forCampaign = session.campaign === null ? {} : { campaign: session.campaign };
    const [scenes, characters, medias] = await Promise.all([
      shell.capability(LIST_SCENES, undefined, { session: session.id || undefined }).then((answer) => parseScenes(answer.data)),
      shell.capability(LIST_CHARACTERS, undefined, forCampaign).then((answer) => parseCharacters(answer.data)),
      shell.capability(LIST_MEDIA, undefined, forCampaign).then((answer) => parseMedias(answer.data)),
    ]);
    const instances: Record<string, Instance> = {};
    await Promise.all(
      scenes.flatMap((scene) =>
        scene.battlemap === null
          ? []
          : [
              shell
                .capability(GET_BATTLEMAP_INSTANCE, undefined, { instance: scene.battlemap, view: 'dm' })
                .then((answer) => parseInstance(answer.data))
                // An instance that cannot be read still is the scene's: the board shows it without its figures.
                .catch(() => null)
                .then((instance) => {
                  if (instance !== null) instances[instance.id] = instance;
                }),
            ],
      ),
    );
    return { session, scenes, characters, medias, instances };
  }

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, payload: Record<string, unknown>, aggregate: string, id: string, key?: string): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, payload, { aggregate, id }, key));
    const commandSent = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && commandSent !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(commandSent));
    }
    return state;
  }

  /** One command at a time; once applied, the board is read again, so what it shows is what the server holds. */
  async function act(wrong: string, run: () => Promise<Settled>): Promise<void> {
    if (busy || board === null) return;
    // A session the board does not know is one it cannot write to.
    if (board.session.id === '') return say('La séance n’est pas identifiée : rien ne peut être modifié.');
    busy = true;
    host.querySelector('#scene-board')?.setAttribute('aria-busy', 'true');
    say('');
    try {
      const state = await run();
      if (state.status === 'rejected') say(`Action refusée : ${reasons(state.violations)}.`);
      else {
        // A command still queued or parked is the server's to apply on its own clock: the board says so, and shows what is held now.
        if (state.status !== 'applied') say(`L’action n’est pas encore appliquée (état : ${state.status || 'inconnu'}).`);
        try {
          board = await read();
          paint();
        } catch {
          say('L’action a eu lieu, mais la séance n’a pas pu être relue.');
        }
      }
    } catch {
      say(wrong);
    } finally {
      busy = false;
      host.querySelector('#scene-board')?.removeAttribute('aria-busy');
    }
  }

  host.addEventListener('click', (event) => {
    const clicked = (event.target as Element | null)?.closest('button');
    if (!clicked || board === null) return;
    const current = board;
    const session = current.session.id;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    switch (clicked.dataset.writes) {
      case REMOVE_PRESENT_CHARACTER: {
        const character = clicked.dataset.removePj ?? '';
        void act('Impossible de retirer ce PJ.', () => send(REMOVE_PRESENT_CHARACTER, { session, character }, 'Session', session));
        return;
      }
      case ADD_PRESENT_CHARACTER: {
        const character = host.querySelector<HTMLSelectElement>('#scenes-present-select')?.value ?? '';
        if (character === '') return;
        void act('Impossible d’ajouter ce PJ.', () => send(ADD_PRESENT_CHARACTER, { session, character }, 'Session', session));
        return;
      }
      case ADD_SCENE:
        void act('Impossible d’ajouter une scène.', () => send(ADD_SCENE, { session, title: 'Nouvelle scène' }, 'Session', session));
        return;
      case REORDER_SCENES: {
        const order = moved(current.scenes, clicked.closest<HTMLElement>('[data-scene]')?.dataset.scene ?? '', Number(clicked.dataset.move));
        if (order === null) return;
        void act('Impossible de réordonner les scènes.', () => send(REORDER_SCENES, { session, order: ranks(order) }, 'Session', session));
        return;
      }
      case CREATE_BATTLEMAP_INSTANCE: {
        const item = clicked.closest<HTMLElement>('[data-scene]');
        const scene = current.scenes.find((s) => s.id === item?.dataset.scene);
        const battlemap = item?.querySelector<HTMLSelectElement>('select[data-map-select]')?.value ?? '';
        // one-instance-per-scene: a scene that has its instance offers no second one. The key is one click's: a retry after a refusal is a new command, never the first one's replayed result.
        if (!scene || scene.battlemap !== null || battlemap === '') return;
        void act('Impossible de créer l’instance.', () => send(CREATE_BATTLEMAP_INSTANCE, { session, scene: scene.id, battlemap }, 'BattlemapInstance', scene.id, `${CREATE_BATTLEMAP_INSTANCE}:${scene.id}:${Date.now()}`));
        return;
      }
    }
    // « Placer les personnages » only leads to the map, which the Battlemap Micro-UI shows; it writes nothing.
    if (clicked.dataset.action === 'place') host.ownerDocument.getElementById('scene-battlemap')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  });

  read().then(
    (loaded) => {
      board = loaded;
      paint();
    },
    () => say('Impossible de lire la séance.'),
  );
}
