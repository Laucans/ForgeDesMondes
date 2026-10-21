// The board's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share and
// the mock-up's `.mf-scenes` rules, scoped to the board, every value a token.
// The kit's Select is a drop list opened by a click; the board needs a field
// a browser and a screen reader know as a list of options, so it dresses a
// native <select> as the kit's Select.

export const STYLE = `
.mf-scenes { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-scenes *, .mf-scenes *::before, .mf-scenes *::after { box-sizing: inherit; }
.mf-scenes h2, .mf-scenes h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-scenes h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-scenes h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-scenes p { margin: 0; }
.mf-scenes.stack, .mf-scenes .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-scenes .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-scenes .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-scenes .muted { color: var(--color-text-muted); }
.mf-scenes .small { font-size: var(--font-size-small); }
.mf-scenes .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-scenes .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); text-decoration: none; cursor: pointer; }
.mf-scenes .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-scenes .chip--todo { border-style: dashed; border-color: var(--color-accent); color: var(--color-accent); }
.mf-scenes .chip--pj { border-color: var(--color-success); }
.mf-scenes .chip--pnj { border-color: var(--color-secondary); }
.mf-scenes .chip--lieu { border-color: var(--color-accent); }
.mf-scenes .chip--media { border-color: var(--color-primary); }
.mf-scenes .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-scenes .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-scenes .icon-btn:disabled { opacity: 0.55; cursor: not-allowed; border-color: var(--color-border); color: var(--color-text-muted); }
.mf-scenes .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-scenes .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-scenes .kit-textarea.is-wide, .mf-scenes .kit-select.is-wide { max-width: none; }
.mf-scenes .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-scenes .kit-select select:hover { border-color: var(--color-primary); }
.mf-scenes .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-scenes__notice:empty { display: none; }
.mf-scenes__notice { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-danger); color: var(--color-danger); }
.mf-scenes__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-3); }
.mf-scenes__scene { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-scenes__order { display: grid; gap: var(--space-1); justify-items: center; align-content: start; }
.mf-scenes__order b { font-family: var(--font-family-heading); font-size: var(--font-size-heading); color: var(--color-primary); line-height: 1; }
.mf-scenes__body { display: grid; gap: var(--space-2); min-width: 0; }
.mf-scenes__links { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-1) var(--space-3); align-items: center; font-size: var(--font-size-small); margin: 0; }
.mf-scenes__links dt { color: var(--color-text-muted); text-transform: uppercase; letter-spacing: 0.05em; }
.mf-scenes__links dd { margin: 0; display: flex; flex-wrap: wrap; gap: var(--space-1); }
.mf-scenes__map { display: flex; flex-wrap: wrap; gap: var(--space-2); align-items: center; justify-content: space-between; padding: var(--space-2) var(--space-3); background: var(--color-surface-2); border-radius: var(--radius-medium); border: var(--border-width) dashed var(--color-border); }
.mf-scenes__map.is-linked { border-style: solid; border-color: var(--color-success); }
`;
