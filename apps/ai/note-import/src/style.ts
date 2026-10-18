// The Micro-UI's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share
// and the mock-up's `.mf-notes` rules, scoped to it, every value a token.

export const STYLE = `
.mf-notes { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-notes *, .mf-notes *::before, .mf-notes *::after { box-sizing: inherit; }
.mf-notes h2, .mf-notes h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-notes h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-notes h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-notes p { margin: 0; }
.mf-notes.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-notes .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-notes .muted { color: var(--color-text-muted); }
.mf-notes .small { font-size: var(--font-size-small); }
.mf-notes .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-notes .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-notes .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-notes .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-notes .item.is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-notes .item.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-notes .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-notes .kit-textarea.is-wide { max-width: none; }
.mf-notes .kit-checkbox fieldset { padding: var(--space-2) var(--space-3); }
.mf-notes .kit-checkbox label span small { display: block; color: var(--color-text-muted); }
.mf-notes__type { font-family: var(--font-family-heading); text-transform: uppercase; font-size: var(--font-size-small); color: var(--color-secondary); margin-right: var(--space-1); }
.mf-notes__dup { color: var(--color-accent); font-size: var(--font-size-small); }
`;
