// Battlemap (system table): the DM's battlemap on a hex grid, and the same map
// as the players may see it. The DM drags a token and reads « 3 hex · 15 pi »
// under the cursor, drops a character « × N », opens a token's menu (rename,
// hide or reveal, condition, remove) and sends the map to the players with the
// zoom and pan he framed. It reads `table.getBattlemapInstance` (and the
// battlemap and the characters), and triggers the table's commands and
// `live.showOnPlayerView`, by identifier, through the shell it is handed: no
// network call of its own. A token's (q, r) is the only position there is
// (`single-position-source`): a drop sends it, and the map is read again.

import {
  ADD_PARTICIPANT,
  CHANGE_HIT_POINTS,
  END_COMBAT,
  GET_BATTLEMAP,
  GET_BATTLEMAP_INSTANCE,
  LIST_CHARACTERS,
  MOVE_TOKEN,
  NEXT_TURN,
  PLACE_TOKENS,
  REMOVE_PARTICIPANT,
  REMOVE_TOKEN,
  RENAME_TOKEN,
  REORDER_INITIATIVE,
  RESET_BATTLEMAP,
  SET_TOKEN_CONDITION,
  SET_TOKEN_VISIBILITY,
  SHOW_ON_PLAYER_VIEW,
  START_COMBAT,
} from './ids.ts';
import { distance, gridPath, label, line, onMap, toHex } from './hexgrid.ts';
import {
  commandBody,
  commandId,
  countOf,
  existingToken,
  hitPointsAfter,
  initiativeOf,
  inserted,
  isFinal,
  parseCharacters,
  parseInstance,
  parseMap,
  parseProps,
  reasons,
  reordered,
  settled,
  shown,
  tableCommand,
  turnAfter,
  withCondition,
  without,
  numbering,
  type Settled,
} from './model.ts';
import { carousel, dropForm, hexMap, inspector, pathMarkup, resetDialog, roster, skeleton, startDialog, toolbar, tokenItem, type Starter } from './render.ts';
import { STYLE } from './style.ts';
import type { Character, Frame, Hex, Instance, MapInfo, Props, Shell, Token } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
  /** How often the data version is asked: a drop shows on the other view within a second. */
  watchMs: number;
  /** The d20 an NPC rolls for its initiative, 1 to 20. */
  roll?: () => number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 500, roll: () => 1 + Math.floor(Math.random() * 20) };

/** What the shell keeps of a mounted Micro-UI: new props follow the screen's context. */
export interface Mounted {
  update(raw: Record<string, unknown>): void;
}

const DRAG_THRESHOLD = 4;
const CHARACTER_TYPE = 'text/fdm-character';
const PARTICIPANT_TYPE = 'text/fdm-participant';
const DM_ONLY = new Set(['bm-start-combat', 'bm-end-combat', 'bm-reset', 'combat-start-submit', 'battlemap-reset-submit', 'combat-next', 'combat-earlier', 'combat-later', 'combat-drop', 'combat-add', 'inspector-damage', 'inspector-heal']);

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="Battlemap"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'Battlemap';
  style.textContent = STYLE;
  doc.head.append(style);
}

interface Drag {
  token: Token;
  el: HTMLElement;
  from: Hex;
  x: number;
  y: number;
  moved: boolean;
  to: Hex | null;
}

interface Pan {
  x: number;
  y: number;
  panX: number;
  panY: number;
  moved: boolean;
}

