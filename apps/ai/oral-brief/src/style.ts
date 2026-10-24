// The Micro-UI's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share
// and the mock-up's `.mf-brief` rules, scoped to it, every value a token.

export const STYLE = `
.mf-brief { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-brief *, .mf-brief *::before, .mf-brief *::after { box-sizing: inherit; }
.mf-brief h2, .mf-brief h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-brief h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-brief h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-brief p { margin: 0; }
.mf-brief.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-brief .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-brief .muted { color: var(--color-text-muted); }
.mf-brief .small { font-size: var(--font-size-small); }
.mf-brief .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-brief .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-brief .item.is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-brief .item.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-brief .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-brief .kit-textarea.is-wide { max-width: none; }
.mf-brief .kit-card { max-width: none; }
.mf-brief .kit-badge { display: inline-flex; }
.mf-brief__lines { margin: 0; padding-left: var(--space-4); display: grid; gap: var(--space-1); max-height: calc(var(--space-5) * 5); overflow-y: auto; }
.mf-brief__result { display: grid; gap: var(--space-3); }
.mf-brief__cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(calc(var(--space-5) * 6), 1fr)); gap: var(--space-3); }
.mf-brief__col { display: grid; gap: var(--space-2); align-content: start; padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-brief__col h3 { font-size: var(--font-size-small); color: var(--color-accent); }
.mf-brief__col ol, .mf-brief__col ul { margin: 0; padding-left: var(--space-4); display: grid; gap: var(--space-1); }
.mf-brief__col small { color: var(--color-text-muted); }
.mf-brief__upd { display: inline-flex; gap: var(--space-2); align-items: center; }
`;
