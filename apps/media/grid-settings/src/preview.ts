// The preview: the kit's HexMap (mock/kit/HexMap.html) over the battlemap —
// its map, the grid redrawn as the settings change, the pan, the zoom and the
// measure of a distance between two hexes, a hex always 5 feet. The markup and
// the classes are the kit's; the geometry is HexGrid@1's (hex.ts).

import { cssColor } from './model.ts';
import { esc } from './render.ts';
import { center, distance, feet, gridPath, line, points, problems, toHex } from './hex.ts';
import type { Battlemap, Grid, Hex } from './types.ts';

const NS = 'http://www.w3.org/2000/svg';

export interface Preview {
  /** Redraws the grid; a grid HexGrid@1 does not hold is not drawn, the last valid one stays. */
  setGrid(grid: Grid): void;
}

function markup(map: Battlemap): string {
  const view = `viewBox="0 0 ${map.width} ${map.height}" width="${map.width}" height="${map.height}"`;
  const shape = (s: Battlemap['terrain'][number]): string =>
    `<${s.tag}${Object.entries(s.attrs)
      .map(([name, value]) => ` ${name}="${esc(value)}"`)
      .join('')}></${s.tag}>`;
  // The map holds no land of its own when the Capability answers no terrain: the base is always there.
  const land = map.terrain.some((s) => s.attrs.class === 'hm-land') ? '' : `<rect class="hm-land" x="0" y="0" width="${map.width}" height="${map.height}"></rect>`;
  return (
    '<div class="kit-hexmap">' +
    '<div class="hm-bar"><span class="hm-zoom"><button type="button" data-act="out" aria-label="Dézoomer">−</button>' +
    '<button type="button" data-act="reset">Ajuster</button><button type="button" data-act="in" aria-label="Zoomer">+</button></span></div>' +
    '<div class="hm-view" tabindex="0" id="grid-preview-map" aria-label="Aperçu de la battlemap avec sa grille">' +
    '<div class="hm-stage">' +
    `<svg class="hm-layer hm-map" ${view} aria-hidden="true">${land}${map.terrain.map(shape).join('')}</svg>` +
    `<svg class="hm-layer hm-grid" ${view} aria-hidden="true"><path class="hm-grid-path" d=""></path></svg>` +
    `<svg class="hm-layer hm-over" ${view}></svg>` +
    '</div><div class="hm-tip" hidden></div></div>' +
    '<p class="hm-hint">Glisser pour déplacer · molette pour zoomer · cliquer deux hex pour mesurer</p></div>'
  );
}

const MIN_ZOOM = 0.3;
const MAX_ZOOM = 4;

