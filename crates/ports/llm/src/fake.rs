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
//! # The answer to a [`TextKind::Prompt`] request
//!
//! A fake handed character proposals ([`FakeLlm::with_character`]) answers a
//! prompt that holds one's words with that proposal, as one JSON object —
//! the shape `ai.proposeCharacter` asks for and parses:
//!
//! ```json
//! {"background":"…","behavior":"…","stats":{"FOR":8,"DEX":15,"CON":10,"INT":13,"SAG":9,"CHA":12},
//!  "ca":13,"pvMax":21,"features":[{"name":"Fuite agile","text":"…"}]}
//! ```
//!
//! The first proposal handed whose words the prompt holds answers it; one
//! handed for no words answers any prompt. A prompt no proposal answers, and
//! every prompt of a fake handed none, keeps the canned sentence.
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
    /// The character proposals a prompt is answered, with the words it
    /// holds for each, in the order they were handed.
    characters: Vec<(String, Value)>,
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

    /// Makes a [`TextKind::Prompt`] request whose text holds `holding` — any
    /// request, when `holding` is empty — answered `proposal`, written as
    /// JSON, unless a proposal handed earlier answers it first.
    #[must_use]
    pub fn with_character(mut self, holding: impl Into<String>, proposal: Value) -> Self {
        self.characters.push((holding.into(), proposal));
        self
    }

    /// The proposal handed for the words `prompt` holds, the first one first.
    fn character(&self, prompt: &str) -> Option<&Value> {
        self.characters
            .iter()
            .find(|(holding, _)| prompt.contains(holding.as_str()))
            .map(|(_, proposal)| proposal)
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
        let character = match request.kind {
            TextKind::Prompt => self.character(&request.text),
            TextKind::Brief | TextKind::Notes | TextKind::Journal => None,
        };
        let text = match (request.kind, character) {
            (TextKind::Notes, _) => notes_answer(&request.text),
            (_, Some(proposal)) => proposal.to_string(),
            (TextKind::Prompt | TextKind::Brief | TextKind::Journal, None) => format!(
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
        if let Some(name) = event_name(sentence) {
            propose("evenement", name, sentence);
        }
        for (kind, name) in names(sentence) {
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
/// holds one.
fn event_name(sentence: &str) -> Option<String> {
    let clause = sentence.split([',', ';', ':']).find(|clause| {
        clause
            .split_whitespace()
            .any(|raw| Word::new(raw).is_one_of(EVENT_WORDS))
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
/// holds or follows a place noun.
fn names(sentence: &str) -> Vec<(&'static str, String)> {
    let words: Vec<Word> = sentence.split_whitespace().map(Word::new).collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < words.len() {
        if !opens_a_name(&words, index) {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while !words[end - 1].ends_clause {
            match words.get(end) {
                Some(next) if next.is_capitalised() => end += 1,
                Some(joiner)
                    if joiner.is_one_of(JOINERS)
                        && !joiner.ends_clause
                        && words.get(end + 1).is_some_and(Word::is_capitalised) =>
                {
                    end += 2;
                }
                _ => break,
            }
        }
        found.push(name_of(&words, index, end));
        index = end;
    }
    found
}

/// Whether a name starts at `words[index]`: a capitalised word, but not a
/// function word that only is capitalised for opening the sentence — unless
/// it is an article opening a name (*La Tour d’Ébène*).
fn opens_a_name(words: &[Word], index: usize) -> bool {
    let word = &words[index];
    if !word.is_capitalised() {
        return false;
    }
    if index > 0 || !FUNCTION_WORDS.contains(&word.text) {
        return true;
    }
    ARTICLES.contains(&word.text) && words.get(1).is_some_and(Word::is_capitalised)
}

/// The type and the text of the name `words[start..end]`, with the place noun
/// that leads it (*bois de* Corbière) and without the elision that opens it
/// (*d’*Elowen).
fn name_of(words: &[Word], start: usize, end: usize) -> (&'static str, String) {
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
    (if place { "lieu" } else { "pnj" }, parts.join(" "))
}
