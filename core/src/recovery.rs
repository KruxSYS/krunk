//! The vault's recovery phrase (Krunk 0.7.0, R13): 24 English BIP39 words for 256 random bits,
//! with BIP39's checksum. The words are the input to a second key slot of the vault
//! (`vault::Vault::set_recovery`); they are shown once, never stored, never logged.
//!
//! Parsing is tolerant of how people write a phrase down: case, extra spaces, line breaks,
//! commas, numbering ("1. abandon") and words cut to their first four letters (BIP39 words are
//! unique in their first four). It says which words it does not know.

use bip39::{Language, Mnemonic};
use zeroize::Zeroizing;

/// Words in a phrase.
pub const WORDS: usize = 24;
const ENTROPY_LEN: usize = 32;

/// Words typed back to confirm a new phrase.
pub const ASKED: usize = 3;

/// `ASKED` different word positions (1-based) to ask back, ascending, uniformly at random.
pub fn ask_positions() -> Option<[usize; ASKED]> {
    let mut picked: Vec<usize> = Vec::with_capacity(ASKED);
    while picked.len() < ASKED {
        let mut b = [0u8; 1];
        getrandom::fill(&mut b).ok()?;
        // Rejection sampling keeps every position equally likely (240 = 10 × 24).
        if usize::from(b[0]) >= 240 {
            continue;
        }
        let p = usize::from(b[0]) % WORDS + 1;
        if !picked.contains(&p) {
            picked.push(p);
        }
    }
    picked.sort_unstable();
    Some([picked[0], picked[1], picked[2]])
}

/// 256 bits of a recovery phrase. The phrase is made from it again when needed.
pub struct Phrase(Zeroizing<[u8; ENTROPY_LEN]>);

impl std::fmt::Debug for Phrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Phrase(…)")
    }
}

/// Why a typed phrase is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhraseError {
    /// Not 24 words (the number found).
    WordCount(usize),
    /// These words (1-based positions) are not in the list.
    UnknownWords(Vec<usize>),
    /// Every word is known but they do not belong together (a word is wrong or two are swapped).
    Checksum,
}

impl std::fmt::Display for PhraseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhraseError::WordCount(n) => write!(f, "A recovery phrase has {WORDS} words; this has {n}"),
            PhraseError::UnknownWords(w) => {
                let list: Vec<String> = w.iter().map(|i| i.to_string()).collect();
                let s = if w.len() == 1 { "" } else { "s" };
                write!(f, "Word{s} {} not in the word list", list.join(", "))
            }
            PhraseError::Checksum => {
                write!(f, "The words do not belong together: check each word and their order")
            }
        }
    }
}

impl Phrase {
    /// A new random phrase.
    pub fn generate() -> Result<Phrase, String> {
        let mut e = Zeroizing::new([0u8; ENTROPY_LEN]);
        getrandom::fill(e.as_mut()).map_err(|_| "System random number generator failed".to_string())?;
        Ok(Phrase(e))
    }

    pub fn from_entropy(entropy: [u8; ENTROPY_LEN]) -> Phrase {
        Phrase(Zeroizing::new(entropy))
    }

    /// The 256 bits (the key slot's input).
    pub fn entropy(&self) -> &[u8; ENTROPY_LEN] {
        &self.0
    }

    /// The words, in order (index 0 is word 1).
    pub fn words(&self) -> Zeroizing<Vec<String>> {
        let list = Language::English.word_list();
        match Mnemonic::from_entropy(self.0.as_ref()) {
            Ok(m) => Zeroizing::new(m.word_indices().map(|i| list[i].to_string()).collect()),
            Err(_) => Zeroizing::new(Vec::new()),
        }
    }

    /// The words separated by spaces.
    pub fn text(&self) -> Zeroizing<String> {
        Zeroizing::new(self.words().join(" "))
    }

    /// Is `word` (as typed) word number `position` (1-based)?
    pub fn word_is(&self, position: usize, word: &str) -> bool {
        let words = self.words();
        match (position.checked_sub(1).and_then(|i| words.get(i)), canonical_word(word)) {
            (Some(w), Some(typed)) => *w == typed,
            _ => false,
        }
    }

    /// A phrase as someone typed it.
    pub fn parse(text: &str) -> Result<Phrase, PhraseError> {
        let tokens = tokens(text);
        if tokens.len() != WORDS {
            return Err(PhraseError::WordCount(tokens.len()));
        }
        let mut joined = Zeroizing::new(String::with_capacity(WORDS * 9));
        let mut unknown = Vec::new();
        for (i, t) in tokens.iter().enumerate() {
            match canonical_word(t) {
                Some(w) => {
                    if !joined.is_empty() {
                        joined.push(' ');
                    }
                    joined.push_str(w);
                }
                None => unknown.push(i + 1),
            }
        }
        if !unknown.is_empty() {
            return Err(PhraseError::UnknownWords(unknown));
        }
        let m = Mnemonic::parse_in_normalized(Language::English, &joined).map_err(|_| PhraseError::Checksum)?;
        let (bytes, len) = m.to_entropy_array();
        if len != ENTROPY_LEN {
            return Err(PhraseError::WordCount(tokens.len()));
        }
        let mut e = Zeroizing::new([0u8; ENTROPY_LEN]);
        e.copy_from_slice(&bytes[..ENTROPY_LEN]);
        Ok(Phrase(e))
    }
}

