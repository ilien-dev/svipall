//! What a machine in a given country looks like.
//!
//! Routing a request through another country and leaving the timezone, the language list and the
//! locale pointing at the machine that made it is a contradiction anyone can check in one line of
//! JavaScript: `Intl.DateTimeFormat().resolvedOptions().timeZone` against the address the request
//! came from. It is one of the cheapest cross-checks there is, and svipall was failing it for every
//! proxied domain.
//!
//! Resolving a proxy's country automatically would mean asking a geolocation service, which is a
//! third party and a network call this project does not make. So the country is declared — by the
//! caller, per route — and everything else is derived from this table, offline.
//!
//! The table is deliberately small: the countries residential and datacentre exits are actually
//! sold in. An unknown country falls back to `None` and the identity keeps its configured values,
//! which is honest rather than guessing a plausible-looking wrong answer.

/// The locale surfaces a country implies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// ISO 3166-1 alpha-2, uppercase.
    pub country: &'static str,
    /// IANA zone for the country's main population centre. Countries with several zones get the
    /// one an exit node is most likely to be in, not the geographic centre.
    pub timezone: &'static str,
    /// BCP-47 tag for the primary language as it is actually written there.
    pub locale: &'static str,
    /// `Accept-Language`, and therefore `navigator.languages`. English trails most of them
    /// because most of the world's browsers are configured that way.
    pub accept_language: &'static str,
}