/** What the shell calls: fills `host` with the map and wires it. */
export function mount(host: HTMLElement, raw: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): Mounted {
  ensureStyle(host.ownerDocument);

  let props: Props = parseProps(raw);
  let instance: Instance | null = null;
  let map: MapInfo | null = null;
  let characters: Record<string, Character> = {};
  let selected: string | null = null;
  let frame: Frame = { zoom: 1, panX: 0, panY: 0 };
  let fitted = false;
  let drag: Drag | null = null;
  let pan: Pan | null = null;
  let stale = false;
  let generation = 0;
  let readNo = 0;
  let stopWatching: () => void = () => undefined;
  let observer: ResizeObserver | null = null;

  const part = <T extends HTMLElement = HTMLElement>(id: string): T | null => host.querySelector<T>(`#${id}`);
  const canMove = (): boolean => props.view !== 'player' || props.pilot;
  const dm = (): boolean => props.view !== 'player';

  // ---------- painting ----------
  function say(kind: 'ok' | 'error' | 'none', text = ''): void {
    const box = part('battlemap-status');
    if (!box) return;
    box.hidden = kind === 'none';
    box.className = `bm-status${kind === 'none' ? '' : ` is-${kind}`}`;
    box.textContent = text;
  }

  function applyFrame(): void {
    const stage = part('battlemap-stage');
    if (stage) stage.style.transform = `translate(${frame.panX}px,${frame.panY}px) scale(${frame.zoom})`;
  }

  function fit(): boolean {
    const view = part('battlemap-view');
    if (!view || !map || !view.clientWidth || !view.clientHeight) return false;
    const zoom = Math.min(view.clientWidth / map.width, view.clientHeight / map.height);
    frame = { zoom, panX: (view.clientWidth - map.width * zoom) / 2, panY: (view.clientHeight - map.height * zoom) / 2 };
    fitted = true;
    applyFrame();
    return true;
  }

  function zoomAt(factor: number, cx: number, cy: number): void {
    const zoom = Math.max(0.3, Math.min(4, frame.zoom * factor));
    const k = zoom / frame.zoom;
    frame = { zoom, panX: cx - (cx - frame.panX) * k, panY: cy - (cy - frame.panY) * k };
    fitted = true;
    applyFrame();
  }

  /** A pointer's place on the map: in the view, and in the image. */
  function world(e: { clientX: number; clientY: number }): { lx: number; ly: number; x: number; y: number } {
    const box = part('battlemap-view')?.getBoundingClientRect();
    const lx = e.clientX - (box?.left ?? 0);
    const ly = e.clientY - (box?.top ?? 0);
    return { lx, ly, x: (lx - frame.panX) / frame.zoom, y: (ly - frame.panY) / frame.zoom };
  }

  function paintToolbar(): void {
    const box = part('battlemap-toolbar');
    if (box && map && instance) box.innerHTML = toolbar(map, instance, props);
  }

  function paintTokens(): void {
    const layer = part('battlemap-tokens');
    if (!layer || !map || !instance) return;
    if (drag?.moved) {
      stale = true;
      return;
    }
    stale = false;
    const grid = map.grid;
    layer.style.setProperty('--bm-scale', ((grid.size * 1.7) / 64).toFixed(3));
    layer.innerHTML = shown(instance.tokens, props.view)
      .map((t) => tokenItem(t, grid, { view: props.view, selected: t.id === selected, character: characters[t.char] }))
      .join('');
  }

  function paintCarousel(): void {
    const box = part('initiative-carousel');
    if (!box) return;
    const combat = instance?.combat ?? null;
    box.hidden = combat === null || !instance;
    if (box.hidden || !combat || !instance) {
      box.innerHTML = '';
      return;
    }
    box.innerHTML = carousel(combat, instance.tokens, { view: props.view, selected, characters, pjInitiative: instance.pjInitiative });
  }

  function paintInspector(): void {
    const box = part('token-inspector');
    if (!box) return;
    const t = dm() ? instance?.tokens.find((x) => x.id === selected) : undefined;
    if (!t || !instance) {
      box.hidden = true;
      box.innerHTML = '';
      return;
    }
    // A refresh does not take the field from under the DM's keys.
    const typing = host.ownerDocument.activeElement?.id;
    const same = !box.hidden && box.dataset.token === t.id;
    if (same && (typing === 'inspector-name' || typing === 'inspector-hp-amount')) return;
    // The amount typed stays for the next change of this token — never for another one's sheet (hp-change-one-token).
    const amount = same ? box.querySelector<HTMLInputElement>('#inspector-hp-amount')?.value : undefined;
    box.hidden = false;
    box.dataset.token = t.id;
    const rank = instance.combat?.order.find((o) => o.id === t.id);
    box.innerHTML = inspector(t, instance.tokens.filter((x) => x.char === t.char), characters[t.char], rank?.init ?? null);
    const field = box.querySelector<HTMLInputElement>('#inspector-hp-amount');
    if (field && amount !== undefined) field.value = amount;
  }

  function paintRoster(): void {
    const box = part('battlemap-roster');
    if (!box) return;
    box.hidden = props.view !== 'prep' || !instance;
    if (box.hidden || !instance) return;
    box.innerHTML = roster(instance.roster.map((id) => characters[id]).filter((c): c is Character => c !== undefined));
  }

  function paintGrid(): void {
    if (!map) return;
    part('battlemap-grid')?.setAttribute('d', gridPath(map.grid, map.width, map.height));
    const path = part('battlemap-grid');
    if (path) path.style.opacity = String(map.grid.opacity);
  }

  /** The map itself: only when the battlemap under the instance changes. */
  function paintMap(): void {
    const box = part('battlemap-map');
    if (!box || !map) return;
    box.innerHTML = hexMap(map, canMove());
    paintGrid();
    fitted = false;
    frame = { zoom: 1, panX: 0, panY: 0 };
    applyFrame();
    observer?.disconnect();
    observer = null;
    if (!fit() && typeof ResizeObserver !== 'undefined') {
      const view = part('battlemap-view');
      if (view) {
        observer = new ResizeObserver(() => {
          if (!fitted) fit();
        });
        observer.observe(view);
      }
    }
  }

  function paintAll(): void {
    paintToolbar();
    paintTokens();
    paintCarousel();
    paintInspector();
    paintRoster();
  }

  // ---------- reading ----------
  async function loadMap(id: string): Promise<MapInfo | null> {
    return parseMap((await shell.capability(GET_BATTLEMAP, undefined, { id })).data);
  }

  async function loadCharacters(): Promise<Record<string, Character>> {
    if (!dm()) return {};
    try {
      return parseCharacters((await shell.capability(LIST_CHARACTERS, undefined, props.campaign === '' ? {} : { campaign: props.campaign })).data);
    } catch {
      // Without the list a character cannot be dropped, but the map and its tokens stay.
      return {};
    }
  }

  /** Reads the instance again; answers whether it could. An older answer never replaces a newer one. */
  async function refresh(): Promise<boolean> {
    const mine = generation;
    const no = (readNo += 1);
    try {
      const read = await shell.capability(GET_BATTLEMAP_INSTANCE, undefined, { instance: props.instance, view: dm() ? 'dm' : 'player' });
      if (mine !== generation || no < readNo) return true;
      const next = parseInstance(read.data);
      if (next === null) {
        say('error', 'Cette battlemap est introuvable.');
        return false;
      }
      next.id ||= props.instance;
      // The map under the instance is read before anything changes: both are taken, or neither.
      const changed = next.battlemap !== instance?.battlemap || map === null;
      const loaded = changed ? await loadMap(next.battlemap) : map;
      if (mine !== generation || no < readNo) return true;
      instance = next;
      if (loaded === null) say('error', 'La carte de cette instance est introuvable.');
      if (loaded !== map) {
        map = loaded;
        paintMap();
      }
      if (selected !== null && !next.tokens.some((t) => t.id === selected)) selected = null;
      paintAll();
      return true;
    } catch {
      say('error', 'Impossible de lire la battlemap.');
      return false;
    }
  }

  async function start(): Promise<void> {
    generation += 1;
    const mine = generation;
    stopWatching();
    instance = null;
    map = null;
    selected = null;
    drag = null;
    pan = null;
    stale = false;
    observer?.disconnect();
    observer = null;
    host.innerHTML = skeleton(props);
    if (props.instance === '') return;
    const listed = await loadCharacters();
    if (mine !== generation) return;
    characters = listed;
    await refresh();
    if (mine !== generation) return;
    let first = true;
    stopWatching = shell.onDataVersion(() => {
      // The first answer only says where the data stands: it was just read.
      if (first) first = false;
      else void refresh();
    }, timing.watchMs);
  }

  // ---------- commands ----------
  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(body: unknown): Promise<Settled> {
    const answer = await shell.command(body);
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  /** One gesture: shown at once, sent, and the map read again — what the server refused goes back where it was. */
  async function act(failure: string, dataCapability: string, payload: Record<string, unknown>, apply?: () => void): Promise<void> {
    if (!instance) return;
    // What is on screen is the newest: a read still on its way is older than this gesture.
    readNo += 1;
    apply?.();
    paintAll();
    try {
      const state = await send(tableCommand(dataCapability, instance.id, payload));
      if (state.status === 'rejected') say('error', `${failure} refusé : ${reasons(state.violations)}.`);
      else if (state.status !== 'applied') say('error', `${failure} n’a pas abouti (état : ${state.status || 'inconnu'}).`);
      else say('none');
    } catch {
      say('error', `${failure} a échoué.`);
    }
    await refresh();
  }

  const moveTo = (t: Token, hex: Hex): Promise<void> =>
    act('Le déplacement', MOVE_TOKEN, { token: t.id, q: hex.q, r: hex.r }, () => {
      t.q = hex.q;
      t.r = hex.r;
    });

  /** hp-change-one-token: the change goes to the token whose sheet is open, and to it alone. */
  const changeHitPoints = (t: Token, delta: number): Promise<void> =>
    act('Le changement de PV', CHANGE_HIT_POINTS, { token: t.id, delta }, () => {
      if (t.kind !== 'pnj' || t.hp === null || t.hpMax === null) return;
      t.hp = hitPointsAfter(t.hp, t.hpMax, delta);
      t.dead = t.hp === 0;
    });

  /** next-turn: the next living participant by rank. */
  const nextTurn = (): Promise<void> => {
    const current = instance;
    if (!current?.combat) return Promise.resolve();
    const combat = current.combat;
    return act('Le tour suivant', NEXT_TURN, {}, () => Object.assign(combat, turnAfter(combat, current.tokens)));
  };

  /** The participant moves to the rank `position`; the others keep their relative order. */
  const reorder = (id: string, position: number): Promise<void> => {
    const combat = instance?.combat;
    if (!combat || position < 0 || position >= combat.order.length || combat.order[position]?.id === id) return Promise.resolve();
    return act('Le réordonnancement', REORDER_INITIATIVE, { token: id, position }, () => {
      combat.order = reordered(combat.order, id, position);
    });
  };

  const removeParticipant = (id: string): Promise<void> => {
    const current = instance;
    const combat = current?.combat;
    if (!current || !combat) return Promise.resolve();
    return act('Le retrait du combat', REMOVE_PARTICIPANT, { token: id }, () => {
      current.combat = without(combat, current.tokens, id);
    });
  };

  /** A newcomer: an NPC rolls its d20, a PC takes the initiative the DM entered. */
  function addParticipant(id: string, entered: number): Promise<void> {
    const combat = instance?.combat;
    const added = instance?.tokens.find((t) => t.id === id);
    if (!combat || !added) return Promise.resolve();
    const dexMod = characters[added.char]?.dexMod ?? 0;
    const roll = (timing.roll ?? (() => 10))();
    const init = added.kind === 'pnj' ? initiativeOf(roll, dexMod) : entered;
    return act('L’ajout au combat', ADD_PARTICIPANT, added.kind === 'pnj' ? { token: id, roll } : { token: id, initiative: entered }, () => {
      combat.order = inserted(combat.order, { id, init, dex: dexMod });
    });
  }

  const endCombat = (): Promise<void> =>
    act('La fin du combat', END_COMBAT, {}, () => {
      if (instance) instance.combat = null;
    });

  // ---------- dialogs ----------
  let starters: Starter[] = [];

  const closeDialog = (): void => {
    const box = part('battlemap-dialog');
    if (box) box.innerHTML = '';
    starters = [];
  };

  /** « Lancer le combat » : the NPC roll their d20 where the DM can see it, and he enters the PCs' initiative. */
  function askStart(): void {
    const current = instance;
    const box = part('battlemap-dialog');
    if (!current || !box || current.combat) return;
    const roll = timing.roll ?? (() => 10);
    starters = current.tokens
      .filter((t) => !t.dead)
      .map((token) => ({ token, roll: token.kind === 'pnj' ? roll() : 0, dexMod: characters[token.char]?.dexMod ?? 0, entered: token.kind === 'pj' ? (current.pjInitiative[token.char] ?? null) : null }));
    box.innerHTML = startDialog(starters);
    box.querySelector<HTMLInputElement>('[data-init]')?.focus();
  }

  /** The combat starts as the dialog shows it: NPC rolls by token, PC initiative by character. */
  function startCombat(): Promise<void> {
    const box = part('battlemap-dialog');
    const rolled = starters;
    if (!box || rolled.length === 0) return Promise.resolve();
    // A PC's initiative is the DM's to enter: the dialog stays open on the one that is missing.
    const blank = Array.from(box.querySelectorAll<HTMLInputElement>('[data-init]')).find((f) => !(Math.floor(Number(f.value)) >= 1));
    if (blank) {
      const error = part('combat-start-error');
      if (error) {
        error.hidden = false;
        error.textContent = `Saisissez l’initiative de ${rolled.find((s) => s.token.id === blank.dataset.init)?.token.name ?? 'ce PJ'} (1 ou plus).`;
      }
      blank.focus();
      return Promise.resolve();
    }
    const fields = new Map(Array.from(box.querySelectorAll<HTMLInputElement>('[data-init]')).map((f) => [f.dataset.init ?? '', Math.floor(Number(f.value))]));
    const initiative: Record<string, number> = {};
    const rolls: Record<string, number> = {};
    const lines = rolled.map((s, rank) => {
      const init = s.token.kind === 'pnj' ? initiativeOf(s.roll, s.dexMod) : (fields.get(s.token.id) ?? 1);
      if (s.token.kind === 'pnj') rolls[s.token.id] = s.roll;
      else initiative[s.token.char] = init;
      return { id: s.token.id, init, dex: s.dexMod, rank };
    });
    closeDialog();
    // The ties the table breaks at random are broken by the order the dialog listed.
    const order = lines.sort((a, b) => b.init - a.init || b.dex - a.dex || a.rank - b.rank).map(({ id, init, dex }) => ({ id, init, dex }));
    return act('Le lancement du combat', START_COMBAT, { initiative, rolls }, () => {
      if (instance && order[0]) instance.combat = { round: 1, current: order[0].id, order };
    });
  }

  function askReset(): void {
    const box = part('battlemap-dialog');
    if (box) box.innerHTML = resetDialog();
  }

  /** state-persisted-reset: the table empties the instance — tokens, combat, initiative — and the map is read again. */
  function resetMap(): Promise<void> {
    closeDialog();
    selected = null;
    return act('La réinitialisation', RESET_BATTLEMAP, {});
  }

  /** Dropping a character: a PC with a token on the map is moved, anything else placed `count` times. */
  async function place(character: Character, hex: Hex, count: number): Promise<void> {
    if (!instance) return;
    const there = existingToken(instance.tokens, character);
    if (there) return moveTo(there, hex);
    return act('Le placement', PLACE_TOKENS, { character: character.id, q: hex.q, r: hex.r, count: character.kind === 'pnj' ? count : 1 });
  }

  /** « Placer Bandit » : the dialog of a drop, where the DM says how many. */
  function askPlace(character: Character, hex: Hex, at: { lx: number; ly: number }): void {
    const view = part('battlemap-view');
    if (!view || !instance) return;
    part('battlemap-drop')?.remove();
    const pop = host.ownerDocument.createElement('div');
    pop.className = 'bm-drop';
    pop.id = 'battlemap-drop';
    pop.dataset.noPan = '';
    pop.dataset.char = character.id;
    pop.style.left = `${at.lx}px`;
    pop.style.top = `${at.ly}px`;
    pop.innerHTML = dropForm(character, instance.tokens, 1);
    view.append(pop);
    pop.addEventListener('input', (event) => {
      const field = event.target as { id?: string; value?: string } | null;
      const note = pop.querySelector('#battlemap-drop-numbers');
      if (field?.id === 'battlemap-drop-count' && note && instance) note.textContent = numbering(instance.tokens, character, countOf(field.value ?? ''));
    });
    pop.addEventListener('click', (event) => {
      const clicked = (event.target as Element | null)?.closest('button');
      if (!clicked) return;
      const count = countOf(pop.querySelector<HTMLInputElement>('#battlemap-drop-count')?.value ?? '1');
      pop.remove();
      if (clicked.id === 'battlemap-drop-ok') void place(character, hex, count);
    });
    pop.querySelector<HTMLInputElement>('#battlemap-drop-count')?.focus();
  }

  const showOnPlayerView = async (): Promise<void> => {
    if (!instance) return;
    try {
      const body = commandBody(SHOW_ON_PLAYER_VIEW, 'Diffusion', undefined, { kind: 'battlemap', instance: instance.id, zoom: Number(frame.zoom.toFixed(3)), panX: Math.round(frame.panX), panY: Math.round(frame.panY) });
      const state = await send(body);
      if (state.status === 'rejected') say('error', `L’envoi aux joueurs est refusé : ${reasons(state.violations)}.`);
      else if (state.status !== 'applied') say('error', `L’envoi aux joueurs n’a pas abouti (état : ${state.status || 'inconnu'}).`);
      else say('ok', 'Carte envoyée aux joueurs.');
    } catch {
      say('error', 'L’envoi aux joueurs a échoué.');
    }
  };

  // ---------- gestures ----------
  const tokenOf = (el: Element | null): Token | undefined => {
    const id = el?.closest<HTMLElement>('.kit-token__item')?.dataset.token;
    return instance?.tokens.find((t) => t.id === id);
  };

  function drawPath(from: Hex, to: Hex): void {
    const over = part('battlemap-over');
    if (over && map) over.innerHTML = pathMarkup(map.grid, line(from, to));
  }

  function clearCarry(): void {
    const over = part('battlemap-over');
    if (over) over.innerHTML = '';
    const tip = part('battlemap-tip');
    if (tip) tip.hidden = true;
  }

  host.addEventListener('pointerdown', (event) => {
    const e = event;
    if (e.button !== 0 || !canMove() || !map) return;
    const target = e.target as Element;
    const el = target.closest<HTMLElement>('.kit-token__item');
    if (el && part('battlemap-tokens')?.contains(el)) {
      const t = tokenOf(el);
      if (!t) return;
      e.preventDefault();
      drag = { token: t, el, from: { q: t.q, r: t.r }, x: e.clientX, y: e.clientY, moved: false, to: null };
      el.setPointerCapture?.(e.pointerId);
    } else if (target.closest('.hm-view') && !target.closest('[data-no-pan]')) {
      pan = { x: e.clientX, y: e.clientY, panX: frame.panX, panY: frame.panY, moved: false };
      part('battlemap-view')?.setPointerCapture?.(e.pointerId);
    }
  });

  host.addEventListener('pointermove', (event) => {
    const e = event;
    if (drag && map) {
      if (!drag.moved && Math.abs(e.clientX - drag.x) + Math.abs(e.clientY - drag.y) < DRAG_THRESHOLD) return;
      drag.moved = true;
      drag.el.classList.add('kit-token__item--carried');
      const w = world(e);
      drag.el.style.left = `${w.x}px`;
      drag.el.style.top = `${w.y}px`;
      drag.to = toHex(map.grid, w);
      drawPath(drag.from, drag.to);
      const tip = part('battlemap-tip');
      if (tip) {
        tip.textContent = label(distance(drag.from, drag.to));
        tip.style.left = `${w.lx + 16}px`;
        tip.style.top = `${w.ly + 16}px`;
        tip.hidden = false;
      }
    } else if (pan) {
      const dx = e.clientX - pan.x;
      const dy = e.clientY - pan.y;
      if (!pan.moved && Math.abs(dx) + Math.abs(dy) > DRAG_THRESHOLD) pan.moved = true;
      if (pan.moved) {
        frame = { ...frame, panX: pan.panX + dx, panY: pan.panY + dy };
        fitted = true;
        applyFrame();
      }
    }
  });

  const release = (): void => {
    pan = null;
    if (!drag) return;
    const d = drag;
    drag = null;
    clearCarry();
    if (d.moved && d.to && map && !onMap(map.grid, d.to, map.width, map.height)) {
      say('error', 'Ce hex est hors de la carte.');
      paintTokens();
    } else if (d.moved && d.to && (d.to.q !== d.from.q || d.to.r !== d.from.r)) void moveTo(d.token, d.to);
    else if (!d.moved && dm()) {
      selected = d.token.id;
      paintAll();
    } else paintTokens();
    if (stale) paintTokens();
  };
  host.addEventListener('pointerup', release);
  host.addEventListener('pointercancel', release);

  host.addEventListener('wheel', (event) => {
    const e = event;
    if (!canMove() || !(e.target as Element).closest('.hm-view')) return;
    e.preventDefault();
    const box = part('battlemap-view')?.getBoundingClientRect();
    zoomAt(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientX - (box?.left ?? 0), e.clientY - (box?.top ?? 0));
  });

  host.addEventListener('dragstart', (event) => {
    const e = event;
    const carried = (e.target as Element).closest<HTMLElement>('[data-participant]');
    if (carried && e.dataTransfer) {
      e.dataTransfer.setData(PARTICIPANT_TYPE, carried.dataset.participant ?? '');
      e.dataTransfer.effectAllowed = 'move';
      return;
    }
    const chip = (e.target as Element).closest<HTMLElement>('[data-char]');
    if (!chip || !e.dataTransfer) return;
    e.dataTransfer.setData(CHARACTER_TYPE, chip.dataset.char ?? '');
    e.dataTransfer.effectAllowed = 'copy';
  });

  host.addEventListener('dragover', (event) => {
    const e = event;
    if (dm() && (e.target as Element).closest('[data-participant]') && Array.from(e.dataTransfer?.types ?? []).includes(PARTICIPANT_TYPE)) {
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
      const over = (e.target as Element).closest('[data-participant]');
      for (const item of host.querySelectorAll('.bm-drag-over')) if (item !== over) item.classList.remove('bm-drag-over');
      over?.classList.add('bm-drag-over');
      return;
    }
    if (dm() && (e.target as Element).closest('.hm-view') && Array.from(e.dataTransfer?.types ?? []).includes(CHARACTER_TYPE)) {
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    }
  });

  host.addEventListener('dragend', () => {
    for (const item of host.querySelectorAll('.bm-drag-over')) item.classList.remove('bm-drag-over');
  });

  host.addEventListener('drop', (event) => {
    const e = event;
    for (const item of host.querySelectorAll('.bm-drag-over')) item.classList.remove('bm-drag-over');
    // A participant dropped on another takes its rank.
    const carried = e.dataTransfer?.getData(PARTICIPANT_TYPE) ?? '';
    const over = (e.target as Element).closest<HTMLElement>('[data-participant]');
    if (dm() && carried !== '' && over) {
      e.preventDefault();
      const position = instance?.combat?.order.findIndex((o) => o.id === over.dataset.participant) ?? -1;
      if (position >= 0) void reorder(carried, position);
      return;
    }
    const character = characters[e.dataTransfer?.getData(CHARACTER_TYPE) ?? ''];
    if (!dm() || !character || !map || !(e.target as Element).closest('.hm-view')) return;
    e.preventDefault();
    const w = world(e);
    const hex = toHex(map.grid, w);
    if (onMap(map.grid, hex, map.width, map.height)) askPlace(character, hex, w);
    else say('error', 'Ce hex est hors de la carte.');
  });

  host.addEventListener('click', (event) => {
    const target = event.target as Element | null;
    // A participant of the carousel opens the sheet of that very instance.
    const listed = target?.closest<HTMLElement>('#initiative-carousel .kit-token__item');
    if (listed && dm()) {
      selected = listed.dataset.token ?? null;
      paintAll();
      return;
    }
    const button = target?.closest('button');
    if (!button) return;
    // The combat, the reset and the sheet are the DM's: a player view mounts none of them, and a stray click on one does nothing.
    if (!dm() && DM_ONLY.has(button.id)) return;
    const t = instance?.tokens.find((x) => x.id === selected);
    const zoomBox = part('battlemap-view');
    const [cx, cy] = [(zoomBox?.clientWidth ?? 0) / 2, (zoomBox?.clientHeight ?? 0) / 2];
    const gesture = button.dataset.act;
    if (gesture === 'in') zoomAt(1.25, cx, cy);
    else if (gesture === 'out') zoomAt(1 / 1.25, cx, cy);
    else if (gesture === 'reset') fit();
    else if (gesture === 'dialog-close') closeDialog();
    else if (gesture === 'close') {
      selected = null;
      paintAll();
    } else if (button.dataset.select) {
      selected = button.dataset.select;
      paintAll();
    } else if (button.dataset.char && button.closest('#battlemap-roster')) {
      const character = characters[button.dataset.char];
      if (character && map) {
        const box = part('battlemap-view');
        const w = world({ clientX: (box?.getBoundingClientRect().left ?? 0) + (box?.clientWidth ?? 0) / 2, clientY: (box?.getBoundingClientRect().top ?? 0) + (box?.clientHeight ?? 0) / 2 });
        askPlace(character, toHex(map.grid, w), w);
      }
    } else if (button.id === 'bm-start-combat') askStart();
    else if (button.id === 'combat-start-submit') void startCombat();
    else if (button.id === 'combat-start-cancel' || button.id === 'battlemap-reset-cancel') closeDialog();
    else if (button.id === 'bm-reset') askReset();
    else if (button.id === 'battlemap-reset-submit') void resetMap();
    else if (button.id === 'bm-end-combat') void endCombat();
    else if (button.id === 'combat-next') void nextTurn();
    else if ((button.id === 'combat-earlier' || button.id === 'combat-later') && selected !== null) {
      const at = instance?.combat?.order.findIndex((o) => o.id === selected) ?? -1;
      if (at >= 0 && selected !== null) void reorder(selected, at + (button.id === 'combat-earlier' ? -1 : 1));
    } else if (button.id === 'combat-drop' && selected !== null) void removeParticipant(selected);
    else if (button.id === 'combat-add') {
      const id = part<HTMLSelectElement>('combat-add-select')?.value ?? '';
      const entered = Math.max(1, Math.floor(Number(part<HTMLInputElement>('combat-add-init')?.value)) || 10);
      if (id !== '') void addParticipant(id, entered);
    } else if (t && t.kind === 'pnj' && (button.id === 'inspector-damage' || button.id === 'inspector-heal')) {
      const amount = Math.max(0, Math.floor(Number(part<HTMLInputElement>('inspector-hp-amount')?.value)) || 0);
      if (amount > 0) void changeHitPoints(t, button.id === 'inspector-damage' ? -amount : amount);
    } else if (button.dataset.writes === SHOW_ON_PLAYER_VIEW) void showOnPlayerView();
    else if (t && button.dataset.state) {
      const state = button.dataset.state;
      void act('Le changement d’état', SET_TOKEN_CONDITION, { token: t.id, conditions: withCondition(t.states, state) }, () => {
        t.states = withCondition(t.states, state);
      });
    } else if (t && button.id === 'inspector-visible') {
      void act(t.visible ? 'Le fait de cacher' : 'Le fait de révéler', SET_TOKEN_VISIBILITY, { token: t.id, visible: !t.visible }, () => {
        t.visible = !t.visible;
      });
    } else if (t && button.dataset.writes === REMOVE_TOKEN) {
      void act('Le retrait', REMOVE_TOKEN, { token: t.id }, () => {
        if (instance) instance.tokens = instance.tokens.filter((x) => x.id !== t.id);
        selected = null;
      });
    }
  });

  host.addEventListener('change', (event) => {
    const field = event.target as HTMLInputElement | null;
    const t = instance?.tokens.find((x) => x.id === selected);
    if (!t || field?.id !== 'inspector-name') return;
    const name = field.value.trim();
    if (name === '') {
      field.value = t.name;
      say('error', 'Le nom d’un token ne peut pas être vide.');
    } else if (name !== t.name) {
      void act('Le renommage', RENAME_TOKEN, { token: t.id, name }, () => {
        t.name = name;
      });
    }
  });

  host.addEventListener('submit', (event) => event.preventDefault());
  host.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeDialog();
  });

  void start();
  return {
    update(next) {
      const parsed = parseProps(next);
      const same = parsed.instance === props.instance && parsed.view === props.view && parsed.pilot === props.pilot && parsed.fill === props.fill && parsed.campaign === props.campaign;
      props = parsed;
      if (!same) void start();
    },
  };
}
