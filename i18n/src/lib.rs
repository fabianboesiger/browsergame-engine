use std::{
    fmt::Display,
    str::FromStr,
    sync::{Arc, OnceLock, RwLock},
};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use strum::{Display, EnumString};

struct Settings {
    fallback_locale: Locale,
    locales: SmallVec<[Locale; 8]>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fallback_locale: Locale(Language::En, None),
            locales: SmallVec::new(),
        }
    }
}

static SETTINGS: OnceLock<Arc<RwLock<Settings>>> = OnceLock::new();

pub fn set_fallback_locale(locale: Locale) {
    SETTINGS
        .get_or_init(|| Arc::new(RwLock::new(Settings::default())))
        .write()
        .unwrap()
        .fallback_locale = locale;
}

pub fn set_locales(locales: &[Locale]) {
    SETTINGS
        .get_or_init(|| Arc::new(RwLock::new(Settings::default())))
        .write()
        .unwrap()
        .locales = SmallVec::from_slice(locales);
}

fn get_locales() -> SmallVec<[Locale; 8]> {
    let settings = SETTINGS
        .get_or_init(|| Arc::new(RwLock::new(Settings::default())))
        .read()
        .unwrap();
    let mut locales = settings.locales.clone();
    locales.push(settings.fallback_locale);
    locales
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Display, EnumString, PartialEq, Eq)]
#[strum(ascii_case_insensitive)]
pub enum Language {
    En,
    Fr,
    It,
    De,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Display, EnumString, PartialEq, Eq)]
#[strum(ascii_case_insensitive)]
pub enum Country {
    Ch,
    De,
    Gb,
    Us,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Locale(pub Language, pub Option<Country>);

impl Locale {
    pub fn from_str(string: &str) -> Option<Locale> {
        if let Some((language, country)) = string.split_once('-') {
            return Some(Locale(
                Language::from_str(language).ok()?,
                Some(Country::from_str(country).ok()?),
            ));
        }
        if let Some((language, country)) = string.split_once('_') {
            return Some(Locale(
                Language::from_str(language).ok()?,
                Some(Country::from_str(country).ok()?),
            ));
        }
        Some(Locale(Language::from_str(string).ok()?, None))
    }

    /*
    pub fn best_match(user_preferences: &[Locale]) -> Option<Locale> {

        #[derive(PartialEq, Eq, PartialOrd, Ord)]
        enum MatchRating {
            MatchesNothing,
            MatchesLanguage,
            MatchesLanguageAndDefinedCountryIsNone,
            MatchesLanguageAndCountry,
        }

        impl MatchRating {
            fn rate_match(Locale(language, country): Locale, Locale(user_language, user_country): Locale) -> MatchRating {
                if language == user_language && country == user_country {
                    MatchRating::MatchesLanguageAndCountry
                } else if language == user_language && country.is_none() {
                    MatchRating::MatchesLanguageAndDefinedCountryIsNone
                } else if language == user_language {
                    MatchRating::MatchesLanguage
                } else {
                    MatchRating::MatchesNothing
                }
            }
        }

        let mut best_match = None;
        let mut best_rating = MatchRating::MatchesNothing;

        let supported_locales = get_supported_locales();

        for &user_locale in user_preferences {
            for &locale in &supported_locales {
                let rating = MatchRating::rate_match(locale, user_locale);
                if rating > best_rating {
                    best_match = Some(locale);
                    best_rating = rating;
                }
            }
            if best_rating > MatchRating::MatchesNothing {
                break;
            }
        }

        best_match
    }
    */
}

pub trait Localizable: Sized {
    fn localize(self) -> Localized {
        self.localize_with(get_locales().as_slice())
    }

    fn localize_with(self, locale: &[Locale]) -> Localized;
}

impl<'a> Localizable for &'a str {
    fn localize_with(self, _locale: &[Locale]) -> Localized {
        Localized::from(self)
    }
}

impl Localizable for String {
    fn localize_with(self, _locale: &[Locale]) -> Localized {
        Localized::from(self)
    }
}

pub struct Localized(String);

impl From<String> for Localized {
    fn from(value: String) -> Self {
        Localized(value)
    }
}

impl<'a> From<&'a str> for Localized {
    fn from(value: &'a str) -> Self {
        Localized(value.to_owned())
    }
}

impl Display for Localized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(not(feature = "seed"))]
#[macro_export]
macro_rules! localize {
    (pub enum $name:ident { $(
        $variant:ident $( ( $( $var_name:ident: $var_ty:ty ),* $(,)? ) )? {
            $( $pattern:pat => $tr:expr ),+ $(,)?
        } $(,)?
    )* } ) => {
        #[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
        pub enum $name {
            $(
                $variant $( ( $( $var_ty ),* ) )? ,
            )*
        }

        impl $crate::Localizable for $name {
            fn localize_with(self, locales: &[$crate::Locale]) -> $crate::Localized {
                use $crate::Locale;
                use $crate::Language;
                use $crate::Country;

                match self {
                    $(
                        Self:: $variant $( ( $( $var_name ),* ) )? => for locale in locales {
                            match locale {
                                $(
                                    $pattern => return $crate::Localized::from($tr)
                                ),*
                            }
                        }
                    ),*
                }

                $crate::Localized::from(format!("{:?}", self))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", <Self as $crate::Localizable>::localize(self.clone()).to_string())
            }
        }
    };
}

#[cfg(feature = "seed")]
#[macro_export]
macro_rules! localize {
    (pub enum $name:ident { $(
        $variant:ident $( ( $( $var_name:ident: $var_ty:ty ),* $(,)? ) )? {
            $( $pattern:pat => $tr:expr ),+ $(,)?
        } $(,)?
    )* } ) => {
        #[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
        pub enum $name {
            $(
                $variant $( ( $( $var_ty ),* ) )? ,
            )*
        }

        impl $crate::Localizable for $name {
            fn localize_with(self, locales: &[$crate::Locale]) -> $crate::Localized {
                use $crate::Locale;
                use $crate::Language;
                use $crate::Country;

                match self {
                    $(
                        Self:: $variant $( ( $( $var_name ),* ) )? => for locale in locales {
                            match locale {
                                $(
                                    $pattern => return $crate::Localized::from($tr)
                                ),*
                            }
                        }
                    ),*
                }

                $crate::Localized::from(format!("{:?}", self))
            }
        }

        impl<Ms> seed::virtual_dom::UpdateEl<Ms> for $name {
            fn update_el(self, el: &mut seed::virtual_dom::El<Ms>) {
                el.children.push(seed::virtual_dom::Node::Text(seed::virtual_dom::Text::new(<Self as $crate::Localizable>::localize(self.clone()).to_string())));
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", <Self as $crate::Localizable>::localize(self.clone()).to_string())
            }
        }
    };
}

#[cfg(feature = "web-sys")]
pub fn web_sys_set_locales() {
    let locales = web_sys::window()
        .unwrap()
        .navigator()
        .languages()
        .iter()
        .map(|v| v.as_string().unwrap())
        .chain(
            web_sys::window()
                .unwrap()
                .navigator()
                .language(),
        )
        .flat_map(|s| Locale::from_str(&s))
        .collect::<Vec<Locale>>();

    set_locales(locales.as_slice());
}
