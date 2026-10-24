//! The fake of the `llm` port: it answers deterministically, sends nothing
//! (no network, no process, no key) and journals every request it receives.
//!
//! An API URL is recognised the way `ProviderApiType@1` says: the cloud host
//! by itself, any other host by the model list its endpoint answers. The fake
//! has no endpoint to ask, so the answers are handed to it
//! ([`FakeLlm::with_endpoint`]); a URL it was given none for is, like an
//! endpoint that did not answer, not recognised. A binary always answers.
//!
//! # The answer to a [`TextKind::Notes`] request
//!
//! The fake reads the notes the way a quick reader would and answers the
//! shape `ai.extractFromNotes` parses — **one JSON object, no prose around
//! it**:
//!
//! ```json
//! {"proposals":[{"type":"pnj","name":"Mère Ombeline","excerpt":"Mère Ombeline, l’herboriste aveugle…"}]}
//! ```
//!
//! `type` is one of `pj`, `pnj`, `lieu`, `evenement`; `excerpt` is a sentence of
//! the notes, `name` its words (an event's first letter is capitalised). The real adapters' prompt asks the model
//! for this same object. The reading is a plain heuristic, the same text
//! always giving the same answer, in the order of the notes:
//!
//! - a run of capitalised words is a name — `pnj`, or `lieu` when it holds a
//!   place noun (*Tour d’Ébène*) or follows one (*bois de Corbière*);
//! - a sentence holding an event word (*brûlé*, *embuscade*…) is an
//!   `evenement`, named by the clause that holds it;
//! - the excerpt is the sentence the proposal comes from;
//! - a player character (`pj`) cannot be guessed: the fake never proposes one.
//!
//! A heuristic, not a reader: a sentence-opening word it does not know is
//! taken for a name, and an event and the characters of its sentence are
//! proposed side by side.
//!
//! # The answer to a [`TextKind::Brief`] request
//!
//! The brief is cut into scenes and answered as the shape `ai.analyzeBrief`
//! parses — **one JSON object, no prose around it**:
//!
//! ```json
//! {"scenes":[{"title":"Sur la route","notes":"Deuxième scène, sur la route…","elements":[{"type":"pnj","name":"Frère Odric","excerpt":"…un pèlerin, frère Odric, les arrête…"}]}]}
//! ```
//!
//! - a sentence whose first clause holds the word *scène* (*Deuxième scène,
//!   …*) opens a new scene; the brief before the first such sentence is the
//!   first scene, a brief with none is one scene;
//! - a scene's `notes` are its words, and its `title` the first clause of its
//!   opening sentence that is neither its marker nor a single word, cut at
//!   eight words, its first letter capitalised — `Scène <n>` when none is;
//! - its elements are read as the notes are, a name of each type once, its
//!   excerpt the sentences of the scene that name it, with what a brief says
//!   more plainly: the article before a place joins it (*la Taverne du
//!   Sanglier* is *La Taverne du Sanglier*), a title before a character too
//!   (*frère Odric* is *Frère Odric*), and so does the nickname given after
//!   a comma (*Vesna, qu’on appelle la Balafrée* is *Vesna la Balafrée*); an
//!   ordinal or a number opening a sentence and an abbreviation (*PV*) are
//!   no names.
//!
//! Every other kind keeps the canned sentence.

use std::collections::HashMap;

use serde_json::Value;

use crate::{
    CONNECTION_TEST_PROMPT, ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError,
    Provider, ProviderConfig, RequestJournal, RequestKind, TextKind, UNRECOGNISED_API_TYPE,
    detect_api_type,
};

/// The latency every successful fake test reports, in milliseconds.
pub const FAKE_LATENCY_MS: u64 = 12;

/// The recording fake.
#[derive(Clone, Debug, Default)]
pub struct FakeLlm {
    /// What each URL's endpoint answers to its list of models.
    endpoints: HashMap<String, Value>,
    journal: RequestJournal,
}

impl FakeLlm {
    /// A fake whose endpoints answer nothing: only the cloud host and the
    /// binaries are recognised.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the endpoint at `url` — matched as the exact string a
    /// configuration carries — answer `model_list` to its list of models.
    #[must_use]
    pub fn with_endpoint(mut self, url: impl Into<String>, model_list: Value) -> Self {
        self.endpoints.insert(url.into(), model_list);
        self
    }

