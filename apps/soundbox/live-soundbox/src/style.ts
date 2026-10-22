// The panel's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the helpers the mock-up's pages share and the
// fragment's `.mf-live-sb` rules, scoped to the panel, every value a token.

export const STYLE = `
.mf-live-sb { box-sizing: border-box; display: grid; gap: var(--space-3); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-live-sb *, .mf-live-sb *::before, .mf-live-sb *::after { box-sizing: inherit; }
.mf-live-sb h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; font-size: var(--font-size-large); color: var(--color-accent); }
.mf-live-sb p { margin: 0; }
.mf-live-sb .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-live-sb .muted { color: var(--color-text-muted); }
.mf-live-sb .small { font-size: var(--font-size-small); }
.mf-live-sb .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-live-sb .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-live-sb .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-live-sb .chip.is-on { background: var(--color-primary); border-color: var(--color-primary); color: var(--color-primary-contrast); font-weight: var(--font-weight-heading); }
.mf-live-sb .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-live-sb .kit-slider.kit-slider--bare { padding: 0; background: transparent; border: 0; box-shadow: none; max-width: none; }
.mf-live-sb .kit-input.is-wide { max-width: none; }
.mf-live-sb__bar { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 2fr) auto; gap: var(--space-3); align-items: end; }
.mf-live-sb__bar .kit-input { max-width: none; }
.mf-live-sb__message:empty { display: none; }
.mf-live-sb__message { color: var(--color-danger); }
.mf-live-sb__grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 4.5), 1fr)); gap: var(--space-3); }
.mf-live-sb__box { position: relative; display: grid; gap: var(--space-2); padding: var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); overflow: hidden; }
.mf-live-sb__box.is-playing { border-color: var(--color-success); box-shadow: var(--shadow-high); }
.mf-live-sb__play { display: grid; gap: var(--space-1); text-align: left; background: none; border: 0; padding: 0; color: var(--color-text); font: inherit; cursor: pointer; }
.mf-live-sb__play b { font-size: var(--font-size-base); }
.mf-live-sb__play small { color: var(--color-text-muted); }
.mf-live-sb__box.is-playing .mf-live-sb__play b::before { content: "▶ "; color: var(--color-success); }
.mf-live-sb__vol { display: flex; align-items: center; gap: var(--space-2); font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-live-sb__vol input { flex: 1; accent-color: var(--color-primary); }
.mf-live-sb__progress { position: absolute; left: 0; bottom: 0; height: var(--space-1); width: 0; background: var(--color-success); }
@media (max-width: 900px) { .mf-live-sb__bar { grid-template-columns: minmax(0, 1fr); } }
`;
