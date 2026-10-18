// The Micro-UI's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share and
// the mock-up's `.bm` rules, scoped to the Micro-UI, every value a token.

export const STYLE = `
.bm { box-sizing: border-box; display: flex; flex-direction: column; gap: var(--space-3); min-width: 0; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.bm *, .bm *::before, .bm *::after { box-sizing: inherit; }
.bm--fill { height: 100%; }
.bm h2, .bm h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.bm h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.bm h3 { font-size: var(--font-size-base); color: var(--color-text); }
.bm p { margin: 0; }
.bm .muted { color: var(--color-text-muted); }
.bm .small { font-size: var(--font-size-small); }
.bm .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.bm .stack { display: grid; gap: var(--space-3); align-content: start; }
.bm .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.bm .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.bm .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.bm .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.bm .chip.is-on { background: var(--color-primary); border-color: var(--color-primary); color: var(--color-primary-contrast); font-weight: var(--font-weight-heading); }
.bm .chip--pj { border-color: var(--color-success); }
.bm .chip--pnj { border-color: var(--color-secondary); }
.bm .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.bm .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.bm .num-input { width: calc(var(--space-5) * 2); padding: var(--space-1) var(--space-2); background: var(--color-surface); color: var(--color-text); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-small); font: inherit; }
.bm .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.bm .kit-button--small button { font-size: var(--font-size-small); padding: var(--space-1) var(--space-3); }
.bm .kit-button .danger { background: var(--color-danger); color: var(--color-primary-contrast); }
.bm .kit-input.is-wide { max-width: none; }
.bm .kit-toggle.kit-toggle--bare { padding: 0; background: transparent; border: 0; box-shadow: none; }
.bm-toolbar { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.bm-toolbar:empty { display: none; }
.bm-title { display: grid; gap: var(--space-1); }
.bm-body { display: flex; gap: var(--space-3); min-height: 0; flex: 1; }
.bm-map { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.bm .kit-hexmap { flex: 1; display: flex; flex-direction: column; min-height: 0; padding: var(--space-2); }
.bm .kit-hexmap .hm-bar { margin-bottom: var(--space-2); }
.bm .kit-hexmap .hm-view { flex: 1; height: auto; min-height: calc(var(--space-5) * 13); }
.bm--fill .kit-hexmap { padding: 0; border: 0; border-radius: 0; background: transparent; box-shadow: none; }
.bm--fill .kit-hexmap .hm-view { min-height: 0; border: 0; border-radius: 0; }
.bm--fill .kit-hexmap .hm-bar { position: absolute; right: var(--space-3); bottom: var(--space-3); z-index: 4; margin: 0; }
.bm-side { flex: none; width: calc(var(--space-5) * 7); display: grid; gap: var(--space-2); align-content: start; padding: var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); overflow-y: auto; }
.bm-side[hidden] { display: none; }
.bm-dead { color: var(--color-danger); font-weight: var(--font-weight-heading); }
.bm-status { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.bm-status[hidden] { display: none; }
.bm-status.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.bm-status.is-ok { border-color: var(--color-success); color: var(--color-success); }
.bm-tokens.kit-token { position: absolute; inset: 0; display: block; padding: 0; background: transparent; pointer-events: none; }
.bm-tokens .kit-token__item { position: absolute; transform: translate(-50%, calc((var(--space-4) + var(--space-2)) * -1)) scale(var(--bm-scale, 0.8)); transform-origin: 50% calc(var(--space-4) + var(--space-2)); pointer-events: auto; cursor: grab; touch-action: none; }
.bm-tokens .kit-token__item--carried { transform: translate(-50%, calc((var(--space-4) + var(--space-2)) * -1)) scale(calc(var(--bm-scale, 0.8) * 1.2)); z-index: 3; cursor: grabbing; transition: none; }
.bm-tokens .kit-token__name { text-shadow: var(--shadow-low); }
.bm--readonly .bm-tokens .kit-token__item { pointer-events: none; cursor: default; }
.bm .bm-selected .kit-token__disc { outline: calc(var(--border-width) * 3) solid var(--color-primary); outline-offset: calc(var(--border-width) * 3); }
.bm-drop { position: absolute; z-index: 5; display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-primary); border-radius: var(--radius-medium); box-shadow: var(--shadow-high); cursor: default; }
@media (max-width: 960px) { .bm-body { flex-direction: column; } .bm-side { width: auto; } }
`;