    /// The provider `config` reaches; `trust_hint` takes the type a saved API
    /// provider already carries instead of detecting it.
    fn resolve(&self, config: &ProviderConfig, trust_hint: bool) -> Option<Provider> {
        match config {
            ProviderConfig::Binary { binary, .. } => Some(Provider::from(*binary)),
            ProviderConfig::Api { url, api_type, .. } => api_type
                .filter(|_| trust_hint)
                .or_else(|| detect_api_type(url, self.endpoints.get(url)))
                .map(Provider::from),
        }
    }
}

impl Llm for FakeLlm {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        let Some(provider) = self.resolve(config, false) else {
            return ConnectionReport {
                ok: false,
                provider: None,
                latency_ms: 0,
                message: UNRECOGNISED_API_TYPE.to_owned(),
            };
        };
        self.journal.record(
            RequestKind::ConnectionTest,
            provider,
            CONNECTION_TEST_PROMPT,
        );
        let models = config.models();
        ConnectionReport {
            ok: true,
            provider: Some(provider),
            latency_ms: FAKE_LATENCY_MS,
            message: format!(
                "Connexion réussie ({}) — génération : {}, résumé : {}",
                provider.label(),
                models.generation,
                models.summary
            ),
        }
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        let provider = self
            .resolve(config, true)
            .ok_or(LlmError::UnrecognisedApiType)?;
        self.journal
            .record(RequestKind::from(request.kind), provider, &request.text);
        let model = config.models().for_kind(request.kind).to_owned();
        let text = match request.kind {
            TextKind::Notes => notes_answer(&request.text),
            TextKind::Brief => brief_answer(&request.text),
            TextKind::Prompt | TextKind::Journal => format!(
                "Réponse simulée de {model} ({}, {} caractères)",
                provider.label(),
                request.text.chars().count()
            ),
        };
        Ok(GenerateAnswer { text, model })
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

/// Nouns that make a name a place (*Tour d’Ébène*, *bois de Corbière*).
const PLACE_NOUNS: &[&str] = &[
    "abbaye",
    "auberge",
    "bois",
    "caverne",
    "château",
    "colline",
    "crypte",
    "donjon",
    "forêt",
    "forteresse",
    "grotte",
    "gué",
    "manoir",
    "marché",
    "montagne",
    "mont",
    "moulin",
    "pont",
    "port",
    "route",
    "ruine",
    "ruines",
    "taverne",
    "temple",
    "tour",
    "vallée",
    "village",
    "ville",
    "île",
    "rivière",
];

/// Words that say something happened: a sentence holding one is an event.
const EVENT_WORDS: &[&str] = &[
    "assassiné",
    "attaque",
    "attaqué",
    "bataille",
    "brûlé",
    "combat",
    "disparition",
    "disparu",
    "effondrement",
    "embuscade",
    "explosion",
    "fête",
    "incendie",
    "invasion",
    "mariage",
    "meurtre",
    "siège",
];

/// Words that join the parts of a name (*bois **de** Corbière*).
const JOINERS: &[&str] = &["de", "du", "des"];

/// Capitalised at the head of a sentence, and not a name.
const FUNCTION_WORDS: &[&str] = &[
    "À", "Alors", "Après", "Au", "Aux", "Avant", "Avec", "Ce", "Ces", "Cette", "Dans", "Demain",
    "Depuis", "Des", "Elle", "Elles", "En", "Enfin", "Ensuite", "Et", "Hier", "Il", "Ils", "Je",
    "La", "Le", "Les", "Mais", "Nous", "On", "Ou", "Où", "Par", "Pendant", "Pour", "Puis", "Quand",
    "Soudain", "Sur", "Un", "Une", "Vous", "Chaque", "Chez", "Comme", "Donc", "Ici", "Leur",
    "Leurs", "Ma", "Mes", "Mon", "Nos", "Notre", "Ni", "Sa", "Sans", "Selon", "Ses", "Si", "Son",
    "Ta", "Tes", "Ton", "Tous", "Tout", "Toute", "Toutes", "Vers",
];

/// The words of a name an article may open (*La Tour d’Ébène*).
const ARTICLES: &[&str] = &["La", "Le", "Les"];

/// Capitalised at the head of a brief's sentence, and not a name: what
/// counts the scenes and the foes (*Deuxième scène*, *Trois bandits*).
const BRIEF_OPENERS: &[&str] = &[
    "Bon",
    "Cinq",
    "Cinquième",
    "Deux",
    "Deuxième",
    "Dernier",
    "Dernière",
    "Premier",
    "Première",
    "Quatre",
    "Quatrième",
    "Scène",
    "Second",
    "Seconde",
    "Séance",
    "Trois",
    "Troisième",
];

/// Titles a character's name may follow in lowercase (*frère Odric*).
const TITLES: &[&str] = &[
    "abbé",
    "baron",
    "baronne",
    "capitaine",
    "comte",
    "comtesse",
    "dame",
    "frère",
    "maître",
    "mère",
    "messire",
    "père",
    "sergent",
    "sergente",
    "sire",
    "sœur",
];

/// The articles a place's name takes in a brief (*la Taverne du Sanglier*).
const PLACE_ARTICLES: &[&str] = &["la", "le", "les"];

/// The words that give a character a nickname after its name (*Vesna,
/// qu’on appelle la Balafrée*; *Gaspard, dit le Rouge*).
const NICKNAMED: &[&[&str]] = &[
    &["qu’on", "appelle"],
    &["qu'on", "appelle"],
    &["qu’on", "surnomme"],
    &["qu'on", "surnomme"],
    &["dit"],
    &["dite"],
    &["surnommé"],
    &["surnommée"],
];

/// The word that marks a sentence opening a new scene of a brief.
const SCENE_MARKER: &str = "scène";

/// How a text is read: the notes as they always were, a brief with what it
/// says more plainly.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Notes,
    Brief,
}

