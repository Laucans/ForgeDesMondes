// PreviousSummary (system session): the last closed session on one card —
// its number, title and date, its summary and where it came from, the
// entities it mentioned, the threads left open — and, behind « Journal
// complet », its transcribed journal with the time of each line in a side
// panel. It reads `session.getPreviousSessionSummary` and
// `session.getSessionJournal` by identifier, through the shell it is handed:
// no network call of its own, and nothing to trigger.

import { GET_PREVIOUS_SESSION_SUMMARY, GET_SESSION_JOURNAL } from './ids.ts';
import { heading, meta, parseJournal, parseSummary } from './model.ts';
import { lines, mentions, skeleton, threads } from './render.ts';
import { STYLE } from './style.ts';
import type { Shell, Summary } from './types.ts';

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="PreviousSummary"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'PreviousSummary';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the card and wires it. */
export function mount(host: HTMLElement, _props: Record<string, unknown>, shell: Shell): void {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`PreviousSummary: #${id} is not in the card`);
    return found;
  };

  let previous: Summary | null = null;
  /** The journal is read once, the first time the panel opens. */
  let journalRead = false;

  const paintSummary = (summary: Summary | null): void => {
    part('previous-title').textContent = summary === null ? 'Séance précédente' : heading(summary);
    part('previous-summary-text').textContent = summary === null ? 'Aucune séance précédente n’est encore close.' : summary.summary || 'Cette séance n’a pas de résumé.';
    part('previous-meta').textContent = summary === null ? '' : meta(summary);
    part('previous-mentions-list').innerHTML = summary === null ? '' : mentions(summary);
    part('previous-threads-list').innerHTML = summary === null ? '' : threads(summary);
    (part('previous-journal-open') as HTMLButtonElement).disabled = summary === null;
  };

  const readJournal = async (summary: Summary): Promise<void> => {
    journalRead = true;
    part('previous-journal-meta').textContent = 'Lecture du journal…';
    try {
      const journal = parseJournal((await shell.capability(GET_SESSION_JOURNAL, undefined, { session: summary.session.id })).data);
      part('previous-journal-meta').textContent = journal.journal.length === 0 ? 'Le journal de cette séance est vide.' : journal.journalStats;
      part('previous-journal-lines').innerHTML = lines(journal);
    } catch {
      journalRead = false;
      part('previous-journal-meta').textContent = 'Impossible de lire le journal.';
      part('previous-journal-lines').innerHTML = '';
    }
  };

  host.addEventListener('click', (event) => {
    const target = event.target as Element | null;
    if (target?.closest('#previous-journal-open')) {
      part('previous-journal').hidden = false;
      if (previous !== null && !journalRead) void readJournal(previous);
    } else if (target?.closest('#previous-journal-close')) part('previous-journal').hidden = true;
  });

  paintSummary(null);
  part('previous-summary-text').textContent = 'Lecture de la séance précédente…';
  shell
    .capability(GET_PREVIOUS_SESSION_SUMMARY, undefined)
    .then((answer) => {
      previous = parseSummary(answer.data);
      paintSummary(previous);
    })
    .catch(() => {
      part('previous-summary-text').textContent = 'Impossible de lire la séance précédente.';
    });
}
