# llm port — the Claude Code adapter, against a real `claude`

## Context

Task #704. The `llm` port's real adapter is now the DM's own `claude` program,
run as `claude -p --model claude-haiku-5-5 --effort medium` with the prompt on
its standard input, `LLM_API_KEY` removed from its environment, under the DM's
Claude subscription. It replaces the Anthropic API adapter and the opencode
adapters (crate `ports-llm-claude-code`).

What was run: the port's contract suite and the argument, standard-input,
environment and failure checks, all against **stub** `claude` scripts. The
server still serves the fake for `PORTS=fake` and `PORTS=local`: nothing wires
this adapter into it yet.

## Not exercised

- A real `claude -p --model claude-haiku-5-5 --effort medium` call under the
  DM's subscription: that it answers, that its answer is the text asked for
  (a JSON object for notes and briefs), and that no per-token charge appears.
- Whether the installed CLI accepts `--effort`: the stubs accept anything.
- That `-p` with no `--tools ""` never runs a tool: print mode is expected to
  deny what needs a permission, nothing here shows it.

## Checklist

- [ ] `claude --help` lists `--effort` and accepts `medium`; note the version.
- [ ] `echo 'Réponds par le mot : ok' | env -u LLM_API_KEY claude -p --model claude-haiku-5-5 --effort medium`
      prints an answer, logged in under the subscription, no API key set.
- [ ] A long prompt (a whole session's journal) on stdin comes back answered
      within the 180 s the adapter allows.
- [ ] Logged out, the failure the DM would read is « le programme a échoué
      (code …) : … » with what `claude` says.