/// The longest a clause may be to name an event, in words.
const EVENT_NAME_WORDS: usize = 8;

/// One whitespace-separated word of a sentence.
struct Word<'a> {
    /// The word without the punctuation around it.
    text: &'a str,
    /// The word ended a clause (a comma, a colon, a closing parenthesis…): a
    /// name does not run past it.
    ends_clause: bool,
}

impl<'a> Word<'a> {
    fn new(raw: &'a str) -> Self {
        Self {
            text: raw.trim_matches(|c: char| !c.is_alphanumeric()),
            ends_clause: raw.ends_with([',', ';', ':', ')', '—', '–']),
        }
    }

    /// The word without its elision (*d’Elowen* → *Elowen*).
    fn core(&self) -> &'a str {
        let mut chars = self.text.chars();
        match (chars.next(), chars.next()) {
            (Some('d' | 'D' | 'l' | 'L'), Some('’' | '\'')) => chars.as_str(),
            _ => self.text,
        }
    }

    fn is_capitalised(&self) -> bool {
        let core = self.core();
        core.chars().count() > 1 && core.chars().next().is_some_and(char::is_uppercase)
    }

    /// An abbreviation (*PV*, *PNJ*), of a capitalised word — two letters
    /// or more: none lowercase.
    fn is_acronym(&self) -> bool {
        !self.text.chars().any(char::is_lowercase)
    }

    /// Whether the word may be part of a name, as `reading` reads it.
    fn names_in(&self, reading: Reading) -> bool {
        self.is_capitalised() && (reading == Reading::Notes || !self.is_acronym())
    }

    fn is_one_of(&self, words: &[&str]) -> bool {
        let lowercase = self.text.to_lowercase();
        words.contains(&lowercase.as_str())
    }
}

/// The answer to a Notes request: the proposals found in `text`, as the one
/// JSON object the module documents.
fn notes_answer(text: &str) -> String {
    let mut proposals: Vec<Value> = Vec::new();
    let mut seen: Vec<(&str, String)> = Vec::new();
    let mut propose = |kind: &'static str, name: String, excerpt: &str| {
        let key = (kind, name.to_lowercase());
        if !seen.contains(&key) {
            seen.push(key);
            proposals.push(serde_json::json!({"type": kind, "name": name, "excerpt": excerpt}));
        }
    };
    for sentence in sentences(text) {
        if let Some(name) = event_name(sentence, Reading::Notes) {
            propose("evenement", name, sentence);
        }
        for (kind, name) in names(sentence, Reading::Notes) {
            propose(kind, name, sentence);
        }
    }
    serde_json::json!({ "proposals": proposals }).to_string()
}

/// The sentences of `text`, trimmed, in order: a full stop, `!`, `?` or `…`
/// followed by a space, or a line break, ends one.
fn sentences(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let ends = c == '\n'
            || (matches!(c, '.' | '!' | '?' | '…')
                && chars.peek().is_none_or(|(_, next)| next.is_whitespace()));
        if ends {
            found.push(text[start..index].trim());
            start = index + c.len_utf8();
        }
    }
    found.push(text[start..].trim());
    found.retain(|sentence| !sentence.is_empty());
    found
}