/** Fills `host` with the preview of `map` under `grid` and wires its pan, zoom and measure. */
export function mountPreview(host: HTMLElement, map: Battlemap, grid: Grid): Preview {
  host.innerHTML = markup(map);
  const part = <T extends Element>(selector: string): T => {
    const found = host.querySelector<T>(selector);
    if (!found) throw new Error(`GridSettings: ${selector} is not in the preview`);
    return found;
  };
  const root = part<HTMLElement>('.kit-hexmap');
  const view = part<HTMLElement>('.hm-view');
  const stage = part<HTMLElement>('.hm-stage');
  const path = part<SVGPathElement>('.hm-grid-path');
  const over = part<SVGElement>('.hm-over');
  const tip = part<HTMLElement>('.hm-tip');
  stage.style.width = `${map.width}px`;
  stage.style.height = `${map.height}px`;

  let current = grid;
  let zoom = 1;
  let px = 0;
  let py = 0;
  let fitted = false;

  const applyView = (): void => {
    stage.style.transform = `translate(${px}px,${py}px) scale(${zoom})`;
  };
  const zoomAt = (factor: number, cx: number, cy: number): void => {
    const next = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, zoom * factor));
    const k = next / zoom;
    px = cx - (cx - px) * k;
    py = cy - (cy - py) * k;
    zoom = next;
    applyView();
  };
  /** The whole map in the view, centred. */
  const fit = (): boolean => {
    const w = view.clientWidth;
    const h = view.clientHeight;
    if (!w || !h) return false;
    zoom = Math.min(w / map.width, h / map.height);
    px = (w - map.width * zoom) / 2;
    py = (h - map.height * zoom) / 2;
    fitted = true;
    applyView();
    return true;
  };

  const svg = (name: string, attrs: Record<string, string>, cls: string): SVGElement => {
    const element = view.ownerDocument.createElementNS(NS, name);
    element.setAttribute('class', cls);
    for (const [key, value] of Object.entries(attrs)) element.setAttribute(key, value);
    return element;
  };
  const poly = (hex: Hex, cls: string): SVGElement => svg('polygon', { points: points(current, hex) }, cls);
  const clearOver = (): void => over.replaceChildren();
  const showTip = (words: string, lx: number, ly: number): void => {
    tip.textContent = words;
    tip.style.left = `${lx + 16}px`;
    tip.style.top = `${ly + 16}px`;
    tip.hidden = false;
  };
  const world = (e: MouseEvent): { lx: number; ly: number; x: number; y: number } => {
    const box = view.getBoundingClientRect();
    const lx = e.clientX - box.left;
    const ly = e.clientY - box.top;
    return { lx, ly, x: (lx - px) / zoom, y: (ly - py) / zoom };
  };

  /** The path from `a` to `b`, drawn; answers its length in hexes. */
  const drawPath = (a: Hex, b: Hex): number => {
    clearOver();
    const cells = line(a, b);
    cells.forEach((h, i) => over.appendChild(poly(h, i === cells.length - 1 ? 'hm-end' : 'hm-step')));
    over.appendChild(
      svg(
        'polyline',
        {
          points: cells
            .map((h) => {
              const c = center(current, h);
              return `${c.x.toFixed(1)},${c.y.toFixed(1)}`;
            })
            .join(' '),
        },
        'hm-line',
      ),
    );
    return distance(a, b);
  };
  /** « 3 hex · 15 pi »: a hex is 5 feet, whatever the grid. */
  const label = (hexes: number): string => `${hexes} hex · ${feet(hexes)} pi`;

  let start: Hex | null = null;
  let end: Hex | null = null;
  const forget = (): void => {
    start = null;
    end = null;
    clearOver();
    tip.hidden = true;
  };

  let down: { x: number; y: number; px: number; py: number; moved: boolean } | null = null;
  view.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    down = { x: e.clientX, y: e.clientY, px, py, moved: false };
    view.setPointerCapture?.(e.pointerId);
  });
  view.addEventListener('pointermove', (e) => {
    if (down) {
      const dx = e.clientX - down.x;
      const dy = e.clientY - down.y;
      if (!down.moved && Math.abs(dx) + Math.abs(dy) > 4) {
        down.moved = true;
        view.classList.add('hm-drag');
      }
      if (down.moved) {
        px = down.px + dx;
        py = down.py + dy;
        applyView();
      }
      return;
    }
    if (!start || end) return;
    const w = world(e);
    showTip(label(drawPath(start, toHex(current, w.x, w.y))), w.lx, w.ly);
  });
  view.addEventListener('pointerup', (e) => {
    const click = down !== null && !down.moved;
    down = null;
    view.classList.remove('hm-drag');
    if (!click) return;
    const w = world(e);
    const hex = toHex(current, w.x, w.y);
    if (!start || end) {
      start = hex;
      end = null;
      clearOver();
      over.appendChild(poly(hex, 'hm-end'));
      tip.hidden = true;
    } else {
      end = hex;
      showTip(label(drawPath(start, end)), w.lx, w.ly);
    }
  });
  view.addEventListener(
    'wheel',
    (e) => {
      e.preventDefault();
      const box = view.getBoundingClientRect();
      zoomAt(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientX - box.left, e.clientY - box.top);
    },
    { passive: false },
  );
  root.querySelectorAll<HTMLElement>('[data-act]').forEach((button) =>
    button.addEventListener('click', () => {
      const act = button.dataset.act;
      if (act === 'in') zoomAt(1.25, view.clientWidth / 2, view.clientHeight / 2);
      else if (act === 'out') zoomAt(1 / 1.25, view.clientWidth / 2, view.clientHeight / 2);
      else fit();
    }),
  );
  if (typeof ResizeObserver !== 'undefined') {
    new ResizeObserver(() => {
      if (!fitted) fit();
    }).observe(view);
  }

  const draw = (next: Grid): void => {
    // Only a grid HexGrid@1 holds is drawn: while the DM types a value out of range, the last valid grid stays.
    if (problems(next).length > 0) return;
    current = next;
    root.style.setProperty('--hm-grid', cssColor(next.color));
    path.style.opacity = String(next.opacity);
    path.setAttribute('d', gridPath(next, map.width, map.height));
    forget();
  };
  draw(grid);
  applyView();
  return { setGrid: draw };
}
