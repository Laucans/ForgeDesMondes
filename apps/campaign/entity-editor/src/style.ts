// The sheet's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the helpers the mock-up's pages share and the
// mock-up's `.mf-entity` rules, scoped to the sheet, every value a token.
// The kit's Select is a drop list opened by a click; the sheet needs a field
// a browser and a screen reader know as a list of options, so it dresses a
// native <select> as the kit's Select.

export const STYLE = `
.mf-entity { box-sizing: border-box; display: grid; gap: var(--space-4); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-entity *, .mf-entity *::before, .mf-entity *::after { box-sizing: inherit; }
.mf-entity [hidden] { display: none !important; }
.mf-entity h1, .mf-entity h2, .mf-entity h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-entity h1 { font-size: var(--font-size-heading); color: var(--color-primary); }
.mf-entity h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-entity h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-entity p { margin: 0; }
.mf-entity .layout-2 { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
@media (max-width: 960px) { .mf-entity .layout-2 { grid-template-columns: minmax(0, 1fr); } }
.mf-entity .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-entity .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-entity .muted { color: var(--color-text-muted); }
.mf-entity .small { font-size: var(--font-size-small); }
.mf-entity .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-entity .stack { display: grid; gap: var(--space-3); align-content: start; min-width: 0; }
.mf-entity .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-entity .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-entity .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-entity .kit-input.is-wide, .mf-entity .kit-select.is-wide { max-width: none; }
.mf-entity .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-entity .kit-select select:hover { border-color: var(--color-primary); }
.mf-entity .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-entity__unsaved { position: sticky; top: var(--space-2); z-index: 20; display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2) var(--space-4); padding: var(--space-2) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-left: calc(var(--border-width) * 4) solid var(--color-accent); border-radius: var(--radius-medium); box-shadow: var(--shadow-high); }
.mf-entity__notice { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2) var(--space-4); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-accent); border-radius: var(--radius-medium); }
.mf-entity__status { min-height: var(--space-4); }
.mf-entity__status p { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-entity__status .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-entity__status .is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-entity__head { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-4); align-items: center; }
.mf-entity__portrait { display: grid; place-items: center; width: calc(var(--space-5) * 2.5); height: calc(var(--space-5) * 2.5); border-radius: var(--radius-pill); overflow: hidden; border: calc(var(--border-width) * 3) solid var(--color-secondary); box-shadow: var(--shadow-high); background: var(--color-surface-2); }
.mf-entity__glyph { font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: var(--font-size-large); text-transform: uppercase; color: var(--color-text); }
.mf-entity .kit-token__portrait { display: grid; place-items: center; width: 100%; height: 100%; background: var(--color-surface-2); }
.mf-entity__title { display: grid; gap: var(--space-1); }
.mf-entity__grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(calc(var(--space-5) * 5), 1fr)); gap: var(--space-3); }
.mf-entity__abilities { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: var(--space-2); }
.mf-entity__abilities .kit-input label { font-size: var(--font-size-small); }
.mf-entity__abilities .kit-input input { width: 100%; text-align: center; padding: var(--space-1); }
.mf-entity__features { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-entity__features li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--space-2); align-items: start; padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-entity__features b { color: var(--color-accent); }
.mf-entity__token { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); align-items: center; }
.mf-entity__token .kit-token { padding: 0; background: transparent; }
.mf-entity__token .kit-token__portrait { transform-origin: center; }
.mf-entity__kept { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); font-size: var(--font-size-small); }
.mf-entity__kept li::before { content: "✓ "; color: var(--color-success); }
.mf-entity .kit-statblock { max-width: none; }
@media (max-width: 760px) { .mf-entity__abilities { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
`;