/// The clause of `sentence` that holds an event word, capitalised, when it
/// holds one — in a brief, an elided one too (*l’embuscade*).
fn event_name(sentence: &str, reading: Reading) -> Option<String> {
    let clause = sentence.split([',', ';', ':']).find(|clause| {
        clause.split_whitespace().any(|raw| {
            let word = Word::new(raw);
            word.is_one_of(EVENT_WORDS)
                || (reading == Reading::Brief
                    && EVENT_WORDS.contains(&word.core().to_lowercase().as_str()))
        })
    })?;
    let clause = clause.trim();
    let (mut words, mut in_word, mut cut) = (0, false, clause.len());
    for (index, c) in clause.char_indices() {
        if !c.is_whitespace() && !in_word {
            words += 1;
            if words > EVENT_NAME_WORDS {
                cut = index;
                break;
            }
        }
        in_word = !c.is_whitespace();
    }
    let mut chars = clause[..cut].trim_end().chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// The names of `sentence`, with their type: `pnj`, or `lieu` for a name that
/// holds or follows a place noun — as `reading` reads them.
fn names(sentence: &str, reading: Reading) -> Vec<(&'static str, String)> {
    let words: Vec<Word> = sentence.split_whitespace().map(Word::new).collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < words.len() {
        if !opens_a_name(&words, index, reading) {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while !words[end - 1].ends_clause {
            match words.get(end) {
                Some(next) if next.names_in(reading) => end += 1,
                Some(joiner)
                    if joiner.is_one_of(JOINERS)
                        && !joiner.ends_clause
                        && words.get(end + 1).is_some_and(|w| w.names_in(reading)) =>
                {
                    end += 2;
                }
                _ => break,
            }
        }
        let (kind, mut name) = name_of(&words, index, end, reading);
        if reading == Reading::Brief
            && kind == "pnj"
            && let Some((nickname, after)) = nickname(&words, end)
        {
            name = format!("{name} {nickname}");
            end = after;
        }
        found.push((kind, name));
        index = end;
    }
    found
}

/// The nickname given after the name that ends at `words[at]` (*, qu’on
/// appelle la Balafrée*), its article kept, and where it ends: `None` when
/// none is given.
fn nickname(words: &[Word], at: usize) -> Option<(String, usize)> {
    if !words.get(at.checked_sub(1)?)?.ends_clause {
        return None;
    }
    let given = NICKNAMED.iter().find(|given| {
        given.iter().enumerate().all(|(offset, word)| {
            words
                .get(at + offset)
                .is_some_and(|met| !met.ends_clause && met.text.to_lowercase() == *word)
        })
    })?;
    let mut start = at + given.len();
    let mut parts = Vec::new();
    if let Some(article) = words
        .get(start)
        .filter(|word| !word.ends_clause && word.is_one_of(PLACE_ARTICLES))
    {
        parts.push(article.text);
        start += 1;
    }
    let mut end = start;
    while let Some(word) = words.get(end).filter(|word| word.names_in(Reading::Brief)) {
        parts.push(word.text);
        end += 1;
        if word.ends_clause {
            break;
        }
    }
    (end > start).then(|| (parts.join(" "), end))
}

/// Whether a name starts at `words[index]`: a capitalised word, but not a
/// function word that only is capitalised for opening the sentence — unless
/// it is an article opening a name (*La Tour d’Ébène*).
fn opens_a_name(words: &[Word], index: usize, reading: Reading) -> bool {
    let word = &words[index];
    if !word.names_in(reading) {
        return false;
    }
    let opener = FUNCTION_WORDS.contains(&word.text)
        || (reading == Reading::Brief && BRIEF_OPENERS.contains(&word.text));
    if index > 0 || !opener {
        return true;
    }
    ARTICLES.contains(&word.text) && words.get(1).is_some_and(|w| w.names_in(reading))
}

/// The type and the text of the name `words[start..end]`, with the place noun
/// that leads it (*bois de* Corbière) and without the elision that opens it
/// (*d’*Elowen); in a brief, with the title that leads a character (*frère*
/// Odric) and the article that leads a place (*la* Taverne du Sanglier),
/// capitalised.
fn name_of(words: &[Word], start: usize, end: usize, reading: Reading) -> (&'static str, String) {
    let mut parts: Vec<&str> = words[start..end].iter().map(|word| word.text).collect();
    parts[0] = words[start].core();
    let mut place = words[start..end]
        .iter()
        .any(|word| PLACE_NOUNS.contains(&word.core().to_lowercase().as_str()));
    // The words before the name, none of them closing a clause.
    let before = |back: usize| {
        start
            .checked_sub(back)
            .map(|index| &words[index])
            .filter(|word| !word.ends_clause)
    };
    let lead = if before(1).is_some_and(|word| word.is_one_of(PLACE_NOUNS)) {
        1
    } else if before(1).is_some_and(|word| word.is_one_of(JOINERS))
        && before(2).is_some_and(|word| word.is_one_of(PLACE_NOUNS))
    {
        2
    } else {
        0
    };
    if lead > 0 {
        place = true;
        let led: Vec<&str> = words[start - lead..start]
            .iter()
            .map(|word| word.text)
            .collect();
        parts.splice(0..0, led);
    }
    if reading == Reading::Notes {
        return (if place { "lieu" } else { "pnj" }, parts.join(" "));
    }
    let led_by = |words_of: &[&str]| before(lead + 1).filter(|word| word.is_one_of(words_of));
    let lead = if place {
        led_by(PLACE_ARTICLES)
    } else {
        before(1).filter(|word| word.is_one_of(TITLES))
    };
    if let Some(word) = lead {
        parts.insert(0, word.text);
    }
    let name = capitalised(&parts.join(" "));
    (if place { "lieu" } else { "pnj" }, name)
}

/// `text` with its first letter in uppercase.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The answer to a Brief request: its scenes, each with its title, its
/// notes and the elements it names, as the one JSON object the module
/// documents.
fn brief_answer(text: &str) -> String {
    let scenes: Vec<Value> = scenes(text)
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            serde_json::json!({
                "title": title(scene).unwrap_or_else(|| format!("Scène {}", index + 1)),
                "notes": scene.notes,
                "elements": elements(&scene.sentences),
            })
        })
        .collect();
    serde_json::json!({ "scenes": scenes }).to_string()
}

