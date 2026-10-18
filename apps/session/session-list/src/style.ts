// The list's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share
// and the mock-up's `.mf-sessions` rules, scoped to the list, every value a
// token.

export const STYLE = `
.mf-sessions, .mf-sessions__modal { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-sessions *, .mf-sessions *::before, .mf-sessions *::after, .mf-sessions__modal *, .mf-sessions__modal *::before, .mf-sessions__modal *::after { box-sizing: inherit; }
.mf-sessions h2, .mf-sessions h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-sessions h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sessions h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-sessions p, .mf-sessions__modal p { margin: 0; }
.mf-sessions.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-sessions .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-sessions .muted, .mf-sessions__modal .muted { color: var(--color-text-muted); }
.mf-sessions .small, .mf-sessions__modal .small { font-size: var(--font-size-small); }
.mf-sessions .kit-button.kit-button--bare, .mf-sessions__modal .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-sessions .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-sessions__modal .kit-input.is-wide { max-width: none; }
.mf-sessions__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-3); }
.mf-sessions__item { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-sessions__item.is-next { border-color: var(--color-primary); box-shadow: var(--shadow-high); }
.mf-sessions__num { display: grid; place-items: center; width: calc(var(--space-5) + var(--space-1)); height: calc(var(--space-5) + var(--space-1)); border-radius: var(--radius-pill); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); font-family: var(--font-family-heading); font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sessions__main { display: grid; gap: var(--space-2); min-width: 0; }
.mf-sessions__summary { color: var(--color-text-muted); }
.mf-sessions__status, .mf-sessions__error { min-height: var(--space-3); }
.mf-sessions__status:empty { display: none; }
.mf-sessions__status, .mf-sessions__error { color: var(--color-danger); }
.mf-sessions__modal { position: fixed; inset: 0; z-index: 100; }
.mf-sessions__modal .kit-dialog, .mf-sessions__modal .kit-dialog__veil { height: 100%; }
.mf-sessions__modal .kit-dialog__window { max-height: 88vh; }
.mf-sessions__modal .kit-dialog__body > * + * { margin-top: var(--space-3); }
`;
