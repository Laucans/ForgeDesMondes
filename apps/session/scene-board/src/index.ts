// SceneBoard (system session): the session's preparation on one board — the
// PCs present, the scenes in their order, and for each scene its battlemap
// instance or the campaign's battlemaps to make one from. It reads
// `session.getSession`, `session.listScenes`, `campaign.listCharacters`,
// `campaign.listPlaces`, `campaign.listEvents`, `media.listMedia` and
// `table.getBattlemapInstance`, and triggers
// `session.addPresentCharacter`, `session.removePresentCharacter`,
// `session.addScene`, `session.reorderScenes`, `session.updateSceneNotes` and
// `table.createBattlemapInstance`, by identifier, through the shell it is
// handed: no network call of its own. Every command is the DM's, as the
// approver of the Session's critical fields, so it applies rather than
// waits in review; a reorder sends the scene ids in their new order, and a
// scene's notes are written when the DM leaves the field. It does not show or drive the map. The
// session is its `session` prop, else the screen's `session` context value;
// with neither it reads nothing and writes nothing. The campaign its
// campaign reads are for is the one session.getSession gives, else its
// `campaign` prop; with neither they are asked with no campaign.

import {
  ADD_PRESENT_CHARACTER,
  ADD_SCENE,
  CREATE_BATTLEMAP_INSTANCE,
  GET_BATTLEMAP_INSTANCE,
  GET_SESSION,
  LIST_CHARACTERS,
  LIST_EVENTS,
  LIST_MEDIA,
  LIST_PLACES,
  LIST_SCENES,
  REMOVE_PRESENT_CHARACTER,
  REORDER_SCENES,
  UPDATE_SCENE_NOTES,
} from './ids.ts';
import { campaignAsked, commandBody, commandId, isFinal, moved, parseCharacters, parseEntities, parseInstance, parseMedias, parseScenes, parseSession, reasons, sessionAsked, settled, type Settled } from './model.ts';
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
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): void {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`SceneBoard: #${id} is not in the board`);
    return found;
  };

  // The prop first (`session.getSession` is asked by `id`), the screen's context value when no prop names one.
  const wanted = sessionAsked(props.session, shell.context?.get('session'));
  let board: Board | null = null;
  // The commands not yet settled, in the order the DM gave them: one runs at a time, none is dropped.
  let pending: Promise<void> = Promise.resolve();

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

  /** The seven reads the board shows: the session first, which names the campaign the others are read for (else the `campaign` prop does). */
  async function read(): Promise<Board> {
    const session = parseSession((await shell.capability(GET_SESSION, undefined, { id: wanted })).data);
    const campaign = campaignAsked(session.campaign, props.campaign);
    const forCampaign = campaign === null ? {} : { campaign };
    const [scenes, characters, places, events, medias] = await Promise.all([
      shell.capability(LIST_SCENES, undefined, { session: wanted }).then((answer) => parseScenes(answer.data)),
      shell.capability(LIST_CHARACTERS, undefined, forCampaign).then((answer) => parseCharacters(answer.data)),
      shell.capability(LIST_PLACES, undefined, forCampaign).then((answer) => parseEntities(answer.data)),
      shell.capability(LIST_EVENTS, undefined, forCampaign).then((answer) => parseEntities(answer.data)),
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
    return { session, scenes, characters, places, events, medias, instances };
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

  /** One command at a time, each waiting its turn rather than dropped: a note saved as the DM leaves the field, and the click that left it, are both sent. */
  function act(wrong: string, run: (current: Board) => (() => Promise<Settled>) | null, applied?: () => void): Promise<void> {
    pending = pending.then(() => attempt(wrong, run, applied)).catch(() => undefined);
    return pending;
  }

  /** The command is built from the board as it is when its turn comes, not as it was clicked: a command the board no longer allows is not sent. Once applied, the board is read again, so what it shows is what the server holds — unless `applied` says what changed, which keeps the field the DM is in; a command not yet applied leaves that field as typed, and `applied` unsaid. */
  async function attempt(wrong: string, run: (current: Board) => (() => Promise<Settled>) | null, applied?: () => void): Promise<void> {
    if (board === null) return;
    // A session the board does not know is one it cannot write to.
    if (board.session.id === '') return say('La séance n’est pas identifiée : rien ne peut être modifié.');
    const sending = run(board);
    if (sending === null) return;
    host.querySelector('#scene-board')?.setAttribute('aria-busy', 'true');
    say('');
    try {
      const state = await sending();
      if (state.status === 'rejected') say(`Action refusée : ${reasons(state.violations)}.`);
      else {
        // A command still queued or parked is the server's to apply on its own clock: the board says so, and shows what is held now.
        if (state.status !== 'applied') say(`L’action n’est pas encore appliquée (état : ${state.status || 'inconnu'}).`);
        try {
          if (applied !== undefined) {
            if (state.status === 'applied') applied();
          } else {
            board = await read();
            paint();
          }
        } catch {
          say('L’action a eu lieu, mais la séance n’a pas pu être relue.');
        }
      }
    } catch {
      say(wrong);
    } finally {
      host.querySelector('#scene-board')?.removeAttribute('aria-busy');
    }
  }

  host.addEventListener('click', (event) => {
    const clicked = (event.target as Element | null)?.closest('button');
    if (!clicked || board === null) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    switch (clicked.dataset.writes) {
      case REMOVE_PRESENT_CHARACTER: {
        const character = clicked.dataset.removePj ?? '';
        void act('Impossible de retirer ce PJ.', ({ session: { id: session } }) => () => send(REMOVE_PRESENT_CHARACTER, { session, character }, 'Session', session));
        return;
      }
      case ADD_PRESENT_CHARACTER: {
        const character = host.querySelector<HTMLSelectElement>('#scenes-present-select')?.value ?? '';
        if (character === '') return;
        void act('Impossible d’ajouter ce PJ.', ({ session: { id: session } }) => () => send(ADD_PRESENT_CHARACTER, { session, character }, 'Session', session));
        return;
      }
      case ADD_SCENE:
        void act('Impossible d’ajouter une scène.', ({ session: { id: session } }) => () => send(ADD_SCENE, { session, title: 'Nouvelle scène' }, 'Session', session));
        return;
      case REORDER_SCENES: {
        const scene = clicked.closest<HTMLElement>('[data-scene]')?.dataset.scene ?? '';
        const delta = Number(clicked.dataset.move);
        if (moved(board.scenes, scene, delta) === null) return;
        // The move is made on the order the server holds when it is sent, so two quick moves add up rather than the second undoing the first.
        void act('Impossible de réordonner les scènes.', (current) => {
          const order = moved(current.scenes, scene, delta);
          const session = current.session.id;
          return order === null ? null : () => send(REORDER_SCENES, { session, order }, 'Session', session);
        });
        return;
      }
      case CREATE_BATTLEMAP_INSTANCE: {
        const item = clicked.closest<HTMLElement>('[data-scene]');
        const id = item?.dataset.scene;
        const battlemap = item?.querySelector<HTMLSelectElement>('select[data-map-select]')?.value ?? '';
        if (battlemap === '') return;
        // one-instance-per-scene: a scene that has its instance, by the time the command is sent, offers no second one. The key is one click's: a retry after a refusal is a new command, never the first one's replayed result.
        const key = `${CREATE_BATTLEMAP_INSTANCE}:${id ?? ''}:${Date.now()}`;
        void act('Impossible de créer l’instance.', (current) => {
          const scene = current.scenes.find((s) => s.id === id);
          const session = current.session.id;
          return !scene || scene.battlemap !== null ? null : () => send(CREATE_BATTLEMAP_INSTANCE, { session, scene: scene.id, battlemap }, 'BattlemapInstance', scene.id, key);
        });
        return;
      }
    }
    // « Placer les personnages » only leads to the map, which the Battlemap Micro-UI shows; it writes nothing.
    if (clicked.dataset.action === 'place') host.ownerDocument.getElementById('scene-battlemap')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  });

  // A scene's notes are written when the DM leaves the field, once, and only when they differ from what the board holds when their turn comes.
  host.addEventListener('change', (event) => {
    const field = event.target as HTMLTextAreaElement | null;
    if (field?.tagName !== 'TEXTAREA' || field.dataset.writes !== UPDATE_SCENE_NOTES || board === null) return;
    const id = field.closest<HTMLElement>('[data-scene]')?.dataset.scene;
    const notes = field.value;
    void act(
      'Impossible d’enregistrer ces notes.',
      (current) => {
        const scene = current.scenes.find((s) => s.id === id);
        const session = current.session.id;
        return !scene || notes === scene.notes ? null : () => send(UPDATE_SCENE_NOTES, { session, scene: scene.id, notes }, 'Session', scene.id);
      },
      () => {
        // The board may have been read again while the note waited its turn: the scene held now is the one updated, and a repainted field shows the note unless the DM is in it.
        const held = board?.scenes.find((s) => s.id === id);
        if (held) held.notes = notes;
        const shown = [...host.querySelectorAll<HTMLElement>('[data-scene]')]
          .find((item) => item.dataset.scene === id)
          ?.querySelector<HTMLTextAreaElement>(`textarea[data-writes="${UPDATE_SCENE_NOTES}"]`);
        if (shown && shown !== field && shown !== host.ownerDocument.activeElement) shown.value = notes;
      },
    );
  });

  // No session named, nothing to read: the board stays empty and writes nothing.
  if (wanted === '') {
    say('Aucune séance n’est indiquée : rien à afficher.');
    return;
  }
  read().then(
    (loaded) => {
      board = loaded;
      paint();
    },
    () => say('Impossible de lire la séance.'),
  );
}
