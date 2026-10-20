// The panel's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share and
// the fragment's `.mf-grid` rules, scoped to the panel, every value a token.
// The kit's Select is a drop list opened by a click; the panel needs a field a
// browser and a screen reader know as a list of options, so it dresses a
// native <select> as the kit's Select.

export const STYLE = `
.mf-grid { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-grid *, .mf-grid *::before, .mf-grid *::after { box-sizing: inherit; }
.mf-grid.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-grid h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; font-size: var(--font-size-large); color: var(--color-accent); }
.mf-grid p { margin: 0; }
.mf-grid .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-grid .muted { color: var(--color-text-muted); }
.mf-grid .small { font-size: var(--font-size-small); }
.mf-grid .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-grid .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-grid .kit-slider.kit-slider--bare { padding: 0; background: transparent; border: 0; box-shadow: none; max-width: none; }
.mf-grid .kit-select.is-wide { max-width: none; }
.mf-grid .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-grid .kit-select select:hover { border-color: var(--color-primary); }
.mf-grid .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-grid__layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); gap: var(--space-4); align-items: start; }
.mf-grid .kit-hexmap .hm-view { height: auto; min-height: calc(var(--space-5) * 11); }
.mf-grid__result { min-height: var(--space-4); }
.mf-grid__result p { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-grid__result .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-grid__result .is-error { border-color: var(--color-danger); color: var(--color-danger); }
@media (max-width: 960px) { .mf-grid__layout { grid-template-columns: minmax(0, 1fr); } }
`;
