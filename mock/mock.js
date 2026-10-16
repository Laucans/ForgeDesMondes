/* ForgeDesMondes — runtime of the mock-up.
   - Fills every [data-kit] element with the design system's kit markup, built from its data-props
     (the kit's own fragments are static demos; ../kit/kit.css carries their classes).
   - Loads every [data-component] micro-frontend from components/<Name>.html and runs its scripts.
   - Gives fragments their fixtures, a store shared between tabs (DM view ↔ player view),
     a page bus and the lifecycle of a command (queued, confirmed, applied). */
(() => {
  'use strict';
  if (window.Mock) return;

  const BASE = new URL('./', document.currentScript.src).href;
  const ABILITIES = ['FOR', 'DEX', 'CON', 'INT', 'SAG', 'CHA'];
  const fixtures = {};

  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const attr = (n, v) => (v == null || v === false || v === '' ? '' : v === true ? ` ${n}` : ` ${n}="${esc(v)}"`);
  const clone = v => (v == null ? v : JSON.parse(JSON.stringify(v)));
  const modOf = s => Math.floor(((s == null ? 10 : s) - 10) / 2);
  const mod = s => { const m = modOf(s); return m < 0 ? '−' + Math.abs(m) : '+' + m; };
  const norm = s => String(s || '').normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().trim();
  const uid = p => (p || 'id') + '-' + Math.random().toString(36).slice(2, 8);
  const fire = (el, type, detail) => el.dispatchEvent(new CustomEvent(type, { bubbles: true, detail }));

  const ICON = {
    mic: '<svg class="kit-micbutton__icon kit-micbutton__icon--mic" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0"></path><path d="M12 18v3"></path></svg>',
    pause: '<svg class="kit-micbutton__icon kit-micbutton__icon--pause" viewBox="0 0 24 24" width="28" height="28" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"></rect><rect x="14" y="5" width="4" height="14" rx="1"></rect></svg>',
    eye: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-label="Caché"><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/><path d="M3 3l18 18"/></svg>',
    upload: '<svg class="kit-filedrop__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>'
  };

  // An illustration of the example data: a gradient drawn from the tokens, a glyph, a caption.
  function art(a, extra) {
    if (!a) return '';
    if (typeof a === 'string') a = { art: a };
    return `<div class="art art--${esc(a.art || 'ember')}${extra ? ' ' + extra : ''}" role="img"${attr('aria-label', a.title)}>` +
      (a.glyph ? `<span class="art__glyph">${esc(a.glyph)}</span>` : '') +
      (a.caption ? `<span class="art__caption">${esc(a.caption)}</span>` : '') + '</div>';
  }

  function btn(b, cls) {
    return `<button type="button"${attr('class', cls)}${attr('id', b.id)}${attr('data-href', b.href)}${attr('data-target', b.target)}` +
      `${attr('data-action', b.action)}${attr('data-writes', b['data-writes'])}${attr('title', b.title)}${b.disabled ? ' disabled' : ''}>${esc(b.label)}</button>`;
  }

  const DARK_ON = ['primary', 'accent', 'success', 'danger'];
  function tokenItem(t) {
    const cls = ['kit-token__item', t.current && 'kit-token__item--current', t.hidden && 'kit-token__item--hidden',
      t.dead && 'kit-token__item--dead', t.carried && 'kit-token__item--carried', t.cls].filter(Boolean).join(' ');
    const clip = t.art
      ? `<span class="kit-token__portrait art art--${esc(t.art)}"><span class="art__glyph">${esc(t.initials)}</span></span>`
      : `<span class="kit-token__initials${DARK_ON.includes(t.color) ? ' is-dark' : ''}">${esc(t.initials)}</span>`;
    const badges = (t.badges || []).slice(0, 3).map((b, i) =>
      `<span class="kit-token__badge kit-token__badge--${b.pos || ['tl', 'tr', 'br'][i]}" title="${esc(b.title)}">${esc(b.glyph)}</span>`).join('');
    let hp = '';
    if (t.hp) {
      const pct = t.hp.max ? Math.round(100 * Math.max(0, t.hp.cur) / t.hp.max) : 0;
      const fill = pct <= 25 ? ' kit-token__gauge-fill--low' : pct <= 50 ? ' kit-token__gauge-fill--mid' : '';
      hp = `<div class="kit-token__hp"><div class="kit-token__gauge" style="--kit-token-hp: ${pct}%"><div class="kit-token__gauge-fill${fill}"></div></div>` +
        `<span class="kit-token__hp-label">${t.hp.cur}/${t.hp.max} PV</span></div>`;
    } else if (t.hpLabel) {
      hp = `<div class="kit-token__hp"><span class="kit-token__hp-label">${esc(t.hpLabel)}</span></div>`;
    }
    return `<div class="${cls}"${attr('id', t.id)}${attr('data-token', t.key)}${attr('style', t.style)}${attr('title', t.title)}>` +
      `<div class="kit-token__disc"${t.color ? ` style="--kit-token-bg: var(--color-${esc(t.color)})"` : ''}><div class="kit-token__clip">${clip}</div>` +
      (t.hidden ? `<div class="kit-token__eye">${ICON.eye}</div>` : '') + (t.dead ? '<span class="kit-token__strike"></span>' : '') + badges +
      (t.num != null ? `<span class="kit-token__num">${esc(t.num)}</span>` : '') +
      `</div><div class="kit-token__name">${esc(t.name)}</div>${hp}${t.extra || ''}</div>`;
  }

  // ---------- kit renderers: markup of the kit's classes, built from data-props ----------
  const R = {
    Button: p => `<div class="kit-button kit-button--bare${p.small ? ' kit-button--small' : ''}">` +
      (p.items || [p]).map(b => btn(b, b.variant || 'primary')).join('') + '</div>',

    Input: p => `<div class="kit-input${p.wide ? ' is-wide' : ''}"><label${attr('class', p.error ? 'error' : null)}>${esc(p.label)}` +
      `<input${attr('type', p.type || 'text')}${attr('id', p.id)}${attr('name', p.name)}${attr('value', p.value)}${attr('placeholder', p.placeholder)}` +
      `${attr('min', p.min)}${attr('max', p.max)}${p.disabled ? ' disabled' : ''}>` +
      (p.error || p.hint ? `<small>${esc(p.error || p.hint)}</small>` : '') + '</label></div>',

    Select: p => {
      const opts = (p.options || []).map(o => (typeof o === 'string' ? { value: o, label: o } : o));
      const cur = opts.find(o => o.value === p.value) || opts[0] || { label: '' };
      return `<div class="kit-select${p.wide ? ' is-wide' : ''}"><span>${esc(p.label)}</span>` +
        `<button type="button"${attr('id', p.id)} aria-haspopup="listbox" aria-expanded="false"><b data-value>${esc(cur.label)}</b><i aria-hidden="true">▾</i></button>` +
        '<ul role="listbox" hidden>' + opts.map(o => `<li role="option" data-value="${esc(o.value)}" aria-selected="${o === cur}">${esc(o.label)}</li>`).join('') + '</ul></div>';
    },

    Checkbox: p => `<div class="kit-checkbox"><fieldset${attr('id', p.id)}>` + (p.legend ? `<legend>${esc(p.legend)}</legend>` : '') +
      (p.options || []).map(o => `<label><input type="checkbox"${attr('name', p.name)}${attr('value', o.value)}${o.checked ? ' checked' : ''}${o.disabled ? ' disabled' : ''}>` +
        `<span>${p.html ? o.label : esc(o.label)}</span></label>`).join('') + '</fieldset></div>',

    Radio: p => `<div class="kit-radio"><fieldset${attr('id', p.id)}>` + (p.legend ? `<legend>${esc(p.legend)}</legend>` : '') +
      (p.options || []).map(o => `<label><input type="radio" name="${esc(p.name || 'radio')}"${attr('value', o.value)}${o.checked ? ' checked' : ''}>` +
        `<span>${esc(o.label)}</span>${o.hint ? `<small>${esc(o.hint)}</small>` : ''}</label>`).join('') + '</fieldset></div>',

    Toggle: p => `<div class="kit-toggle${p.bare ? ' kit-toggle--bare' : ''}">` + (p.items || [p]).map((t, i) =>
      `<label><button type="button" role="switch"${attr('id', t.id)} data-index="${i}" aria-checked="${!!t.on}"${t.disabled ? ' disabled' : ''}><i></i></button><span>${esc(t.label)}</span></label>`).join('') + '</div>',

    Menu: p => `<div class="kit-menu"><a class="brand"${attr('href', p.brandHref || '#')}>${esc(p.brand)}</a>` +
      `<nav${attr('id', p.navId)}${attr('aria-label', p.label || 'Sections')}>` +
      (p.links || []).map(l => `<a${attr('href', l.href)}${attr('id', l.id)}${l.current ? ' aria-current="page"' : ''}>${esc(l.label)}</a>`).join('') + '</nav>' +
      (p.end && p.end.length ? '<div class="end">' + p.end.map(b => btn(b)).join('') + '</div>' : '') + '</div>',

    Tabs: p => `<div class="kit-tabs"><div role="tablist"${attr('aria-label', p.label)}>` + (p.tabs || []).map((t, i) =>
      `<button type="button" role="tab" id="${esc((p.id || 'tabs') + '-tab-' + i)}" aria-selected="${i === (p.selected || 0)}">${esc(t.label || t)}</button>`).join('') + '</div></div>',

    Card: p => `<article class="kit-card"${attr('id', p.id)}>` +
      (p.art ? `<div class="media">${art(p.art)}</div>` : p.media ? `<div class="media">${esc(p.media)}</div>` : '') +
      `<div class="body"><h3>${esc(p.title)}</h3>${p.text ? `<p>${esc(p.text)}</p>` : ''}${p.html || ''}</div>` +
      (p.actions && p.actions.length ? '<div class="actions">' + p.actions.map(a => btn(a, a.secondary ? 'secondary' : null)).join('') + '</div>' : '') + '</article>',

    Table: p => {
      const cols = (p.columns || []).map(c => (typeof c === 'string' ? { label: c } : c));
      return `<table class="kit-table"${attr('id', p.id)}><thead><tr>` + cols.map(c => `<th${attr('class', c.num ? 'num' : null)}>${esc(c.label)}</th>`).join('') + '</tr></thead><tbody>' +
        (p.rows || []).map(r => {
          const cells = Array.isArray(r) ? r : r.cells;
          return `<tr${attr('id', r.id)}>` + cells.map((c, i) => `<td${attr('class', cols[i] && cols[i].num ? 'num' : null)}>${p.html ? c : esc(c)}</td>`).join('') + '</tr>';
        }).join('') + '</tbody></table>';
    },

    Badge: p => '<div class="kit-badge">' + (p.items || [p]).map(b => `<span${attr('class', b.tone)}${attr('id', b.id)}>${esc(b.label)}</span>`).join('') + '</div>',

    Breadcrumb: p => {
      const items = p.items || [];
      return `<nav class="kit-breadcrumb"${attr('id', p.id)} aria-label="Fil d’Ariane"><ol>` + items.map((s, i) =>
        `<li>${i === items.length - 1 ? `<span aria-current="page">${esc(s.label)}</span>` : `<a href="${esc(s.href)}">${esc(s.label)}</a>`}</li>`).join('') + '</ol></nav>';
    },

    HexMap: p => {
      const m = p.map || { width: 800, height: 560, terrain: [] };
      const vb = `viewBox="0 0 ${m.width} ${m.height}" width="${m.width}" height="${m.height}"`;
      const shape = s => `<${s.tag}` + Object.keys(s).filter(k => k !== 'tag').map(k => attr(k, s[k])).join('') + `></${s.tag}>`;
      return `<div class="kit-hexmap">` +
        (p.zoom !== false ? '<div class="hm-bar"><span class="hm-zoom"><button type="button" data-act="out" aria-label="Dézoomer">−</button>' +
          '<button type="button" data-act="reset">Ajuster</button><button type="button" data-act="in" aria-label="Zoomer">+</button></span></div>' : '') +
        `<div class="hm-view" tabindex="0"${attr('id', p.id)}${attr('aria-label', p.label || 'Carte hexagonale')}>` +
        `<div class="hm-stage" style="width:${m.width}px;height:${m.height}px">` +
        `<svg class="hm-layer hm-map" ${vb} aria-hidden="true">${(m.terrain || []).map(shape).join('')}</svg>` +
        `<svg class="hm-layer hm-grid" ${vb} aria-hidden="true"><path class="hm-grid-path" d=""></path></svg>` +
        `<svg class="hm-layer hm-over" ${vb}></svg></div><div class="hm-tip" hidden></div></div>` +
        (p.hint ? `<p class="hm-hint">${esc(p.hint)}</p>` : '') + '</div>';
    },

    Token: p => `<div class="kit-token${p.row ? ' kit-token--row' : ''}">` + (p.items || []).map(tokenItem).join('') + '</div>',

    MicButton: p => `<div class="kit-micbutton${p.compact ? ' kit-micbutton--compact' : ''}" data-state="idle">` +
      `<button type="button" class="kit-micbutton__btn"${attr('id', p.id)} aria-pressed="false"${attr('aria-label', p.label)}><span class="kit-micbutton__ring"></span>${ICON.mic}${ICON.pause}</button>` +
      `<div class="kit-micbutton__text"><span class="kit-micbutton__label">${esc(p.label)}</span>` +
      '<span class="kit-micbutton__status" aria-live="polite"><span class="kit-micbutton__state"></span> <span class="kit-micbutton__timer">00:00</span></span></div></div>',

    DiffView: p => `<div class="kit-diffview"${attr('id', p.id)}><header class="kit-diffview__head"><h2 class="kit-diffview__title">${esc(p.title || 'Changements')}</h2>` +
      `<span class="kit-diffview__meta">${esc(p.meta)}</span></header><ul class="kit-diffview__list">` + (p.rows || []).map((r, i) =>
        `<li class="kit-diffview__row" data-index="${i}"><div class="kit-diffview__field">${esc(r.field)}</div><div class="kit-diffview__values">` +
        `<del class="kit-diffview__old">${esc(r.old || '—')}</del><ins class="kit-diffview__new">${esc(r.new || '—')}</ins></div>` +
        (p.actions === false ? '' : `<label class="kit-diffview__action"><input type="checkbox" class="kit-diffview__check"><span>${esc(r.action || 'Annuler ce changement')}</span></label>`) +
        '</li>').join('') + '</ul></div>',

    Slider: p => `<div class="kit-slider${p.bare ? ' kit-slider--bare' : ''}">` + (p.fields || [p]).map(f => {
      const id = f.id || uid('slider'), unit = esc(f.unit || '');
      return `<div class="kit-slider__field${f.disabled ? ' is-disabled' : ''}"${attr('data-name', f.name)}><div class="kit-slider__head">` +
        `<label class="kit-slider__label" for="${id}">${esc(f.label)}</label><span class="kit-slider__value"><output class="kit-slider__out" for="${id}">${esc(f.value)}</output><span class="kit-slider__unit">${unit}</span></span></div>` +
        `<input class="kit-slider__input" id="${id}" type="range"${attr('name', f.name)} min="${f.min}" max="${f.max}" value="${f.value}"${attr('step', f.step)}${f.disabled ? ' disabled' : ''}>` +
        `<div class="kit-slider__bounds"><span>${f.min} ${unit}</span><span>${f.max} ${unit}</span></div></div>`;
    }).join('') + '</div>',

    Dialog: p => '<div class="kit-dialog"><div class="kit-dialog__veil">' +
      `<div class="kit-dialog__window" role="dialog" aria-modal="true"${attr('aria-label', p.title)}><header class="kit-dialog__header"><h2 class="kit-dialog__title">${esc(p.title)}</h2>` +
      '<button class="kit-dialog__close" type="button" aria-label="Fermer">×</button></header><div class="kit-dialog__body">' +
      (p.text ? `<p class="kit-dialog__text">${esc(p.text)}</p>` : '') + (p.html || '') + '</div><footer class="kit-dialog__footer">' +
      (p.actions || []).map((a, i) => `<button type="button" class="kit-dialog__btn kit-dialog__btn--${a.variant || 'primary'}" data-index="${i}"${attr('id', a.id)}${attr('data-writes', a['data-writes'])}>${esc(a.label)}</button>`).join('') +
      '</footer></div></div></div>',

    TextArea: p => {
      const id = p.id || uid('textarea');
      return `<div class="kit-textarea${p.wide ? ' is-wide' : ''}${p.error ? ' is-error' : ''}${p.disabled ? ' is-disabled' : ''}">` +
        (p.label ? `<label class="kit-textarea__label" for="${id}">${esc(p.label)}</label>` : '') +
        `<textarea class="kit-textarea__field" id="${id}"${attr('name', p.name)} rows="${p.rows || 4}"${attr('maxlength', p.max)}${attr('placeholder', p.placeholder)}${p.disabled ? ' disabled' : ''}>${esc(p.value)}</textarea>` +
        `<div class="kit-textarea__footer"><p class="kit-textarea__hint">${esc(p.error || p.hint || '')}</p>${p.max ? '<span class="kit-textarea__counter" aria-live="polite"></span>' : ''}</div></div>`;
    },

    FileDrop: p => {
      const types = p.types || ['PNG', 'JPG'];
      return `<div class="kit-filedrop${p.wide ? ' is-wide' : ''}"><label class="kit-filedrop__zone" tabindex="0"${attr('id', p.id)}>${ICON.upload}` +
        `<span class="kit-filedrop__title">${esc(p.title || 'Déposez un fichier ou')} <b>parcourez</b></span><span class="kit-filedrop__hint">${esc(p.hint)}</span>` +
        '<span class="kit-filedrop__types">' + types.map(t => `<span class="kit-filedrop__tag">${esc(t)}</span>`).join('') + '</span>' +
        `<input class="kit-filedrop__input" type="file" accept="${esc(p.accept || types.map(t => '.' + t.toLowerCase()).join(','))}"></label>` +
        '<div class="kit-filedrop__error">Type de fichier non accepté.</div><div class="kit-filedrop__file"><div class="kit-filedrop__thumb"></div><div class="kit-filedrop__info">' +
        '<div class="kit-filedrop__name"></div><div class="kit-filedrop__row"><span class="kit-filedrop__size"></span><span class="kit-filedrop__pct">0 %</span></div>' +
        '<div class="kit-filedrop__bar"><div class="kit-filedrop__fill"></div></div></div></div></div>';
    },

    StatBlock: p => {
      const ab = p.abilities || {};
      return `<div class="kit-statblock${p.wide ? ' is-wide' : ''}"${attr('id', p.id)}><div class="kit-statblock__rule"></div><div class="kit-statblock__body">` +
        `<h3 class="kit-statblock__name">${esc(p.name)}</h3><p class="kit-statblock__type">${esc(p.type)}</p><dl class="kit-statblock__vitals">` +
        (p.vitals || []).map(v => `<div class="kit-statblock__vital"><dt>${esc(v.label)}</dt><dd>${esc(v.value)}</dd></div>`).join('') + '</dl><ul class="kit-statblock__abilities">' +
        ABILITIES.map(k => `<li class="kit-statblock__ability"><span class="kit-statblock__abbr">${k}</span><span class="kit-statblock__score">${ab[k] != null ? ab[k] : 10}</span><span class="kit-statblock__mod">${mod(ab[k])}</span></li>`).join('') +
        '</ul><ul class="kit-statblock__features">' + (p.features || []).map(f => `<li class="kit-statblock__feature"><strong>${esc(f.name)}.</strong> ${esc(f.text)}</li>`).join('') + '</ul></div></div>';
    }
  };

  // ---------- hexagon geometry (axial coordinates, 1 hex = 5 pieds) ----------
  const S3 = Math.sqrt(3);
  const GRID_COLORS = { text: 'var(--color-text)', accent: 'var(--color-accent)', primary: 'var(--color-primary)', secondary: 'var(--color-secondary)', success: 'var(--color-success)' };
  function hexRound(q, r) {
    const x = q, z = r, y = -x - z;
    let rx = Math.round(x), ry = Math.round(y), rz = Math.round(z);
    const dx = Math.abs(rx - x), dy = Math.abs(ry - y), dz = Math.abs(rz - z);
    if (dx > dy && dx > dz) rx = -ry - rz; else if (dy > dz) ry = -rx - rz; else rz = -rx - ry;
    return { q: rx, r: rz };
  }
  const hexDist = (a, b) => { const dq = a.q - b.q, dr = a.r - b.r; return (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2; };
  function hexLine(a, b) {
    const n = hexDist(a, b), out = [];
    for (let i = 0; i <= n; i++) { const t = n === 0 ? 0 : i / n; out.push(hexRound(a.q + (b.q - a.q) * t + 1e-6, a.r + (b.r - a.r) * t + 2e-6)); }
    return out;
  }
  const HEX_DIRS = [[1, 0], [1, -1], [0, -1], [-1, 0], [-1, 1], [0, 1]];

  // ---------- kit behaviours ----------
  const I = {
    Select(host) {
      const button = host.querySelector('button'), list = host.querySelector('ul');
      const sel = list.querySelector('[aria-selected="true"]');
      host.value = sel ? sel.dataset.value : '';
      const close = () => { list.hidden = true; button.setAttribute('aria-expanded', 'false'); };
      button.addEventListener('click', e => { e.stopPropagation(); list.hidden = !list.hidden; button.setAttribute('aria-expanded', String(!list.hidden)); });
      list.addEventListener('click', e => {
        const li = e.target.closest('li');
        if (!li) return;
        list.querySelectorAll('li').forEach(x => x.setAttribute('aria-selected', String(x === li)));
        host.querySelector('b[data-value]').textContent = li.textContent;
        host.value = li.dataset.value;
        close();
        fire(host, 'kit:change', { value: host.value });
      });
      document.addEventListener('click', e => { if (!host.contains(e.target)) close(); });
    },

    Toggle(host) {
      host.querySelectorAll('[role="switch"]').forEach(b => b.addEventListener('click', () => {
        const on = b.getAttribute('aria-checked') !== 'true';
        b.setAttribute('aria-checked', String(on));
        fire(host, 'kit:change', { index: +b.dataset.index, id: b.id, on });
      }));
    },

    Tabs(host, p) {
      const root = host.querySelector('.kit-tabs');
      const tabs = Array.from(root.querySelectorAll('[role="tab"]'));
      const panels = (host.__panels || []).map((child, i) => {
        const panel = document.createElement('div');
        panel.setAttribute('role', 'tabpanel');
        panel.id = (p.id || 'tabs') + '-panel-' + i;
        if (tabs[i]) panel.setAttribute('aria-labelledby', tabs[i].id);
        panel.appendChild(child);
        root.appendChild(panel);
        return panel;
      });
      const select = (i, silent) => {
        tabs.forEach((t, j) => t.setAttribute('aria-selected', String(i === j)));
        panels.forEach((x, j) => { x.hidden = i !== j; });
        if (!silent) fire(host, 'kit:change', { index: i });
      };
      tabs.forEach((t, i) => t.addEventListener('click', () => select(i)));
      host.tabs = { select };
      select(p.selected || 0, true);
    },

    MicButton(host, p) {
      const root = host.querySelector('.kit-micbutton'), btnEl = root.querySelector('.kit-micbutton__btn');
      const stateEl = root.querySelector('.kit-micbutton__state'), timerEl = root.querySelector('.kit-micbutton__timer');
      const texts = Object.assign({ idle: 'Prêt à dicter', recording: 'Enregistrement', paused: 'En pause' }, p.texts);
      let state = 'idle', seconds = 0, tick = null, pulse = null, ready = false;
      const fmt = s => String(Math.floor(s / 60)).padStart(2, '0') + ':' + String(s % 60).padStart(2, '0');
      const stop = () => { clearInterval(tick); clearInterval(pulse); tick = pulse = null; root.style.setProperty('--kit-level', '0'); };
      const set = next => {
        state = next; stop();
        if (next === 'idle') seconds = 0;
        if (next === 'recording') {
          tick = setInterval(() => { seconds++; timerEl.textContent = fmt(seconds); }, 1000);
          pulse = setInterval(() => root.style.setProperty('--kit-level', (Math.random() * 0.9 + 0.1).toFixed(2)), 140);
        }
        root.setAttribute('data-state', state);
        stateEl.textContent = texts[state];
        timerEl.textContent = fmt(seconds);
        btnEl.setAttribute('aria-pressed', String(state === 'recording'));
        btnEl.setAttribute('aria-label', (p.label || 'Micro') + ' — ' + texts[state]);
        if (ready) fire(host, 'kit:mic', { state, seconds });
      };
      btnEl.addEventListener('click', () => set(state === 'recording' ? 'paused' : 'recording'));
      host.mic = { set, get state() { return state; }, get seconds() { return seconds; } };
      set(p.state || 'idle');
      ready = true;
    },

    DiffView(host) {
      host.reverted = () => Array.from(host.querySelectorAll('.kit-diffview__row')).filter(r => r.querySelector('.kit-diffview__check:checked')).map(r => +r.dataset.index);
      host.querySelectorAll('.kit-diffview__check').forEach(c => c.addEventListener('change', () => {
        c.closest('.kit-diffview__row').classList.toggle('kit-diffview__row--reverted', c.checked);
        fire(host, 'kit:change', { reverted: host.reverted() });
      }));
    },

    Slider(host) {
      host.querySelectorAll('.kit-slider__field').forEach(field => {
        const input = field.querySelector('.kit-slider__input'), out = field.querySelector('.kit-slider__out');
        const update = () => {
          const min = Number(input.min), max = Number(input.max);
          input.style.setProperty('--fill', ((Number(input.value) - min) / (max - min)) * 100 + '%');
          out.textContent = input.value;
        };
        input.addEventListener('input', update);
        update();
      });
    },

    TextArea(host) {
      const field = host.querySelector('.kit-textarea__field'), counter = host.querySelector('.kit-textarea__counter');
      if (!counter) return;
      const max = parseInt(field.getAttribute('maxlength'), 10) || 200;
      const update = () => {
        const n = field.value.length;
        counter.textContent = n + ' / ' + max;
        counter.classList.toggle('is-near', n >= max * 0.9 && n < max);
        counter.classList.toggle('is-over', n >= max);
      };
      field.addEventListener('input', update);
      update();
    },

    FileDrop(host) {
      const root = host.querySelector('.kit-filedrop'), zone = root.querySelector('.kit-filedrop__zone'), input = root.querySelector('.kit-filedrop__input');
      const thumb = root.querySelector('.kit-filedrop__thumb'), fill = root.querySelector('.kit-filedrop__fill'), pctEl = root.querySelector('.kit-filedrop__pct');
      const exts = input.accept.split(',').map(x => x.trim().toLowerCase()).filter(Boolean);
      let timer = null;
      const size = n => (n < 1024 ? n + ' o' : n < 1048576 ? (n / 1024).toFixed(1).replace('.', ',') + ' Ko' : (n / 1048576).toFixed(1).replace('.', ',') + ' Mo');
      const setPct = p => { fill.style.width = p + '%'; pctEl.textContent = p >= 100 ? 'Terminé' : p + ' %'; root.classList.toggle('is-done', p >= 100); };
      const show = (file, dataURL) => {
        clearInterval(timer);
        const ext = '.' + (file.name.split('.').pop() || '').toLowerCase();
        if (exts.length && !exts.includes(ext)) { root.classList.remove('has-file'); root.classList.add('has-error'); return; }
        root.classList.remove('has-error');
        root.classList.add('has-file');
        root.querySelector('.kit-filedrop__name').textContent = file.name;
        root.querySelector('.kit-filedrop__size').textContent = typeof file.size === 'number' ? size(file.size) : file.size;
        thumb.style.backgroundImage = dataURL ? `url(${dataURL})` : 'none';
        thumb.textContent = dataURL ? '' : ext.slice(1).toUpperCase();
        let p = 0;
        setPct(0);
        timer = setInterval(() => {
          p = Math.min(100, p + 10);
          setPct(p);
          if (p >= 100) { clearInterval(timer); fire(host, 'kit:file', { name: file.name, size: file.size, dataURL }); }
        }, 120);
      };
      const read = file => {
        if (/^image\//.test(file.type)) { const r = new FileReader(); r.onload = () => show(file, r.result); r.readAsDataURL(file); } else show(file);
      };
      input.addEventListener('change', () => { if (input.files[0]) read(input.files[0]); });
      zone.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); input.click(); } });
      ['dragenter', 'dragover'].forEach(t => zone.addEventListener(t, e => { e.preventDefault(); zone.classList.add('is-over'); }));
      ['dragleave', 'drop'].forEach(t => zone.addEventListener(t, e => { e.preventDefault(); zone.classList.remove('is-over'); }));
      zone.addEventListener('drop', e => { const f = e.dataTransfer && e.dataTransfer.files[0]; if (f) read(f); });
      host.filedrop = { simulate: file => show(file), reset: () => { clearInterval(timer); root.classList.remove('has-file', 'has-error', 'is-done'); } };
    },

    Dialog(host) {
      host.querySelector('.kit-dialog__close').addEventListener('click', () => { host.hidden = true; });
    },

    HexMap(host, p) {
      const root = host.querySelector('.kit-hexmap'), view = root.querySelector('.hm-view'), stage = root.querySelector('.hm-stage');
      const gridPath = root.querySelector('.hm-grid-path'), over = root.querySelector('.hm-over'), tip = root.querySelector('.hm-tip');
      const m = p.map || { width: 800, height: 560 };
      const NS = 'http://www.w3.org/2000/svg';
      const hx = { W: m.width, H: m.height, z: 1, px: 0, py: 0, fitted: false, root, view, stage, over, tip, handlers: {},
        grid: Object.assign({ size: 32, ox: 20, oy: 20, orient: 'pointy', color: 'text', opacity: 45 }, p.grid) };
      const g = hx.grid;
      hx.on = (type, fn) => (hx.handlers[type] = hx.handlers[type] || []).push(fn);
      hx.emit = (type, d) => (hx.handlers[type] || []).forEach(fn => fn(d));
      hx.center = (q, r) => {
        const s = g.size;
        let x, y;
        if (g.orient === 'pointy') { x = s * S3 * (q + r / 2); y = s * 1.5 * r; } else { x = s * 1.5 * q; y = s * S3 * (r + q / 2); }
        return { x: x + g.ox + s, y: y + g.oy + s };
      };
      hx.toHex = (x, y) => {
        const s = g.size;
        let q, r;
        x -= g.ox + s; y -= g.oy + s;
        if (g.orient === 'pointy') { q = (S3 / 3 * x - y / 3) / s; r = (2 / 3 * y) / s; } else { q = (2 / 3 * x) / s; r = (-x / 3 + S3 / 3 * y) / s; }
        return hexRound(q, r);
      };
      hx.dist = hexDist;
      hx.line = hexLine;
      hx.neighbors = h => HEX_DIRS.map(d => ({ q: h.q + d[0], r: h.r + d[1] }));
      hx.label = n => `${n} hex · ${n * 5} pi`;
      hx.corners = c => {
        const pts = [], off = g.orient === 'pointy' ? -30 : 0;
        for (let i = 0; i < 6; i++) { const a = (60 * i + off) * Math.PI / 180; pts.push([c.x + g.size * Math.cos(a), c.y + g.size * Math.sin(a)]); }
        return pts;
      };
      hx.svg = (name, attrs, cls) => {
        const e = document.createElementNS(NS, name);
        if (cls) e.setAttribute('class', cls);
        Object.keys(attrs).forEach(k => e.setAttribute(k, attrs[k]));
        return e;
      };
      hx.poly = (h, cls) => hx.svg('polygon', { points: hx.corners(hx.center(h.q, h.r)).map(v => v[0].toFixed(1) + ',' + v[1].toFixed(1)).join(' ') }, cls);
      hx.drawGrid = () => {
        let d = '';
        const s = g.size;
        for (let q = -40; q <= 60; q++) {
          for (let r = -40; r <= 50; r++) {
            const c = hx.center(q, r);
            if (c.x < -s || c.x > hx.W + s || c.y < -s || c.y > hx.H + s) continue;
            d += 'M' + hx.corners(c).map(v => v[0].toFixed(1) + ' ' + v[1].toFixed(1)).join('L') + 'Z';
          }
        }
        gridPath.setAttribute('d', d);
      };
      hx.setGrid = patch => {
        Object.assign(g, patch || {});
        root.style.setProperty('--hm-grid', GRID_COLORS[g.color] || GRID_COLORS.text);
        gridPath.style.opacity = g.opacity / 100;
        hx.drawGrid();
        hx.emit('grid', g);
      };
      hx.applyView = silent => {
        stage.style.transform = `translate(${hx.px}px,${hx.py}px) scale(${hx.z})`;
        if (!silent) hx.emit('view', { z: hx.z, px: hx.px, py: hx.py });
      };
      hx.setView = v => { hx.z = v.z; hx.px = v.px; hx.py = v.py; hx.fitted = true; hx.applyView(true); };
      hx.zoomAt = (f, cx, cy) => {
        const nz = Math.max(0.3, Math.min(4, hx.z * f)), k = nz / hx.z;
        hx.px = cx - (cx - hx.px) * k; hx.py = cy - (cy - hx.py) * k; hx.z = nz;
        hx.applyView();
      };
      hx.fit = silent => {
        const w = view.clientWidth, h = view.clientHeight;
        if (!w || !h) return false;
        hx.z = Math.min(w / hx.W, h / hx.H);
        hx.px = (w - hx.W * hx.z) / 2; hx.py = (h - hx.H * hx.z) / 2;
        hx.fitted = true;
        hx.applyView(silent);
        return true;
      };
      hx.world = e => {
        const b = view.getBoundingClientRect(), lx = e.clientX - b.left, ly = e.clientY - b.top;
        return { lx, ly, x: (lx - hx.px) / hx.z, y: (ly - hx.py) / hx.z };
      };
      hx.showTip = (text, w) => { tip.textContent = text; tip.style.left = (w.lx + 16) + 'px'; tip.style.top = (w.ly + 16) + 'px'; tip.hidden = false; };
      hx.hideTip = () => { tip.hidden = true; };
      hx.clearOver = () => { while (over.firstChild) over.removeChild(over.firstChild); };
      hx.drawPath = (a, b) => {
        hx.clearOver();
        const cells = hexLine(a, b);
        cells.forEach((h, i) => over.appendChild(hx.poly(h, i === cells.length - 1 ? 'hm-end' : 'hm-step')));
        over.appendChild(hx.svg('polyline', { points: cells.map(h => { const c = hx.center(h.q, h.r); return c.x.toFixed(1) + ',' + c.y.toFixed(1); }).join(' ') }, 'hm-line'));
        return hexDist(a, b);
      };

      if (p.pan !== false) {
        let down = null;
        view.addEventListener('pointerdown', e => {
          if (e.button !== 0 || e.target.closest('.kit-token__item, [data-no-pan]')) return;
          down = { x: e.clientX, y: e.clientY, px: hx.px, py: hx.py, moved: false };
          view.setPointerCapture(e.pointerId);
        });
        view.addEventListener('pointermove', e => {
          if (!down) return;
          const dx = e.clientX - down.x, dy = e.clientY - down.y;
          if (!down.moved && Math.abs(dx) + Math.abs(dy) > 4) { down.moved = true; view.classList.add('hm-drag'); }
          if (down.moved) { hx.px = down.px + dx; hx.py = down.py + dy; hx.applyView(); }
        });
        view.addEventListener('pointerup', e => {
          const wasClick = down && !down.moved;
          down = null;
          view.classList.remove('hm-drag');
          if (wasClick) hx.emit('click', hx.world(e));
        });
        view.addEventListener('wheel', e => {
          e.preventDefault();
          const b = view.getBoundingClientRect();
          hx.zoomAt(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientX - b.left, e.clientY - b.top);
        }, { passive: false });
      }
      root.querySelectorAll('[data-act]').forEach(b => b.addEventListener('click', () => {
        const a = b.getAttribute('data-act'), cx = view.clientWidth / 2, cy = view.clientHeight / 2;
        if (a === 'in') hx.zoomAt(1.25, cx, cy); else if (a === 'out') hx.zoomAt(1 / 1.25, cx, cy); else hx.fit();
      }));
      if (p.measure) {
        let start = null, end = null;
        hx.on('click', w => {
          const h = hx.toHex(w.x, w.y);
          if (!start || end) { start = h; end = null; hx.clearOver(); over.appendChild(hx.poly(h, 'hm-end')); hx.hideTip(); } else { end = h; hx.showTip(hx.label(hx.drawPath(start, end)), w); }
        });
        view.addEventListener('pointermove', e => {
          if (!start || end) return;
          const w = hx.world(e);
          hx.showTip(hx.label(hx.drawPath(start, hx.toHex(w.x, w.y))), w);
        });
        hx.on('grid', () => { start = end = null; hx.clearOver(); hx.hideTip(); });
      }
      if (p.fit !== false && 'ResizeObserver' in window) new ResizeObserver(() => { if (!hx.fitted) hx.fit(true); }).observe(view);
      host.hexmap = hx;
      hx.setGrid();
      hx.applyView(true);
    }
  };

  // ---------- mounting kits and micro-frontends ----------
  function renderKit(host) {
    const name = host.getAttribute('data-kit');
    if (host.__kit || !R[name]) return;
    host.__kit = true;
    let p = {};
    try { p = JSON.parse(host.getAttribute('data-props') || '{}'); } catch (e) { console.warn('data-props illisible pour le kit', name, e); }
    if (name === 'Tabs') host.__panels = Array.from(host.children);
    host.innerHTML = R[name](p, host);
    if (I[name]) I[name](host, p);
  }

  function load(el) {
    el.__mf = true;
    const name = el.getAttribute('data-component');
    return fetch(BASE + 'components/' + name + '.html')
      .then(r => { if (!r.ok) throw new Error('HTTP ' + r.status); return r.text(); })
      .then(html => {
        const tpl = document.createElement('template');
        tpl.innerHTML = html;
        const scripts = Array.from(tpl.content.querySelectorAll('script'));
        scripts.forEach(s => s.remove());
        el.replaceChildren(tpl.content);
        el.querySelectorAll('[data-kit]').forEach(renderKit);
        scripts.forEach(s => { const n = document.createElement('script'); n.textContent = s.textContent; el.appendChild(n); });
        return mount(el);
      })
      .catch(err => { el.innerHTML = `<p class="mock-error">Micro-frontend ${esc(name)} indisponible (${esc(err.message)}).</p>`; });
  }

  function mount(scope) {
    scope = scope || document;
    scope.querySelectorAll('[data-kit]').forEach(renderKit);
    return Promise.all(Array.from(scope.querySelectorAll('[data-component]')).filter(el => !el.__mf).map(load));
  }

  const kitHTML = (name, props, inner) => `<div data-kit="${esc(name)}" data-props="${esc(JSON.stringify(props || {}))}">${inner || ''}</div>`;

  function kit(name, props) {
    const d = document.createElement('div');
    d.setAttribute('data-kit', name);
    d.setAttribute('data-props', JSON.stringify(props || {}));
    renderKit(d);
    return d;
  }

  // A kit Dialog opened as a modal over the page.
  function dialog(o) {
    const wrap = document.createElement('div');
    wrap.className = 'mf-modal' + (o.wide ? ' mf-modal--wide' : '');
    if (o.id) wrap.id = o.id;
    wrap.innerHTML = R.Dialog({ title: o.title, text: o.text, html: o.html, actions: o.actions || [{ label: 'Fermer', variant: 'ghost' }] });
    document.body.appendChild(wrap);
    const onKey = e => { if (e.key === 'Escape') api.close(); };
    const api = {
      el: wrap,
      body: wrap.querySelector('.kit-dialog__body'),
      close() { wrap.remove(); document.removeEventListener('keydown', onKey); if (o.onClose) o.onClose(); }
    };
    document.addEventListener('keydown', onKey);
    wrap.querySelector('.kit-dialog__close').addEventListener('click', api.close);
    wrap.querySelector('.kit-dialog__veil').addEventListener('click', e => { if (e.target === e.currentTarget) api.close(); });
    wrap.querySelectorAll('.kit-dialog__footer [data-index]').forEach(b => b.addEventListener('click', () => {
      const a = (o.actions || [])[+b.dataset.index];
      if (a && a.onClick) a.onClick(api); else api.close();
    }));
    mount(wrap);
    return api;
  }

  // A command's lifecycle, shown in a corner of the page: queued, confirmed, applied.
  function command(name, payload, opts) {
    opts = opts || {};
    let box = document.getElementById('mock-commands');
    if (!box) {
      box = document.createElement('div');
      box.id = 'mock-commands';
      box.className = 'mock-commands';
      box.setAttribute('aria-live', 'polite');
      document.body.appendChild(box);
    }
    const item = document.createElement('div');
    item.className = 'mock-command';
    item.dataset.state = 'queued';
    item.innerHTML = `<span class="mock-command__label">${esc(opts.label || name)}</span><code>${esc(name)}</code><span class="mock-command__state">en file</span>`;
    box.appendChild(item);
    const state = item.querySelector('.mock-command__state');
    const step = (s, text) => { item.dataset.state = s; state.textContent = text; if (opts.onState) opts.onState(s); };
    if (opts.onState) opts.onState('queued');
    return new Promise(resolve => {
      setTimeout(() => {
        step('confirmed', 'confirmée');
        setTimeout(() => {
          step('applied', 'appliquée');
          resolve(payload);
          setTimeout(() => item.remove(), 2600);
        }, opts.delay || 550);
      }, 300);
    });
  }

  function fixture(path) {
    if (Array.isArray(path)) return Promise.all(path.map(fixture));
    if (!fixtures[path]) fixtures[path] = fetch(BASE + path).then(r => { if (!r.ok) throw new Error(path + ' : HTTP ' + r.status); return r.json(); });
    return fixtures[path].then(clone);
  }

  // State shared by the tabs of this machine (DM view, player view): localStorage, no network.
  const PREFIX = 'fdm-mock:';
  const store = {
    get(key, fallback) {
      try { const v = localStorage.getItem(PREFIX + key); if (v) return JSON.parse(v); } catch (e) { /* stockage indisponible : on garde la fixture */ }
      return clone(fallback);
    },
    set(key, value) {
      try { localStorage.setItem(PREFIX + key, JSON.stringify(value)); } catch (e) { /* stockage indisponible */ }
      document.dispatchEvent(new CustomEvent('mock:store', { detail: { key, value } }));
    },
    remove(key) {
      try { localStorage.removeItem(PREFIX + key); } catch (e) { /* stockage indisponible */ }
      document.dispatchEvent(new CustomEvent('mock:store', { detail: { key, value: null } }));
    },
    on(key, fn) {
      document.addEventListener('mock:store', e => { if (e.detail.key === key) fn(e.detail.value); });
      window.addEventListener('storage', e => { if (e.key === PREFIX + key) fn(e.newValue ? JSON.parse(e.newValue) : null); });
    }
  };

  const props = el => { try { return JSON.parse(el.getAttribute('data-props') || '{}'); } catch (e) { return {}; } };
  const param = name => new URLSearchParams(location.search).get(name);
  const on = (name, fn) => document.addEventListener('fdm:' + name, e => fn(e.detail));
  const emit = (name, detail) => document.dispatchEvent(new CustomEvent('fdm:' + name, { detail }));

  document.addEventListener('click', e => {
    const b = e.target.closest('button[data-href]');
    if (!b || b.disabled) return;
    if (b.dataset.target === '_blank') window.open(b.dataset.href, '_blank'); else location.href = b.dataset.href;
  });

  window.Mock = { base: BASE, abilities: ABILITIES, esc, attr, clone, mod, modOf, norm, uid, art, mount, kit, kitHTML, tokenHTML: tokenItem,
    renderers: R, dialog, command, fixture, store, props, param, on, emit };
  mount(document);
})();
