//! The vocabulary a campaign's microphone listens for, and how a heard phrase
//! names one of its terms: DetectionVocabulary@1, with AliasMatch@1's
//! normalisation, kept local to this unit (rule 3) and held to the Concept's
//! fixtures by a test of its own.
//!
//! The campaign's rows come from [`crate::VOCABULARY`], the entities' names
//! then their aliases, then the sounds' names, in the order the Concept
//! asks; [`Vocabulary::from_rows`] leaves out what names nothing, and
//! [`Vocabulary::detect`] reads a phrase against what is left.

use std::collections::BTreeSet;

use ports_speech_to_text::Vocabulary as Heard;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

/// What a term names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// An element of the campaign.
    Entity,
    /// A sound of the campaign.
    Sound,
}

impl Kind {
    /// The kind a row names; `None` for any other text.
    fn of(text: &str) -> Option<Self> {
        match text {
            "entity" => Some(Self::Entity),
            "sound" => Some(Self::Sound),
            _ => None,
        }
    }
}

/// One entry of the vocabulary: a term as written, and its target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    /// The name or alias, exactly as written.
    pub term: String,
    /// What it names.
    pub kind: Kind,
    /// The id of the element or of the sound.
    pub id: String,
    /// Whether it is an alias of an element rather than a name.
    pub alias: bool,
    /// The term's normalised words.
    words: Vec<String>,
    /// The characters of its words joined by single spaces: the length of a
    /// candidate it makes.
    length: usize,
}

/// A target a phrase names, and the term of the vocabulary that named it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    /// What was named.
    pub kind: Kind,
    /// The id of the element or of the sound.
    pub id: String,
    /// The term as written that named it.
    pub via: String,
}

/// The vocabulary of a campaign, in the Concept's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vocabulary {
    terms: Vec<Term>,
}

/// AliasMatch@1: trimmed, lowercase, accents (U+0300 to U+036F) removed from
/// the decomposed letter, recomposed.
fn normalised(term: &str) -> String {
    term.trim()
        .to_lowercase()
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect()
}

/// The normalised words of `text`: its maximal runs of letters, digits and
/// combining marks.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || ('\u{300}'..='\u{36f}').contains(&c)))
        .filter(|word| !word.is_empty())
        .map(normalised)
        .collect()
}

/// A candidate: where an entry occurs in the phrase.
struct Candidate {
    start: usize,
    length: usize,
    entry: usize,
}

impl Vocabulary {
    /// The vocabulary the `rows` of [`crate::VOCABULARY`] make — `term`,
    /// `kind` (`entity` or `sound`), `id` and `alias` — in their order. A
    /// blank term names nothing, and a term that matches (AliasMatch@1) an
    /// earlier term of the same target is left out: the first spelling stays.
    #[must_use]
    pub fn from_rows(rows: &Value) -> Self {
        let mut terms: Vec<Term> = Vec::new();
        let mut seen: BTreeSet<(Kind, String, String)> = BTreeSet::new();
        for row in rows.as_array().map(Vec::as_slice).unwrap_or_default() {
            let text = |key: &str| row.get(key).and_then(Value::as_str);
            let (Some(term), Some(kind), Some(id)) =
                (text("term"), text("kind").and_then(Kind::of), text("id"))
            else {
                continue;
            };
            let own = normalised(term);
            if own.is_empty() || !seen.insert((kind, id.to_string(), own)) {
                continue;
            }
            let words = words(term);
            terms.push(Term {
                term: term.to_string(),
                kind,
                id: id.to_string(),
                alias: row.get("alias") == Some(&Value::Bool(true)),
                length: words.join(" ").chars().count(),
                words,
            });
        }
        Self { terms }
    }

    /// The entries, in order.
    #[must_use]
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    /// What the engine of the speech-to-text port listens for: the elements'
    /// names, their aliases, the sounds' names — each term once, as written,
    /// a term with no word (only marks and punctuation) to hear nothing of.
    #[must_use]
    pub fn to_hear(&self) -> Heard {
        let of = |wanted: fn(&Term) -> bool| {
            let mut seen = BTreeSet::new();
            self.terms
                .iter()
                .filter(|t| wanted(t) && !t.words.is_empty() && seen.insert(t.term.clone()))
                .map(|t| t.term.clone())
                .collect()
        };
        Heard {
            campaign_names: of(|t| t.kind == Kind::Entity && !t.alias),
            aliases: of(|t| t.kind == Kind::Entity && t.alias),
            sound_names: of(|t| t.kind == Kind::Sound),
        }
    }

    /// The targets `phrase` names, in the order of the phrase: each by its
    /// longest term, never twice, and no two sharing a word of the phrase.
    #[must_use]
    pub fn detect(&self, phrase: &str) -> Vec<Detection> {
        let heard = words(phrase);
        let mut candidates: Vec<Candidate> = Vec::new();
        for (entry, term) in self.terms.iter().enumerate() {
            let n = term.words.len();
            if n == 0 || n > heard.len() {
                continue;
            }
            for start in 0..=heard.len() - n {
                if heard[start..start + n] == term.words[..] {
                    candidates.push(Candidate {
                        start,
                        length: term.length,
                        entry,
                    });
                }
            }
        }
        candidates.sort_by_key(|c| (std::cmp::Reverse(c.length), c.start, c.entry));
        let mut used: BTreeSet<usize> = BTreeSet::new();
        let mut named: BTreeSet<(Kind, &str)> = BTreeSet::new();
        let mut kept: Vec<&Candidate> = Vec::new();
        for candidate in &candidates {
            let term = &self.terms[candidate.entry];
            let span = candidate.start..candidate.start + term.words.len();
            if span.clone().any(|word| used.contains(&word))
                || named.contains(&(term.kind, term.id.as_str()))
            {
                continue;
            }
            used.extend(span);
            named.insert((term.kind, term.id.as_str()));
            kept.push(candidate);
        }
        kept.sort_by_key(|c| c.start);
        kept.iter()
            .map(|c| {
                let term = &self.terms[c.entry];
                Detection {
                    kind: term.kind,
                    id: term.id.clone(),
                    via: term.term.clone(),
                }
            })
            .collect()
    }
}