const REGIONS: &[Region] = &[
    Region {
        country: "US",
        timezone: "America/New_York",
        locale: "en-US",
        accept_language: "en-US,en;q=0.9",
    },
    Region {
        country: "CA",
        timezone: "America/Toronto",
        locale: "en-CA",
        accept_language: "en-CA,en;q=0.9,fr-CA;q=0.8",
    },
    Region {
        country: "MX",
        timezone: "America/Mexico_City",
        locale: "es-MX",
        accept_language: "es-MX,es;q=0.9,en;q=0.8",
    },
    Region {
        country: "BR",
        timezone: "America/Sao_Paulo",
        locale: "pt-BR",
        accept_language: "pt-BR,pt;q=0.9,en;q=0.8",
    },
    Region {
        country: "AR",
        timezone: "America/Argentina/Buenos_Aires",
        locale: "es-AR",
        accept_language: "es-AR,es;q=0.9,en;q=0.8",
    },
    Region {
        country: "CL",
        timezone: "America/Santiago",
        locale: "es-CL",
        accept_language: "es-CL,es;q=0.9,en;q=0.8",
    },
    Region {
        country: "CO",
        timezone: "America/Bogota",
        locale: "es-CO",
        accept_language: "es-CO,es;q=0.9,en;q=0.8",
    },
    Region {
        country: "GB",
        timezone: "Europe/London",
        locale: "en-GB",
        accept_language: "en-GB,en;q=0.9",
    },
    Region {
        country: "IE",
        timezone: "Europe/Dublin",
        locale: "en-IE",
        accept_language: "en-IE,en;q=0.9",
    },
    Region {
        country: "ES",
        timezone: "Europe/Madrid",
        locale: "es-ES",
        accept_language: "es-ES,es;q=0.9,en;q=0.8",
    },
    Region {
        country: "PT",
        timezone: "Europe/Lisbon",
        locale: "pt-PT",
        accept_language: "pt-PT,pt;q=0.9,en;q=0.8",
    },
    Region {
        country: "FR",
        timezone: "Europe/Paris",
        locale: "fr-FR",
        accept_language: "fr-FR,fr;q=0.9,en;q=0.8",
    },
    Region {
        country: "DE",
        timezone: "Europe/Berlin",
        locale: "de-DE",
        accept_language: "de-DE,de;q=0.9,en;q=0.8",
    },
    Region {
        country: "IT",
        timezone: "Europe/Rome",
        locale: "it-IT",
        accept_language: "it-IT,it;q=0.9,en;q=0.8",
    },
    Region {
        country: "NL",
        timezone: "Europe/Amsterdam",
        locale: "nl-NL",
        accept_language: "nl-NL,nl;q=0.9,en;q=0.8",
    },
    Region {
        country: "BE",
        timezone: "Europe/Brussels",
        locale: "nl-BE",
        accept_language: "nl-BE,nl;q=0.9,fr-BE;q=0.8,en;q=0.7",
    },
    Region {
        country: "CH",
        timezone: "Europe/Zurich",
        locale: "de-CH",
        accept_language: "de-CH,de;q=0.9,fr-CH;q=0.8,en;q=0.7",
    },
    Region {
        country: "AT",
        timezone: "Europe/Vienna",
        locale: "de-AT",
        accept_language: "de-AT,de;q=0.9,en;q=0.8",
    },
    Region {
        country: "SE",
        timezone: "Europe/Stockholm",
        locale: "sv-SE",
        accept_language: "sv-SE,sv;q=0.9,en;q=0.8",
    },
    Region {
        country: "NO",
        timezone: "Europe/Oslo",
        locale: "nb-NO",
        accept_language: "nb-NO,nb;q=0.9,en;q=0.8",
    },
    Region {
        country: "DK",
        timezone: "Europe/Copenhagen",
        locale: "da-DK",
        accept_language: "da-DK,da;q=0.9,en;q=0.8",
    },
    Region {
        country: "FI",
        timezone: "Europe/Helsinki",
        locale: "fi-FI",
        accept_language: "fi-FI,fi;q=0.9,en;q=0.8",
    },
    Region {
        country: "PL",
        timezone: "Europe/Warsaw",
        locale: "pl-PL",
        accept_language: "pl-PL,pl;q=0.9,en;q=0.8",
    },
    Region {
        country: "CZ",
        timezone: "Europe/Prague",
        locale: "cs-CZ",
        accept_language: "cs-CZ,cs;q=0.9,en;q=0.8",
    },
    Region {
        country: "RO",
        timezone: "Europe/Bucharest",
        locale: "ro-RO",
        accept_language: "ro-RO,ro;q=0.9,en;q=0.8",
    },
    Region {
        country: "TR",
        timezone: "Europe/Istanbul",
        locale: "tr-TR",
        accept_language: "tr-TR,tr;q=0.9,en;q=0.8",
    },
    Region {
        country: "RU",
        timezone: "Europe/Moscow",
        locale: "ru-RU",
        accept_language: "ru-RU,ru;q=0.9,en;q=0.8",
    },
    Region {
        country: "UA",
        timezone: "Europe/Kyiv",
        locale: "uk-UA",
        accept_language: "uk-UA,uk;q=0.9,ru;q=0.8,en;q=0.7",
    },
    Region {
        country: "IN",
        timezone: "Asia/Kolkata",
        locale: "en-IN",
        accept_language: "en-IN,en;q=0.9,hi;q=0.8",
    },
    Region {
        country: "JP",
        timezone: "Asia/Tokyo",
        locale: "ja-JP",
        accept_language: "ja-JP,ja;q=0.9,en;q=0.8",
    },
    Region {
        country: "KR",
        timezone: "Asia/Seoul",
        locale: "ko-KR",
        accept_language: "ko-KR,ko;q=0.9,en;q=0.8",
    },
    Region {
        country: "CN",
        timezone: "Asia/Shanghai",
        locale: "zh-CN",
        accept_language: "zh-CN,zh;q=0.9,en;q=0.8",
    },
    Region {
        country: "HK",
        timezone: "Asia/Hong_Kong",
        locale: "zh-HK",
        accept_language: "zh-HK,zh;q=0.9,en;q=0.8",
    },
    Region {
        country: "TW",
        timezone: "Asia/Taipei",
        locale: "zh-TW",
        accept_language: "zh-TW,zh;q=0.9,en;q=0.8",
    },
    Region {
        country: "SG",
        timezone: "Asia/Singapore",
        locale: "en-SG",
        accept_language: "en-SG,en;q=0.9,zh;q=0.8",
    },
    Region {
        country: "ID",
        timezone: "Asia/Jakarta",
        locale: "id-ID",
        accept_language: "id-ID,id;q=0.9,en;q=0.8",
    },
    Region {
        country: "TH",
        timezone: "Asia/Bangkok",
        locale: "th-TH",
        accept_language: "th-TH,th;q=0.9,en;q=0.8",
    },
    Region {
        country: "VN",
        timezone: "Asia/Ho_Chi_Minh",
        locale: "vi-VN",
        accept_language: "vi-VN,vi;q=0.9,en;q=0.8",
    },
    Region {
        country: "PH",
        timezone: "Asia/Manila",
        locale: "en-PH",
        accept_language: "en-PH,en;q=0.9,fil;q=0.8",
    },
    Region {
        country: "AE",
        timezone: "Asia/Dubai",
        locale: "ar-AE",
        accept_language: "ar-AE,ar;q=0.9,en;q=0.8",
    },
    Region {
        country: "IL",
        timezone: "Asia/Jerusalem",
        locale: "he-IL",
        accept_language: "he-IL,he;q=0.9,en;q=0.8",
    },
    Region {
        country: "SA",
        timezone: "Asia/Riyadh",
        locale: "ar-SA",
        accept_language: "ar-SA,ar;q=0.9,en;q=0.8",
    },
    Region {
        country: "ZA",
        timezone: "Africa/Johannesburg",
        locale: "en-ZA",
        accept_language: "en-ZA,en;q=0.9",
    },
    Region {
        country: "NG",
        timezone: "Africa/Lagos",
        locale: "en-NG",
        accept_language: "en-NG,en;q=0.9",
    },
    Region {
        country: "EG",
        timezone: "Africa/Cairo",
        locale: "ar-EG",
        accept_language: "ar-EG,ar;q=0.9,en;q=0.8",
    },
    Region {
        country: "AU",
        timezone: "Australia/Sydney",
        locale: "en-AU",
        accept_language: "en-AU,en;q=0.9",
    },
    Region {
        country: "NZ",
        timezone: "Pacific/Auckland",
        locale: "en-NZ",
        accept_language: "en-NZ,en;q=0.9",
    },
];

