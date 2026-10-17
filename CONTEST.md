# Contested: `scripts/rules-runtime.sh serve` does not return when its output is a pipe

**The rule.** The rules' runtime (`scripts/rules-runtime.sh`, contract P):
`serve` starts the server, then the shell when `apps/shell/package.json`
exists, prints `RULES_SERVER_URL`, `RULES_APP_URL` and `RULES_SCREEN_ROUTE`,
and exits 0 — "in the background, their output in target/rules-runtime/".

**The test.** The `rules-hold` gate of `.github/workflows/gates.yml`:

```
bash scripts/rules-runtime.sh serve | tee rules-env
cat rules-env >> "$GITHUB_ENV"
```

**Why it cannot hold once `apps/shell` exists.** In `serve()` the shell is
started by

```
(cd "$root/apps/shell" && PORT=… SERVER_URL=… nohup npm run serve >"$state/shell.log" 2>&1 & echo $! > "$state/shell.pid")
```

`A && B &` backgrounds the whole list `A && B` as a subshell. `B` has its
output redirected, but the subshell itself keeps the script's standard
output — the pipe to `tee` — for as long as `npm run serve` lives, that is
until `stop`. `tee` never reads end-of-file, so the step never ends and
`RULES_*` never reach `$GITHUB_ENV`. (The server's own leg is a simple
command with its redirects, `PORT=… nohup "$binary" >log 2>&1 &`: it holds
nothing.)

Observed on this branch with the shell of #33, built and serving:

- `bash scripts/rules-runtime.sh serve > file` prints the three `RULES_*`
  lines and exits 0 at once; the journey tests then report all 22 rules
  pending, none failing; `stop` leaves no shell and no server.
- `bash scripts/rules-runtime.sh serve | tail` printed nothing and returned
  only after `stop` had killed the shell.
- In isolation: `bash -c '(cd /tmp && sleep 4 >/dev/null 2>&1 & echo $! > p)' | cat`
  takes 4 s; with `> file` instead of the pipe it takes 0 s.

Nothing in `apps/shell` can close a descriptor held by the script's own
subshell, and a shell that detaches itself would escape `stop`
(`pkill -P` / `kill` of the recorded pid), so the product does not work
around it.

**What would settle it** (the rules' task, which owns the script): give the
list its own redirection, e.g. `(cd "$root/apps/shell" && …) >/dev/null 2>&1 &`
followed by `echo $! > "$state/shell.pid"`, or `nohup npm --prefix
"$root/apps/shell" run serve >"$state/shell.log" 2>&1 &` as a simple
command. No test and no rule changes; the shell of #33 needs nothing more.