/// One scene of a brief: its words, and its sentences.
struct Scene<'a> {
    notes: &'a str,
    sentences: Vec<&'a str>,
}

/// The scenes of `text`, in order: a sentence whose first clause holds the
/// marker opens one.
fn scenes(text: &str) -> Vec<Scene<'_>> {
    let mut found: Vec<Scene> = Vec::new();
    for sentence in sentences(text) {
        let at = sentence.as_ptr() as usize - text.as_ptr() as usize;
        let marked = sentence
            .split([',', ';', ':'])
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .any(|raw| Word::new(raw).text.to_lowercase() == SCENE_MARKER);
        match found.last_mut() {
            Some(scene) if !marked => {
                let start = scene.notes.as_ptr() as usize - text.as_ptr() as usize;
                scene.notes = with_end(text, start, at + sentence.len() - start);
                scene.sentences.push(sentence);
            }
            _ => found.push(Scene {
                notes: with_end(text, at, sentence.len()),
                sentences: vec![sentence],
            }),
        }
    }
    found
}

/// The `len` bytes of `text` from `start`, and the mark that ends their
/// sentence when one does (*repart !*).
fn with_end(text: &str, start: usize, len: usize) -> &str {
    let end = start + len;
    let rest = &text[end..];
    let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let mark = rest[spaces..]
        .chars()
        .next()
        .filter(|c| matches!(c, '.' | '!' | '?' | '…'))
        .map_or(0, |c| spaces + c.len_utf8());
    &text[start..end + mark]
}

/// A scene's title: the first clause of its opening sentence that is
/// neither its marker nor a single word, cut at eight words, capitalised.
fn title(scene: &Scene) -> Option<String> {
    let clause = scene
        .sentences
        .first()?
        .split([',', ';', ':'])
        .find(|clause| {
            let words: Vec<Word> = clause.split_whitespace().map(Word::new).collect();
            words.len() > 1
                && !words
                    .iter()
                    .any(|word| word.text.to_lowercase() == SCENE_MARKER)
        })?;
    let words: Vec<&str> = clause.split_whitespace().take(EVENT_NAME_WORDS).collect();
    Some(capitalised(&words.join(" ")))
}

/// The elements `sentences` name: each type and name once, in the order met,
/// its excerpt the sentences that name it.
fn elements(sentences: &[&str]) -> Vec<Value> {
    let mut found: Vec<(&'static str, String, Vec<&str>)> = Vec::new();
    for sentence in sentences {
        let named = event_name(sentence, Reading::Brief)
            .map(|name| ("evenement", name))
            .into_iter()
            .chain(names(sentence, Reading::Brief));
        for (kind, name) in named {
            let key = name.to_lowercase();
            match found
                .iter_mut()
                .find(|(k, n, _)| *k == kind && n.to_lowercase() == key)
            {
                Some((_, _, excerpts)) if !excerpts.contains(sentence) => excerpts.push(sentence),
                Some(_) => {}
                None => found.push((kind, name, vec![sentence])),
            }
        }
    }
    found
        .into_iter()
        .map(|(kind, name, excerpts)| {
            serde_json::json!({"type": kind, "name": name, "excerpt": excerpts.join(" ")})
        })
        .collect()
}
