#!/usr/bin/env bash
# Proves each check of scripts/design-system-gate.mjs (the `design-system` job
# of .github/workflows/gates.yml) on scratch copies of mock/ — no network,
# nothing outside a temp dir, the real mock/ is never written.
#
#   bash scripts/test-design-system-values-gate.sh
#
# DS_GATE points it at another copy of the gate. Exit status: 0 when every case behaves, 1 otherwise.
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
gate=${DS_GATE:-$root/scripts/design-system-gate.mjs}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0
check() { # name, expected, actual
  if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1 (expected $2, got $3)"; failures=$((failures + 1)); fi
}

# A fresh scratch tree: only mock/, copied from the real one.
scratch() { rm -rf "$work/tree"; mkdir "$work/tree"; cp -R "$root/mock" "$work/tree/mock"; cd "$work/tree" || exit 1; }
has() { if printf '%s\n' "$1" | grep -q -F -- "$2"; then echo 1; else echo 0; fi; }
# run: sets $out and $code.
run() { out=$(node "$gate" 2>&1); code=$?; }
# fails <name> <text the annotation must carry> [file:line]
fails() {
  run
  check "$1: exits non-zero" 1 "$code"
  check "$1: annotated" 1 "$(has "$out" "$2")"
  if [ -n "${3:-}" ]; then
    check "$1: names ${3}" 1 "$(has "$out" "::error file=${3%%:*},line=${3##*:}::")"
  fi
}
passes() { run; check "$1: exits 0" 0 "$code"; [ "$code" = 0 ] || printf '%s\n' "$out" | grep '^::error' | head -5; }
# seed <file> <text>: appends a line to a scratch file.
seed() { printf '%s\n' "$2" >> "$1"; }
last_line() { wc -l < "$1" | tr -d ' '; }

# --- the clean tree -----------------------------------------------------------
scratch; passes "clean tree"
pages=$(ls "$root"/mock/pages/*.html | wc -l | tr -d ' '); components=$(ls "$root"/mock/components/*.html | wc -l | tr -d ' ')
check "  ... reports pages, components and style.css" 1 "$(has "$out" "$pages pages, $components components,")"
check "  ... and other files" 1 "$(has "$out" "other files scanned under mock/ (mock/design/ and mock/kit/ excluded), mock/style.css included")"
check "  ... says apps/ is not scanned" 1 "$(printf '%s\n' "$out" | grep -c '^::notice::apps/ is not scanned')"
before=$(cd "$root" && find mock -type f -exec cksum {} + | sort | cksum)
(cd "$root" && node "$gate" > /dev/null 2>&1)
check "  ... writes nothing under mock/" "$before" "$(cd "$root" && find mock -type f -exec cksum {} + | sort | cksum)"

# --- nothing scanned is a failure ------------------------------------------------
scratch; rm -f mock/pages/*.html; fails "no pages" "no mock/pages/*.html found"
scratch; rm -f mock/components/*.html; fails "no components" "no mock/components/*.html found"
scratch; rm -f mock/style.css; fails "no style.css" "mock/style.css is missing"
scratch; rm -rf mock; fails "no mock/ at all" "no mock/pages/*.html found"

# --- colour literals ---------------------------------------------------------------
scratch; seed mock/style.css 'a { color: #fff; }'; n=$(last_line mock/style.css)
fails "hex in style.css" '"#fff"' "mock/style.css:$n"
scratch; seed mock/style.css 'a { color: #FFAA0080; }'; fails "8-digit hex, upper case" '"#FFAA0080"'
scratch; seed mock/style.css 'a { color: #abcd; }'; fails "4-digit hex" '"#abcd"'
scratch; seed mock/style.css 'a { color: #12345; }'; passes "5-digit hex is not a colour"
scratch; seed mock/style.css 'a { color: hsl(10 20% 30%); }'; fails "hsl()" 'hsl('
scratch; seed mock/style.css 'a { border-color: oklch(70% .1 40); }'; fails "oklch()" 'oklch('
scratch; seed mock/style.css 'a { border: 1px solid red; }'; fails "named colour in border" '"red"'
scratch; seed mock/style.css 'a { color: var(--color-text, red); }'; fails "named colour as var() fallback" '"red"'
scratch; seed mock/style.css 'a { box-shadow: 0 0 4px rebeccapurple; }'; fails "named colour in a shadow" '"rebeccapurple"'
scratch; seed mock/style.css 'a { background: url(#fade) no-repeat; transition: color .2s; color: currentColor; border-color: transparent; outline: none; fill: inherit; }'
passes "allowed values and url(#fragment)"
scratch; seed mock/style.css 'a { animation-name: tan; content: "red"; }'; passes "a name outside a colour property"
scratch
printf '%s\n' '<style>' '.x { background: rgba(0,0,0,.5); }' '</style>' >> mock/components/SessionList.html
n=$(grep -n 'rgba(0,0,0' mock/components/SessionList.html | cut -d: -f1)
fails "rgba() in a <style> block" 'rgba(' "mock/components/SessionList.html:$n"
scratch; seed mock/components/SessionList.html '<p style="color: red">x</p>'; n=$(last_line mock/components/SessionList.html)
fails "inline style on a component" '"red"' "mock/components/SessionList.html:$n"
scratch; seed mock/components/SessionList.html '<svg><path fill="#ff0000"/></svg>'; fails "SVG fill attribute" '"#ff0000"'
scratch; seed mock/mock.js "document.body.style.setProperty('--x', '#abc');"; n=$(last_line mock/mock.js)
fails "setProperty in mock.js" '"#abc"' "mock/mock.js:$n"
scratch; seed mock/mock.js "document.body.style.color = 'blue';"; fails "style.color in mock.js" '"blue"'
scratch; seed mock/mock.js "const T = { bg: '#12ab34' };"; fails "colour in a JS string" '"#12ab34"'
scratch; seed mock/mock.js "const el = \`<i style=\"background:#fff\"></i>\`;"; fails "style attribute in a JS template" '"#fff"'
scratch; seed mock/mock.js "document.querySelector('#add'); // #fff"; passes "selector string and comment in JS"
scratch; seed mock/mock.js "document.body.style.color = ok ? 'red' : 'blue';"; fails "named colour in a ternary" '"red"'
scratch; seed mock/style.css ':root { --panel: white; }'; fails "named colour in a custom property" '"white"'
scratch; seed mock/style.css 'a { filter: drop-shadow(0 0 2px red); }'; fails "named colour in a filter" '"red"'
scratch; printf '%s\n' '<svg xmlns="http://www.w3.org/2000/svg"><rect fill="#ff0000"/></svg>' > mock/fixtures/logo.svg; fails "an .svg file is scanned" '"#ff0000"'
scratch; mkdir mock/components/sub; printf '%s\n' '<link rel="stylesheet" href="../../style.css">' > mock/components/sub/X.html; fails "a nested component is a component" '<link in a micro-frontend'
scratch; mkdir mock/pages/sub; sed 's#"\.\./#"../../#' mock/pages/campagne.html > mock/pages/sub/p.html; passes "a nested page links the design system two levels up"
scratch; seed mock/style.css 'a { background: url(https://example.com/a.png); }'; passes "a remote image is not a stylesheet or a font"
scratch; seed mock/style.css '@font-face { src: url(https://fonts.googleapis.com/css2?family=Inter); }'; fails "a remote address with no extension" 'url() of a stylesheet or a font'
scratch; seed mock/fixtures/entities.json '{"colour": "#ff0000"}'; fails "colour in a fixture" '"#ff0000"'
scratch; seed mock/pages/campagne.html '<div style="color: var(--color-accent, #abc)"></div>'; n=$(last_line mock/pages/campagne.html)
fails "literal fallback in a page" '"#abc"' "mock/pages/campagne.html:$n"
scratch; seed mock/pages/campagne.html '<p>var(--color-accent, #abc)</p>'; fails "literal fallback in a page's markup" '"#abc"'
scratch
seed mock/style.css 'a { color: #fff; }'; seed mock/mock.js "x.style.setProperty('--x', '#abc');"; seed mock/pages/medias.html '<i style="color: red"></i>'
run
check "everything is reported before the verdict" 3 "$(printf '%s\n' "$out" | grep -c '^::error file=')"
check "  ... exit non-zero" 1 "$code"
scratch; seed mock/pages/campagne.html '<a href="#abc">x</a><section id="player-scene"></section>'; seed mock/style.css '#site-header, #player-scene { margin: 0; }'
passes "id selectors and href anchors"

# --- the design system's own files are never judged --------------------------------
scratch; seed mock/kit/kit.css '.x { color: var(--color-accent-cyan, #27d7f0); }'; seed mock/design/tokens.css ':root { --color-extra: #123456; }'
seed mock/kit/Button.html '<button style="color: red">x</button><link rel="stylesheet" href="x.css">'; passes "literals in design/ and kit/"
scratch; cp mock/design/tokens.css mock/pages/tokens.css; n=$(grep -n -m1 '#[0-9a-f]\{3,8\};' mock/pages/tokens.css | cut -d: -f1)
fails "a tokens.css outside design/ is scanned" '"#' "mock/pages/tokens.css:$n"
scratch; cp mock/kit/kit.css mock/pages/kit.css; run; check "a kit.css outside kit/ is scanned" 1 "$([ "$code" = 1 ] && echo 1 || echo 0)"

# --- unknown tokens -------------------------------------------------------------------
scratch; seed mock/style.css 'a { margin: var(--space-99); }'; n=$(last_line mock/style.css)
fails "unknown spacing token" '--space-99' "mock/style.css:$n"
scratch; seed mock/style.css 'a { color: var(--color-nope); }'; fails "unknown colour token" '--color-nope'
scratch; seed mock/style.css 'a { width: var(--kit-token-hp); margin: var(--space-3); gap: var(--border-width); }'; passes "kit tokens and known tokens"
scratch; seed mock/mock.js "el.style.setProperty('--hm-grid', 'var(--color-text)');"; passes "dynamic custom property in JS"
scratch; seed mock/mock.js "el.style.width = 'var(--space-77)';"; fails "unknown token in JS" '--space-77'

# --- the stylesheet links ----------------------------------------------------------------
page=mock/pages/campagne.html
l1=$(grep -n 'href="../design/tokens.css"' "$root/$page" | cut -d: -f1); l2=$((l1 + 1)); l3=$((l1 + 2))
scratch; passes "pages and index.html with the triple"
scratch; sed -i.bak 's#\.\./design/tokens.css#./tokens.css#' $page; rm $page.bak; cp mock/design/tokens.css mock/pages/tokens.css
fails "a copied tokens.css linked by a page" 'href="./tokens.css"' "$page:$l1"
scratch; sed -i.bak 's#\.\./kit/kit.css#../style/kit.css#' $page; rm $page.bak; fails "a variant path" '../style/kit.css' "$page:$l2"
scratch; sed -i.bak 's#\.\./design/tokens.css#https://cdn.example.com/tokens.css#' $page; rm $page.bak; fails "an absolute URL" 'https://cdn.example.com/tokens.css' "$page:$l1"
scratch; sed -i.bak -e 's#\.\./design/tokens.css#TMP#' -e 's#\.\./kit/kit.css#../design/tokens.css#' -e 's#TMP#../kit/kit.css#' $page; rm $page.bak
fails "swapped order" 'link 1 is' "$page:$l1"
scratch; sed -i.bak "${l2}d" $page; rm $page.bak; fails "a link missing" '2 stylesheet link(s) found, expected 3'
scratch; sed -i.bak "${l2}s#.*#  <link rel=\"stylesheet\" href=\"../design/tokens.css\">#" $page; rm $page.bak; fails "a link duplicated" 'link 2 is' "$page:$l2"
scratch; sed -i.bak "${l3}a\\
  <link rel=\"icon\" href=\"../style.css\">" $page; rm $page.bak; fails "a fourth link" 'a 4th <link>' "$page:$((l3 + 1))"
scratch; sed -i.bak "${l2}s#.*#  <link rel=\"stylesheet\" href=\"../kit/kit.css\"><link rel=\"stylesheet\" href=\"../x.css\">#" $page; rm $page.bak; run
check "two links on one line fail" 1 "$code"
scratch; rm mock/kit/kit.css; fails "an href that does not resolve (kit.css)" 'does not resolve' "$page:$l2"
scratch; sed -i.bak 's#"style.css"#"../style.css"#; s#href="style.css"#href="../style.css"#' mock/index.html; rm mock/index.html.bak; i3=$(grep -n 'href="../style.css"' mock/index.html | cut -d: -f1); fails "index.html with a page's triple" 'link 3 is' "mock/index.html:$i3"
scratch; sed -i.bak 's#href="kit/kit.css"#href="../kit/kit.css"#' mock/index.html; rm mock/index.html.bak; i2=$(grep -n 'href="../kit/kit.css"' mock/index.html | cut -d: -f1); fails "index.html with a page's path" 'link 2 is' "mock/index.html:$i2"

# --- components load nothing ----------------------------------------------------------------
scratch; passes "components with no <link>"
scratch; seed mock/components/SessionList.html '<link rel="stylesheet" href="../style.css">'; n=$(last_line mock/components/SessionList.html)
fails "a <link in a component" '<link in a micro-frontend' "mock/components/SessionList.html:$n"
scratch; seed mock/components/SessionList.html '<style>@import "../style.css";</style>'; fails "@import in a component" '@import'

# --- imports and external loads ---------------------------------------------------------------
scratch; seed mock/style.css '@import url("fonts.css");'; n=$(last_line mock/style.css); fails "@import in style.css" '@import' "mock/style.css:$n"
scratch; seed mock/style.css '@font-face { font-family: X; src: url(https://fonts.example.com/x.woff2); }'; fails "external font in style.css" 'url() of a stylesheet or a font'
scratch; seed mock/style.css '@font-face { font-family: X; src: url(fonts/x.woff2); }'; fails "a local font file is still a font of our own" 'url() of a stylesheet or a font'
scratch; seed mock/style.css 'a { background: url(data:image/png;base64,AAAA) no-repeat; }'; passes "a data: url()"
scratch; printf '%s\n' '<style>' '@import url(https://x.example/a.css);' '</style>' >> mock/pages/medias.html; fails "@import in a page's <style> block" '@import'
scratch; printf '%s\n' '<style>' '.f { background: url(//cdn.example/a.css); }' '</style>' >> mock/components/SessionList.html; fails "url() of a stylesheet or a font in a <style> block" 'url() of a stylesheet or a font'
scratch; seed mock/mock.js "const css = '@import url(a.css)';"; fails "@import in JS" '@import'
scratch; seed mock/mock.js "el.style.backgroundImage = 'url(https://x.example/a.css)';"; fails "url() of a stylesheet or a font in JS" 'url() of a stylesheet or a font'
scratch; seed mock/mock.js "head.innerHTML = '<link rel=\"stylesheet\" href=\"a.css\">';"; fails "<link in JS" '<link'

# --- font, spacing, border, radius and shadow values -------------------------------
# Rules 13-15: a token, a calc() over tokens, or zero. Each seed fails alone.
scratch; seed mock/style.css 'a { margin: 4px; }'; n=$(last_line mock/style.css)
fails "margin: 4px" 'literal spacing "4px"' "mock/style.css:$n"
scratch; seed mock/style.css 'a { margin: var(--space-2) 4px; }'; fails "a literal beside a token" '"4px"'
scratch; seed mock/style.css 'a { padding: calc(var(--space-2) + 3px); }'; fails "a mixed calc()" 'literal spacing "3px"'
scratch; seed mock/style.css 'a { padding-inline-start: 2rem; margin-block: 1em; }'; fails "logical and sided margins" '"2rem"'
scratch; seed mock/style.css 'a { gap: 1rem; }'; fails "gap: 1rem" 'literal spacing "1rem"'
scratch; seed mock/style.css 'a { row-gap: 3px; }'; fails "row-gap" '"3px"'
scratch; seed mock/style.css 'a { font-size: 14px; }'; fails "font-size: 14px" 'literal font size "14px"'
scratch; seed mock/style.css 'a { font-size: larger; }'; fails "a font size keyword" 'literal font size "larger"'
scratch; seed mock/style.css 'a { font-family: Arial; }'; fails "font-family: Arial" 'literal font family "Arial"'
scratch; seed mock/style.css "a { font-family: 'Open Sans', sans-serif; }"; fails "a family list" 'literal font family'
scratch; seed mock/style.css 'a { font-weight: 700; }'; fails "font-weight: 700" 'literal font weight "700"'
scratch; seed mock/style.css 'a { font-weight: bold; }'; fails "font-weight: bold" 'literal font weight "bold"'
scratch; seed mock/style.css 'a { font: bold 14px Arial; }'; fails "the font shorthand" 'literal font'
scratch; seed mock/style.css 'a { border-radius: 6px; }'; fails "border-radius: 6px" 'literal radius "6px"'
scratch; seed mock/style.css 'a { border-top-left-radius: calc(var(--radius-small) + 2px); }'; fails "a corner radius" '"2px"'
scratch; seed mock/style.css 'a { box-shadow: 0 1px 2px var(--color-text); }'; fails "box-shadow" 'literal shadow "1px"'
scratch; seed mock/style.css 'a { text-shadow: 1px 1px var(--color-text); }'; fails "text-shadow" 'literal shadow'
scratch; seed mock/style.css 'a { filter: drop-shadow(0 0 2px var(--color-text)); }'; fails "drop-shadow()" 'literal shadow "2px"'
scratch; seed mock/style.css 'a { border: 2px solid var(--color-border); }'; fails "the width inside border" 'literal border width "2px"'
scratch; seed mock/style.css 'a { border-bottom-width: 3px; outline: 2px solid var(--color-accent); }'; fails "border-*-width and outline" '"3px"'
scratch; seed mock/style.css 'a { border: thick solid var(--color-border); }'; fails "a width keyword" 'literal border width "thick"'
scratch; seed mock/style.css 'a { outline-offset: 1px; }'; fails "outline-offset" 'literal outline offset "1px"'
scratch; seed mock/style.css 'a { left: 12px; }'; fails "a literal offset in a stylesheet" 'literal offset "12px"'
scratch; seed mock/style.css 'a { margin: var(--space-2, 4px); }'; fails "a literal var() fallback" 'literal spacing "4px"'
scratch; seed mock/style.css 'a { box-shadow: 0 0 var(--space-1, 3px) var(--color-text); }'; fails "a literal fallback in a shadow" '"3px"'
scratch; seed mock/style.css 'a { font: var(--font-size-small) Arial; }'; fails "a family in the font shorthand" 'literal font family "Arial"'
scratch; seed mock/style.css 'a { grid-gap: 8px; scroll-margin: 8px; -webkit-box-shadow: 0 1px 2px var(--color-text); }'; fails "aliases and prefixes" '"8px"'
scratch; seed mock/style.css 'a { padding: calc(var(--space-2) * 2) 7 var(--space-1); }'; fails "a bare number after a closed calc()" '"7"'
scratch; seed mock/style.css 'a { left: calc(var(--space-2) + 4px); inset: 4px; }'; fails "inset and a mixed calc() offset" 'literal offset'
# ... wherever a declaration is written
scratch; printf '%s\n' '<style>' '.x { margin: 4px; }' '</style>' >> mock/components/SessionList.html
n=$(grep -n 'margin: 4px' mock/components/SessionList.html | cut -d: -f1)
fails "a <style> block" 'literal spacing "4px"' "mock/components/SessionList.html:$n"
scratch; seed mock/pages/campagne.html '<p style="padding: 4px">x</p>'; n=$(last_line mock/pages/campagne.html)
fails "an inline style" '"4px"' "mock/pages/campagne.html:$n"
scratch; seed mock/mock.js "document.body.style.margin = '4px';"; n=$(last_line mock/mock.js)
fails "a JS style write" 'literal spacing "4px"' "mock/mock.js:$n"
scratch; seed mock/mock.js "document.body.style.setProperty('font-weight', 'bold');"; fails "setProperty" 'literal font weight "bold"'
scratch; seed mock/mock.js 'el.innerHTML = `<i style="gap: 1rem"></i>`;'; fails "a style attribute in a JS template" '"1rem"'
scratch; seed mock/mock.js "el.style.fontSize = '14px';"; fails "style.fontSize" '"14px"'
# ... inside @media and @container, the declarations are judged
scratch; seed mock/style.css '@media (max-width: 500px) { a { margin: 4px; } }'; n=$(last_line mock/style.css)
fails "a literal inside @media" 'literal spacing "4px"' "mock/style.css:$n"
scratch; seed mock/style.css '@container (min-width: 20rem) { a { font-size: 14px; } }'; fails "a literal inside @container" '"14px"'
scratch; printf '%s\n' '<style>' '@media (max-width: 500px) {' '.x { padding: 3px; }' '}' '</style>' >> mock/components/SessionList.html
fails "a literal inside @media in a <style> block" '"3px"'
# The values that pass (rules 14, 16, 18)
scratch; seed mock/style.css 'a { margin: 0 auto; padding: 0px; gap: 0; border: 0; outline: none; border-width: 0; box-shadow: none; text-shadow: none; outline-offset: 0; border-radius: 0; }'
passes "zero, auto and none"
scratch; seed mock/style.css 'a { margin: var(--space-2); padding: calc(var(--space-2) * 2) var(--space-3); gap: calc(var(--space-1) / 2); border: calc(var(--border-width) * 2) solid var(--color-border); outline-offset: var(--border-width); border-radius: var(--radius-small); box-shadow: var(--shadow-low); text-shadow: 0 0 var(--space-1) var(--color-text); filter: drop-shadow(0 0 var(--space-1) var(--color-text)); }'
passes "tokens and a calc() over tokens"
scratch; seed mock/style.css 'a { font-weight: var(--font-weight-heading); font-weight: normal; font-weight: inherit; font-family: var(--font-family); font-family: inherit; font-size: var(--font-size-small); font-size: inherit; font: inherit; }'
passes "token, normal and inherit for fonts"
scratch; seed mock/style.css 'a { letter-spacing: .05em; line-height: 1.4; opacity: .5; z-index: 3; stroke-width: 2; border-radius: 50%; border-radius: 50% 50% 0 0; width: 120px; height: 12px; min-width: 3px; max-width: 40rem; min-height: 3px; max-height: 9px; aspect-ratio: 16 / 9; left: 50%; transform: translate(4px, 4px); display: none; }'
passes "shapes have no token"
scratch; seed mock/style.css 'a { border: var(--border-width) solid color-mix(in srgb, var(--color-primary) 40%, transparent); box-shadow: 0 0 var(--space-1) color-mix(in srgb, var(--color-text) 30%, transparent); font: var(--font-size-small)/1.4 var(--font-family); margin: min(var(--space-2), var(--space-3)); padding: max(var(--space-1), 0px); }'
passes "colour mixing, line-height in font and min()/max()"
scratch; seed mock/style.css '@media (min-width: 761px) and (max-width: 1100px) { a { margin: var(--space-1); } } @container card (min-width: 400px) { a { gap: 0; } }'
passes "@media and @container preludes are left alone"
scratch; seed mock/style.css 'a { --hm-cell: 4px; }'; passes "a custom property is not a design value"
# Rule 17: JS-computed placement, in four files only. Never colour.
placement=$(cat <<'JS'
tip.style.left = (w.lx + 16) + 'px'; tip.style.top = (w.ly + 16) + 'px'; tip.style.right = w + 'px'; tip.style.bottom = w + 'px';
win.style.width = (w.w * W) + 'px'; win.style.height = h + 'px'; stage.style.transform = `translate(${x}px,${y}px) scale(${z})`;
win.style.aspectRatio = i.ratio; win.style.display = 'none'; root.style.setProperty('--kit-token-hp', pct + '%');
el.style.left = '12px'; el.style.top = '4px'; el.style.setProperty('left', '8px');
el.innerHTML = `<div style="left: 12px; top: 4px; width: ${w}px; --kit-token-hp: ${pct}%"></div>`;
JS
)
for f in mock/mock.js mock/components/PlayerStage.html mock/components/Battlemap.html mock/components/EntityEditor.html; do
  scratch; if [ "${f##*.}" = js ]; then printf '%s\n' "$placement" >> "$f"; else printf '%s\n' '<script>' "$placement" '</script>' >> "$f"; fi
  passes "placement writes in $f"
  if [ "${f##*.}" = js ]; then seed "$f" "el.style.margin = '4px';"; else printf '%s\n' '<script>' "el.style.margin = '4px';" '</script>' >> "$f"; fi
  fails "  ... a spacing in $f is still judged" '"4px"'
  scratch; if [ "${f##*.}" = js ]; then seed "$f" "el.style.color = 'red';"; else printf '%s\n' '<script>' "el.style.color = 'red';" '</script>' >> "$f"; fi
  fails "  ... colour is never exempt in $f" 'colour literal "red"'
done
scratch; printf '%s\n' '<script>' "el.style.left = '12px';" '</script>' >> mock/components/SessionList.html
fails "a literal offset in another component" 'literal offset "12px"'
scratch; seed mock/pages/campagne.html '<i style="left: 12px"></i>'; fails "an inline offset in a page" 'literal offset "12px"'
scratch; printf '%s\n' '<style>' '.x { left: 12px; top: 4px; }' '</style>' >> mock/components/Battlemap.html
fails "a <style> block of a placement file is judged" 'literal offset "12px"'
scratch; seed mock/mock.js "el.style.color = 'red'; el.style.left = '0';"; fails "colour in a placement file" 'colour literal "red"'

# --- the job around the script ---------------------------------------------------------------
job=$(awk '/^  design-system:/ {j=1; print; next} j && /^  [a-z#]/ {exit} j' "$root/.github/workflows/gates.yml")
check "gates.yml has a design-system job" 1 "$(printf '%s\n' "$job" | grep -c '^  design-system:')"
check "  ... on ubuntu-latest" 1 "$(printf '%s\n' "$job" | grep -c 'runs-on: ubuntu-latest')"
check "  ... node 22" 1 "$(printf '%s\n' "$job" | grep -c 'node-version: 22')"
check "  ... runs the script" 1 "$(printf '%s\n' "$job" | grep -c 'run: node scripts/design-system-gate.mjs')"
check "  ... no if:, no paths:" 0 "$(printf '%s\n' "$job" | grep -c '^\s*if:\|paths:')"

[ "$failures" -eq 0 ] && echo "all cases behave" || echo "$failures case(s) misbehave"
[ "$failures" -eq 0 ]
