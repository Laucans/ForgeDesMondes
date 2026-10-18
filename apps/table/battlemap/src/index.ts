// Battlemap (system table): the DM's battlemap on a hex grid, and the same map
// as the players may see it. The DM drags a token and reads « 3 hex · 15 pi »
// under the cursor, drops a character « × N », opens a token's menu (rename,
// hide or reveal, condition, remove) and sends the map to the players with the
// zoom and pan he framed. It reads `table.getBattlemapInstance` (and the
// battlemap and the characters), and triggers the table's commands and
// `live.showOnPlayerView`, by identifier, through the shell it is handed: no
// network call of its own. A token's (q, r) is the only position there is
// (`single-position-source`): a drop sends it, and the map is read again.

import { GET_BATTLEMAP, GET_BATTLEMAP_INSTANCE, LIST_CHARACTERS, MOVE_TOKEN, PLACE_TOKENS, REMOVE_TOKEN, RENAME_TOKEN, SET_TOKEN_CONDITION, SET_TOKEN_VISIBILITY, SHOW_ON_PLAYER_VIEW } from './ids.ts';
import { distance, gridPath, label, line, onMap, toHex } from './hexgrid.ts';
import {
  commandBody,
  commandId,
  countOf,
  existingToken,
  isFinal,
  parseCharacters,
  parseInstance,
  parseMap,
  parseProps,
  reasons,
  settled,
  shown,
  tableCommand,
  withCondition,
  numbering,
  type Settled,
} from './model.ts';
import { dropForm, hexMap, inspector, pathMarkup, roster, skeleton, toolbar, tokenItem } from './render.ts';
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
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 500 };

/** What the shell keeps of a mounted Micro-UI: new props follow the screen's context. */
export interface Mounted {
  update(raw: Record<string, unknown>): void;
}

const DRAG_THRESHOLD = 4;
const CHARACTER_TYPE = 'text/fdm-character';

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
    if (!box.hidden && host.ownerDocument.activeElement?.id === 'inspector-name' && box.dataset.token === t.id) return;
    box.hidden = false;
    box.dataset.token = t.id;
    box.innerHTML = inspector(t, instance.tokens.filter((x) => x.char === t.char), characters[t.char]);
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
    const chip = (e.target as Element).closest<HTMLElement>('[data-char]');
    if (!chip || !e.dataTransfer) return;
    e.dataTransfer.setData(CHARACTER_TYPE, chip.dataset.char ?? '');
    e.dataTransfer.effectAllowed = 'copy';
  });

  host.addEventListener('dragover', (event) => {
    const e = event;
    if (dm() && (e.target as Element).closest('.hm-view') && Array.from(e.dataTransfer?.types ?? []).includes(CHARACTER_TYPE)) {
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    }
  });

  host.addEventListener('drop', (event) => {
    const e = event;
    const character = characters[e.dataTransfer?.getData(CHARACTER_TYPE) ?? ''];
    if (!dm() || !character || !map || !(e.target as Element).closest('.hm-view')) return;
    e.preventDefault();
    const w = world(e);
    const hex = toHex(map.grid, w);
    if (onMap(map.grid, hex, map.width, map.height)) askPlace(character, hex, w);
    else say('error', 'Ce hex est hors de la carte.');
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    const t = instance?.tokens.find((x) => x.id === selected);
    const zoomBox = part('battlemap-view');
    const [cx, cy] = [(zoomBox?.clientWidth ?? 0) / 2, (zoomBox?.clientHeight ?? 0) / 2];
    const gesture = button.dataset.act;
    if (gesture === 'in') zoomAt(1.25, cx, cy);
    else if (gesture === 'out') zoomAt(1 / 1.25, cx, cy);
    else if (gesture === 'reset') fit();
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
