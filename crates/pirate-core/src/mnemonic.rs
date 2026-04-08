//! BIP39 mnemonic language support and helpers.

use crate::{Error, Result};
use bip39::{Language, Mnemonic};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Supported BIP39 languages for wallet seed phrases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MnemonicLanguage {
    /// English word list.
    English,
    /// Chinese simplified word list.
    ChineseSimplified,
    /// Chinese traditional word list.
    ChineseTraditional,
    /// French word list.
    French,
    /// Italian word list.
    Italian,
    /// Japanese word list.
    Japanese,
    /// Korean word list.
    Korean,
    /// Spanish word list.
    Spanish,
}

impl Default for MnemonicLanguage {
    fn default() -> Self {
        Self::English
    }
}

impl MnemonicLanguage {
    /// Ordered list of supported mnemonic languages.
    pub const ALL: [Self; 8] = [
        Self::English,
        Self::ChineseSimplified,
        Self::ChineseTraditional,
        Self::French,
        Self::Italian,
        Self::Japanese,
        Self::Korean,
        Self::Spanish,
    ];

    /// Native-script label for UI display.
    pub const fn native_label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::ChineseSimplified => "中文(简体)",
            Self::ChineseTraditional => "中文(繁體)",
            Self::French => "Français",
            Self::Italian => "Italiano",
            Self::Japanese => "日本語",
            Self::Korean => "한국어",
            Self::Spanish => "Español",
        }
    }

    /// Stable storage / RPC key for the language.
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::English => "english",
            Self::ChineseSimplified => "chinese_simplified",
            Self::ChineseTraditional => "chinese_traditional",
            Self::French => "french",
            Self::Italian => "italian",
            Self::Japanese => "japanese",
            Self::Korean => "korean",
            Self::Spanish => "spanish",
        }
    }

    /// Parse from a stable storage / RPC key.
    pub fn from_key(value: &str) -> Option<Self> {
        match value {
            "english" => Some(Self::English),
            "chinese_simplified" => Some(Self::ChineseSimplified),
            "chinese_traditional" => Some(Self::ChineseTraditional),
            "french" => Some(Self::French),
            "italian" => Some(Self::Italian),
            "japanese" => Some(Self::Japanese),
            "korean" => Some(Self::Korean),
            "spanish" => Some(Self::Spanish),
            _ => None,
        }
    }

    /// Convert to the underlying bip39 crate language.
    pub const fn to_bip39(self) -> Language {
        match self {
            Self::English => Language::English,
            Self::ChineseSimplified => Language::SimplifiedChinese,
            Self::ChineseTraditional => Language::TraditionalChinese,
            Self::French => Language::French,
            Self::Italian => Language::Italian,
            Self::Japanese => Language::Japanese,
            Self::Korean => Language::Korean,
            Self::Spanish => Language::Spanish,
        }
    }

    /// Convert from the underlying bip39 crate language.
    pub fn from_bip39(language: Language) -> Option<Self> {
        match language {
            Language::English => Some(Self::English),
            Language::SimplifiedChinese => Some(Self::ChineseSimplified),
            Language::TraditionalChinese => Some(Self::ChineseTraditional),
            Language::French => Some(Self::French),
            Language::Italian => Some(Self::Italian),
            Language::Japanese => Some(Self::Japanese),
            Language::Korean => Some(Self::Korean),
            Language::Spanish => Some(Self::Spanish),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

/// Inspection details for a mnemonic phrase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MnemonicInspection {
    /// Whether the phrase validated successfully.
    pub is_valid: bool,
    /// Detected language when a unique valid language is known.
    pub detected_language: Option<MnemonicLanguage>,
    /// Valid candidate languages when the phrase is ambiguous.
    pub ambiguous_languages: Vec<MnemonicLanguage>,
    /// Number of words in the provided phrase.
    pub word_count: u32,
}

impl MnemonicInspection {
    /// True when multiple valid languages remain after validation.
    pub fn is_ambiguous(&self) -> bool {
        self.ambiguous_languages.len() > 1
    }
}

fn entropy_size_for_word_count(word_count: u32) -> usize {
    match word_count {
        12 => 16,
        18 => 24,
        24 => 32,
        _ => 32,
    }
}

fn map_bip39_error(error: bip39::Error) -> Error {
    Error::InvalidSeed(error.to_string())
}

/// Parse a mnemonic, optionally constraining the language.
pub fn parse_mnemonic(
    mnemonic: &str,
    language: Option<MnemonicLanguage>,
) -> Result<(Mnemonic, MnemonicLanguage)> {
    match language {
        Some(language) => {
            let parsed =
                Mnemonic::parse_in(language.to_bip39(), mnemonic).map_err(map_bip39_error)?;
            Ok((parsed, language))
        }
        None => {
            let parsed = Mnemonic::parse(mnemonic).map_err(map_bip39_error)?;
            let language = MnemonicLanguage::from_bip39(parsed.language())
                .ok_or_else(|| Error::InvalidSeed("unsupported mnemonic language".to_string()))?;
            Ok((parsed, language))
        }
    }
}

/// Inspect a mnemonic phrase without mutating wallet state.
pub fn inspect_mnemonic(mnemonic: &str) -> MnemonicInspection {
    let word_count = mnemonic.split_whitespace().count() as u32;

    match Mnemonic::parse(mnemonic) {
        Ok(parsed) => MnemonicInspection {
            is_valid: true,
            detected_language: MnemonicLanguage::from_bip39(parsed.language()),
            ambiguous_languages: Vec::new(),
            word_count,
        },
        Err(bip39::Error::AmbiguousLanguages(ambiguous)) => {
            let valid_languages = ambiguous
                .iter()
                .filter_map(MnemonicLanguage::from_bip39)
                .filter(|language| Mnemonic::parse_in(language.to_bip39(), mnemonic).is_ok())
                .collect::<Vec<_>>();

            let detected_language = match valid_languages.as_slice() {
                [language] => Some(*language),
                _ => None,
            };

            MnemonicInspection {
                is_valid: !valid_languages.is_empty(),
                detected_language,
                ambiguous_languages: if valid_languages.len() > 1 {
                    valid_languages
                } else {
                    Vec::new()
                },
                word_count,
            }
        }
        Err(_) => MnemonicInspection {
            is_valid: false,
            detected_language: None,
            ambiguous_languages: Vec::new(),
            word_count,
        },
    }
}

/// Canonicalize a mnemonic phrase and return its resolved language.
pub fn canonicalize_mnemonic(
    mnemonic: &str,
    language: Option<MnemonicLanguage>,
) -> Result<(String, MnemonicLanguage)> {
    let (parsed, language) = parse_mnemonic(mnemonic, language)?;
    Ok((parsed.to_string(), language))
}

/// Derive seed bytes from a mnemonic phrase.
pub fn seed_bytes_from_mnemonic(
    mnemonic: &str,
    language: Option<MnemonicLanguage>,
) -> Result<Vec<u8>> {
    let entropy = entropy_from_mnemonic(mnemonic, language)?;
    let english =
        Mnemonic::from_entropy_in(Language::English, &entropy).map_err(map_bip39_error)?;
    Ok(english.to_seed("").to_vec())
}

/// Recover entropy bytes from a mnemonic phrase.
pub fn entropy_from_mnemonic(
    mnemonic: &str,
    language: Option<MnemonicLanguage>,
) -> Result<Vec<u8>> {
    let (parsed, _) = parse_mnemonic(mnemonic, language)?;
    Ok(parsed.to_entropy())
}

/// Render a mnemonic phrase from entropy in the requested language.
pub fn mnemonic_from_entropy(entropy: &[u8], language: MnemonicLanguage) -> Result<String> {
    let mnemonic =
        Mnemonic::from_entropy_in(language.to_bip39(), entropy).map_err(map_bip39_error)?;
    Ok(mnemonic.to_string())
}

/// Convert a mnemonic to a different display language.
pub fn convert_mnemonic_language(
    mnemonic: &str,
    source_language: Option<MnemonicLanguage>,
    target_language: MnemonicLanguage,
) -> Result<String> {
    let entropy = entropy_from_mnemonic(mnemonic, source_language)?;
    mnemonic_from_entropy(&entropy, target_language)
}

/// Generate a new mnemonic in the requested language.
pub fn generate_mnemonic(word_count: Option<u32>, language: Option<MnemonicLanguage>) -> String {
    let word_count = word_count.unwrap_or(24);
    let entropy_size = entropy_size_for_word_count(word_count);

    let mut entropy = vec![0u8; entropy_size];
    rand::thread_rng().fill_bytes(&mut entropy);

    let mnemonic = Mnemonic::from_entropy_in(language.unwrap_or_default().to_bip39(), &entropy)
        .expect("entropy should always produce a valid mnemonic");
    mnemonic.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entropy_roundtrip_for_supported_languages() {
        let entropy = [
            0x5a, 0x11, 0x93, 0x20, 0xf0, 0x44, 0x17, 0x9b, 0x2e, 0x77, 0x80, 0x4d, 0xc3, 0x0f,
            0xa1, 0x55,
        ];

        for language in MnemonicLanguage::ALL {
            let mnemonic = mnemonic_from_entropy(&entropy, language).expect("render mnemonic");
            let (parsed, detected_language) =
                parse_mnemonic(&mnemonic, Some(language)).expect("parse mnemonic");
            assert_eq!(detected_language, language);
            assert_eq!(parsed.to_entropy(), entropy);

            let inspection = inspect_mnemonic(&mnemonic);
            assert!(
                inspection.is_valid,
                "inspection should validate {language:?}"
            );
            assert_eq!(inspection.detected_language, Some(language));
            assert!(inspection.ambiguous_languages.is_empty());
        }
    }

    #[test]
    fn test_convert_mnemonic_language_preserves_seed_bytes() {
        let entropy = [
            0x7c, 0x2f, 0x91, 0x01, 0x33, 0x0a, 0x4e, 0x9d, 0x55, 0x81, 0xe2, 0x10, 0x42, 0xaa,
            0x60, 0xdd,
        ];
        let english = mnemonic_from_entropy(&entropy, MnemonicLanguage::English).unwrap();
        let spanish = convert_mnemonic_language(
            &english,
            Some(MnemonicLanguage::English),
            MnemonicLanguage::Spanish,
        )
        .unwrap();

        let english_seed =
            seed_bytes_from_mnemonic(&english, Some(MnemonicLanguage::English)).unwrap();
        let spanish_seed =
            seed_bytes_from_mnemonic(&spanish, Some(MnemonicLanguage::Spanish)).unwrap();

        assert_eq!(english_seed, spanish_seed);
    }
}