/// The words of a typed phrase: split on anything but letters and digits; numbering dropped.
fn tokens(text: &str) -> Zeroizing<Vec<String>> {
    Zeroizing::new(
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit()))
            .map(|t| t.to_ascii_lowercase())
            .collect(),
    )
}

/// The list's word for `typed`: the word itself, or the one word that starts with its first
/// four letters (a word cut short, or with a typo after the fourth letter).
pub fn canonical_word(typed: &str) -> Option<&'static str> {
    let t = typed.trim().to_ascii_lowercase();
    let list = Language::English.word_list();
    if let Some(i) = Language::English.find_word(&t) {
        return Some(list[usize::from(i)]);
    }
    if t.len() < 4 || !t.is_ascii() {
        return None;
    }
    let prefix = &t[..4];
    let mut found = list.iter().filter(|w| w.starts_with(prefix));
    match (found.next(), found.next()) {
        (Some(w), None) => Some(w),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_vectors() {
        // BIP39's own test vectors (trezor/python-mnemonic vectors.json, 256-bit entries).
        let cases: [([u8; 32], &str); 4] = [
            ([0x00; 32], "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"),
            ([0x7f; 32], "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title"),
            ([0x80; 32], "letter advice cage absurd amount doctor acoustic avoid letter advice cage absurd amount doctor acoustic avoid letter advice cage absurd amount doctor acoustic bless"),
            ([0xff; 32], "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo vote"),
        ];
        for (entropy, words) in cases {
            let p = Phrase::from_entropy(entropy);
            assert_eq!(p.text().as_str(), words);
            assert_eq!(Phrase::parse(words).unwrap().entropy(), &entropy);
        }
    }

    #[test]
    fn typed_phrases_are_read_kindly() {
        let p = Phrase::from_entropy([0x7f; 32]);
        let words = p.words();
        // Numbered, in capitals, over several lines, with commas.
        let numbered: String =
            words.iter().enumerate().map(|(i, w)| format!("{}. {},\n", i + 1, w.to_uppercase())).collect();
        assert_eq!(Phrase::parse(&numbered).unwrap().entropy(), p.entropy());
        // Cut to four letters, or a typo after the fourth.
        let short: Vec<String> = words.iter().map(|w| w.chars().take(4).collect()).collect();
        assert_eq!(Phrase::parse(&short.join(" ")).unwrap().entropy(), p.entropy());
        assert_eq!(canonical_word("sausagx"), Some("sausage"));
        assert_eq!(canonical_word("WINNER"), Some("winner"));
        assert_eq!(canonical_word("ab"), None, "too short to know");
        assert_eq!(canonical_word("xyzzy"), None);
    }

    #[test]
    fn mistakes_are_named() {
        let p = Phrase::from_entropy([0x80; 32]);
        let mut words: Vec<String> = p.words().to_vec();
        assert_eq!(Phrase::parse(&words[..23].join(" ")).unwrap_err(), PhraseError::WordCount(23));
        words[4] = "qqqq".into();
        words[17] = "blorb".into();
        assert_eq!(Phrase::parse(&words.join(" ")).unwrap_err(), PhraseError::UnknownWords(vec![5, 18]));
        // Known words in the wrong order break the checksum.
        let mut swapped: Vec<String> = p.words().to_vec();
        swapped.swap(0, 1);
        assert_eq!(Phrase::parse(&swapped.join(" ")).unwrap_err(), PhraseError::Checksum);
        assert!(PhraseError::UnknownWords(vec![5, 18]).to_string().contains("5, 18"));
    }

    #[test]
    fn generated_phrases_differ_and_round_trip() {
        let a = Phrase::generate().unwrap();
        let b = Phrase::generate().unwrap();
        assert_ne!(a.entropy(), b.entropy());
        assert_eq!(a.words().len(), WORDS);
        assert_eq!(Phrase::parse(&a.text()).unwrap().entropy(), a.entropy());
        assert!(
            a.word_is(3, &a.words()[2]) && !a.word_is(3, "zzzz") && !a.word_is(0, "abandon") && !a.word_is(25, "x")
        );
        assert_eq!(format!("{a:?}"), "Phrase(…)", "never printed");
        for _ in 0..50 {
            let p = ask_positions().unwrap();
            assert!(p[0] >= 1 && p[0] < p[1] && p[1] < p[2] && p[2] <= WORDS, "{p:?}");
        }
    }
}