/// The region a country code implies, if it is one we know.
///
/// Returns `None` for anything unknown rather than guessing: an identity that keeps its configured
/// timezone is merely unproxied-looking, while one wearing the wrong country is actively wrong.
pub fn for_country(code: &str) -> Option<&'static Region> {
    let code = code.trim();
    if code.len() != 2 {
        return None;
    }
    REGIONS
        .iter()
        .find(|r| r.country.eq_ignore_ascii_case(code))
}

/// Every country the table covers, for error messages and `web_status`.
pub fn known_countries() -> impl Iterator<Item = &'static str> {
    REGIONS.iter().map(|r| r.country)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_country_resolves_to_a_consistent_set() {
        let de = for_country("DE").expect("DE is covered");
        assert_eq!(de.timezone, "Europe/Berlin");
        assert_eq!(de.locale, "de-DE");
        assert!(de.accept_language.starts_with("de-DE"));
    }

    #[test]
    fn the_lookup_does_not_care_about_case_or_padding() {
        assert_eq!(for_country("de"), for_country("DE"));
        assert_eq!(for_country("  de "), for_country("DE"));
    }

    #[test]
    fn an_unknown_country_is_none_rather_than_a_guess() {
        // Wearing the wrong country is worse than wearing none: the first is a contradiction, the
        // second just looks unproxied.
        assert!(for_country("ZZ").is_none());
        assert!(for_country("").is_none());
        assert!(for_country("DEU").is_none(), "alpha-3 is not alpha-2");
    }

    #[test]
    fn every_row_is_internally_consistent() {
        for r in REGIONS {
            assert_eq!(r.country.len(), 2, "{} is not alpha-2", r.country);
            assert_eq!(
                r.country,
                r.country.to_uppercase(),
                "{} is not uppercase",
                r.country
            );
            assert!(
                r.timezone.contains('/'),
                "{} is not an IANA zone",
                r.timezone
            );
            // The locale's region has to be the country, or the two disagree with each other and
            // the whole point is lost.
            let region = r.locale.split('-').next_back().unwrap_or("");
            assert_eq!(
                region, r.country,
                "locale {} does not belong to {}",
                r.locale, r.country
            );
            // Accept-Language must lead with the locale, which is what a browser configured in
            // that country actually sends.
            assert!(
                r.accept_language.starts_with(r.locale),
                "{} does not lead with {}",
                r.accept_language,
                r.locale
            );
        }
    }

    #[test]
    fn no_country_appears_twice() {
        let mut seen: Vec<&str> = REGIONS.iter().map(|r| r.country).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "a duplicate row would shadow the other");
    }

    #[test]
    fn the_places_proxies_are_actually_sold_are_covered() {
        for c in ["US", "GB", "DE", "FR", "ES", "BR", "IN", "JP", "SG", "AU"] {
            assert!(for_country(c).is_some(), "{c} is missing");
        }
        assert!(known_countries().count() >= 40);
    }
}
