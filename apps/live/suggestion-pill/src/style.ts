// The Micro-UI's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the few helpers the mock-up's style.css gives the
// pill (`.icon-btn`, the compact MicButton) and the mock-up's `.mf-pill` rules,
// scoped to it, every value a token.

export const STYLE = `
.mf-pill { position: fixed; right: var(--space-3); bottom: var(--space-3); left: auto; top: auto; z-index: 1000; display: grid; gap: var(--space-2); justify-items: end; width: min(calc(100vw - var(--space-4)), calc(var(--space-5) * 8)); max-height: calc(100vh - var(--space-4)); margin: 0; box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-pill *, .mf-pill *::before, .mf-pill *::after { box-sizing: inherit; }
.mf-pill h2, .mf-pill h3, .mf-pill p { margin: 0; }
.mf-pill h2, .mf-pill h3 { font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-pill .small { font-size: var(--font-size-small); }
.mf-pill .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-pill .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-pill .kit-micbutton--compact .kit-micbutton__btn { width: calc(var(--space-5) + var(--space-2)); height: calc(var(--space-5) + var(--space-2)); }
.mf-pill .kit-micbutton--compact .kit-micbutton__label { font-size: var(--font-size-base); }
.mf-pill__card { width: 100%; display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-secondary); border-radius: var(--radius-large); box-shadow: var(--shadow-high); overflow-y: auto; min-height: 0; }
.mf-pill__head { display: flex; align-items: center; justify-content: space-between; }
.mf-pill__head h2 { font-size: var(--font-size-base); color: var(--color-accent); }
.mf-pill__heard { color: var(--color-text-muted); font-style: italic; min-height: 1.5em; }
.mf-pill__status { color: var(--color-danger); }
.mf-pill__stack { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); }
.mf-pill__stack button { width: 100%; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-2); align-items: center; text-align: left; padding: var(--space-2) var(--space-3); background: var(--color-surface-2); color: var(--color-text); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); font: inherit; cursor: pointer; }
.mf-pill__stack button:hover { border-color: var(--color-primary); }
.mf-pill__stack button.is-armed { border-color: var(--color-success); }
.mf-pill__stack li:first-child button { border-color: var(--color-accent); }
.mf-pill__stack b { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-pill__stack small { display: block; color: var(--color-text-muted); }
.mf-pill__icon { display: grid; place-items: center; width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); border-radius: var(--radius-pill); background: var(--color-secondary); color: var(--color-text); font-family: var(--font-family-heading); }
.mf-pill__icon.is-sound { background: var(--color-success); color: var(--color-primary-contrast); }
.mf-pill__empty { color: var(--color-text-muted); font-size: var(--font-size-small); }
.mf-pill__history { border-top: var(--border-width) solid var(--color-border); padding-top: var(--space-2); display: grid; gap: var(--space-1); }
.mf-pill__history h3 { font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-pill__mic { padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); box-shadow: var(--shadow-high); }
`;
