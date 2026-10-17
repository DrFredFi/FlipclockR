//! Date line formatting: `Weekday, D Month YYYY` in the selected date language.

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DateLanguage {
    #[default]
    #[serde(rename = "it-IT")]
    Italian,
    #[serde(rename = "en-GB")]
    English,
    #[serde(rename = "es-ES")]
    Spanish,
}

impl DateLanguage {
    pub const ALL: [DateLanguage; 3] = [Self::Italian, Self::English, Self::Spanish];

    /// The language's own name, for pickers.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::Italian => "Italiano",
            Self::English => "English",
            Self::Spanish => "Español",
        }
    }

    fn weekdays(self) -> [&'static str; 7] {
        match self {
            Self::Italian => [
                "Lunedì",
                "Martedì",
                "Mercoledì",
                "Giovedì",
                "Venerdì",
                "Sabato",
                "Domenica",
            ],
            Self::English => [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ],
            Self::Spanish => [
                "Lunes",
                "Martes",
                "Miércoles",
                "Jueves",
                "Viernes",
                "Sábado",
                "Domingo",
            ],
        }
    }

    fn months(self) -> [&'static str; 12] {
        match self {
            Self::Italian => [
                "Gennaio",
                "Febbraio",
                "Marzo",
                "Aprile",
                "Maggio",
                "Giugno",
                "Luglio",
                "Agosto",
                "Settembre",
                "Ottobre",
                "Novembre",
                "Dicembre",
            ],
            Self::English => [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
            Self::Spanish => [
                "Enero",
                "Febrero",
                "Marzo",
                "Abril",
                "Mayo",
                "Junio",
                "Julio",
                "Agosto",
                "Septiembre",
                "Octubre",
                "Noviembre",
                "Diciembre",
            ],
        }
    }
}

/// Format `date` as `Weekday, D Month YYYY`, e.g. `Giovedì, 15 Ottobre 2026`.
pub fn format_date(language: DateLanguage, date: NaiveDate) -> String {
    let weekday = language.weekdays()[weekday_index(date.weekday())];
    let month = language.months()[date.month0() as usize];
    format!("{weekday}, {} {month} {}", date.day(), date.year())
}

fn weekday_index(weekday: Weekday) -> usize {
    weekday.num_days_from_monday() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oct_15_2026() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
    }

    #[test]
    fn formats_in_italian() {
        assert_eq!(
            format_date(DateLanguage::Italian, oct_15_2026()),
            "Giovedì, 15 Ottobre 2026"
        );
    }

    #[test]
    fn formats_in_english() {
        assert_eq!(
            format_date(DateLanguage::English, oct_15_2026()),
            "Thursday, 15 October 2026"
        );
    }

    #[test]
    fn formats_in_spanish() {
        assert_eq!(
            format_date(DateLanguage::Spanish, oct_15_2026()),
            "Jueves, 15 Octubre 2026"
        );
    }

    #[test]
    fn single_digit_days_have_no_padding() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert_eq!(
            format_date(DateLanguage::English, d),
            "Sunday, 1 March 2026"
        );
    }

    #[test]
    fn languages_use_locale_tags_in_config() {
        #[derive(Serialize)]
        struct W {
            l: DateLanguage,
        }
        let s = toml::to_string(&W {
            l: DateLanguage::Spanish,
        })
        .unwrap();
        assert_eq!(s.trim(), "l = \"es-ES\"");
    }
}
