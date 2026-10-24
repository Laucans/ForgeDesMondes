// The Micro-UI's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share and
// the mock-up's `.mf-tocreate` rules, scoped to it, every value a token.

export const STYLE = `
.mf-tocreate { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-tocreate *, .mf-tocreate *::before, .mf-tocreate *::after { box-sizing: inherit; }
.mf-tocreate h2, .mf-tocreate h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-tocreate h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-tocreate h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-tocreate p { margin: 0; }
.mf-tocreate.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-tocreate .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-tocreate .muted { color: var(--color-text-muted); }
.mf-tocreate .small { font-size: var(--font-size-small); }
.mf-tocreate .quote { padding: var(--space-2) var(--space-3); border-left: calc(var(--border-width) * 3) solid var(--color-secondary); background: var(--color-surface); border-radius: var(--radius-small); color: var(--color-text-muted); font-style: italic; }
.mf-tocreate .item { padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-tocreate .item.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-tocreate .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-tocreate .kit-button--small button, .mf-tocreate .kit-button--small a { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-tocreate .kit-button a { font: inherit; font-weight: 600; letter-spacing: .04em; border-radius: var(--radius-pill); border: var(--border-width) solid var(--color-primary); background: var(--color-surface-2); color: var(--color-primary); text-decoration: none; box-shadow: var(--shadow-low); }
.mf-tocreate .kit-textarea.is-wide { max-width: none; }
.mf-tocreate .kit-micbutton--compact .kit-micbutton__btn { width: calc(var(--space-5) + var(--space-2)); height: calc(var(--space-5) + var(--space-2)); }
.mf-tocreate .kit-micbutton--compact .kit-micbutton__label { font-size: var(--font-size-base); }
.mf-tocreate__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-3); }
.mf-tocreate__item { display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) dashed var(--color-accent); border-radius: var(--radius-medium); }
.mf-tocreate__item.is-created { border-style: solid; border-color: var(--color-success); }
.mf-tocreate__refine { display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface-2); border-radius: var(--radius-medium); }
.mf-tocreate__proposal { display: grid; gap: var(--space-2); margin: 0; }
.mf-tocreate__proposal dt { font-family: var(--font-family-heading); text-transform: uppercase; font-size: var(--font-size-small); color: var(--color-accent); }
.mf-tocreate__proposal dd { margin: 0 0 var(--space-2); }
.mf-tocreate .kit-statblock { max-width: none; }
.mf-tocreate .kit-statblock .kit-statblock__vitals { list-style: none; }
.mf-tocreate .kit-statblock .kit-statblock__vital strong { font-weight: 700; color: var(--color-accent); }
`;
