// The panel's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the mock-up's `.mf-ep` rules (mock/components/
// ElementsPanel.html) and the page's `.chip`, scoped to the panel, every
// value a token. Nothing is given a width: the column decides, long names
// wrap (`overflow-wrap`), the chips wrap under them (`flex-wrap`), and
// nothing overflows or is clipped horizontally (mock-up pin 1 on #dm-elements).

export const STYLE = `
.mf-ep { box-sizing: border-box; min-width: 0; max-width: 100%; display: grid; gap: var(--space-3); align-content: start; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-ep *, .mf-ep *::before, .mf-ep *::after { box-sizing: inherit; }
.mf-ep > * { min-width: 0; }
.mf-ep h2, .mf-ep h3, .mf-ep p { margin: 0; }
.mf-ep h2 { font-size: var(--font-size-base); }
.mf-ep .kit-input { max-width: none; }
.mf-ep .kit-input input { min-width: 0; width: 100%; }
.mf-ep details { min-width: 0; border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); background: var(--color-surface); margin-top: var(--space-2); }
.mf-ep details.is-current { border-color: var(--color-primary); }
.mf-ep summary { cursor: pointer; padding: var(--space-2) var(--space-3); font-weight: var(--font-weight-heading); overflow-wrap: anywhere; }
.mf-ep summary small { display: block; font-weight: normal; color: var(--color-text-muted); }
.mf-ep__group { min-width: 0; padding: 0 var(--space-3) var(--space-2); }
.mf-ep__group h3 { font-size: var(--font-size-small); color: var(--color-text-muted); margin: var(--space-2) 0 var(--space-1); }
.mf-ep__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); min-width: 0; }
.mf-ep__item { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-1); min-width: 0; padding: var(--space-1) var(--space-2); border-radius: var(--radius-small); }
.mf-ep__item:hover { background: var(--color-surface-2); }
.mf-ep__item[draggable="true"] { cursor: grab; }
.mf-ep__open { flex: 1; min-width: 0; text-align: left; background: none; border: 0; padding: 0; color: var(--color-text); font: inherit; cursor: pointer; white-space: normal; overflow-wrap: anywhere; }
.mf-ep__open:hover { color: var(--color-accent); }
.mf-ep__open:disabled { cursor: default; color: var(--color-text-muted); }
.mf-ep__open small { color: var(--color-text-muted); margin-left: var(--space-1); }
.mf-ep .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: 0 var(--space-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; flex: 0 0 auto; white-space: nowrap; }
.mf-ep .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-ep__grip { color: var(--color-text-muted); }
.mf-ep__empty { color: var(--color-text-muted); font-size: var(--font-size-small); }
.mf-ep__status { padding: var(--space-1) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); font-size: var(--font-size-small); overflow-wrap: anywhere; }
.mf-ep__status.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-ep [hidden] { display: none; }
`;
