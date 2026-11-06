//! What an administrator sets in the console: when pages are checked and scanned for, and the
//! model that reads them. The extraction model's address and key are the operator's, in the
//! environment (`AGG_LLM_URL`, `AGG_LLM_KEY`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schedule::parse_cron;

/// How the weekly scan looks for pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ScanSettings {
    /// How many candidate pages per vendor the scan opens in the browser and scores.
    pub max_pages_per_vendor: u32,
    /// The score a page needs to be proposed. A price per million tokens counts 2, a model the
    /// catalogue knows 3, a keyword in the address or title 4.
    pub min_score: f64,
    /// Words in an address or title that make a page worth opening.
    pub keywords: Vec<String>,
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            max_pages_per_vendor: 12,
            min_score: 10.0,
            keywords: ["pricing", "price", "models", "rates", "billing"].map(String::from).to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Settings {
    /// Whether the check and the scan run on their schedules.
    pub enabled: bool,
    /// When the confirmed pages are checked. A cron expression with five fields, as standard cron
    /// reads it: days of the week from 0 (Sunday) to 7 (Sunday again) or by name. It may restrict
    /// the day of the month or the day of the week, not both.
    pub schedule: String,
    /// When the vendors' sites are scanned for pages, in the same form. Weekly by default.
    pub scan_schedule: String,
    /// The IANA time zone the schedules are read in.
    pub time_zone: String,
    /// The model that reads the pages, by the name its endpoint takes: a chat model with
    /// structured output. Pages are not read until one is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_model: Option<String>,
    /// How much the extraction model takes in at once, for the size of the parts of a long page.
    pub extraction_context_window: u32,
    /// How many calls of the extraction model one run may make. A long page takes several.
    pub max_extractions_per_run: u32,
    pub scan: ScanSettings,
    /// The vendors whose prices are taken from OpenRouter's list. For others, open-weight models
    /// in particular, its price is usually a third party's. Its properties apply for every vendor.
    pub openrouter_price_vendors: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // On, so that a new installation scans for pages at once and an administrator only
            // confirms them. Nothing is read by a model before a page is confirmed and a model set.
            enabled: true,
            // Daily, an hour before a gateway's sync at its default hour.
            schedule: "17 2 * * *".into(),
            // Mondays, after the daily check.
            scan_schedule: "41 4 * * 1".into(),
            time_zone: "UTC".into(),
            extraction_model: None,
            extraction_context_window: 200_000,
            max_extractions_per_run: 60,
            scan: ScanSettings::default(),
            openrouter_price_vendors: ["openai", "anthropic", "google", "xai"].map(String::from).to_vec(),
        }
    }
}

impl Settings {
    /// What is wrong with the settings, in a sentence.
    pub fn check(&self) -> Result<(), String> {
        parse_cron(&self.schedule).map_err(|e| format!("schedule: {e}"))?;
        parse_cron(&self.scan_schedule).map_err(|e| format!("scan_schedule: {e}"))?;
        if self.time_zone.parse::<chrono_tz::Tz>().is_err() {
            return Err(format!("\"{}\" is not an IANA time zone", self.time_zone));
        }
        if !(1..=1000).contains(&self.max_extractions_per_run) {
            return Err("max_extractions_per_run must be between 1 and 1000".into());
        }
        if !(8_000..=10_000_000).contains(&self.extraction_context_window) {
            return Err("extraction_context_window must be between 8000 and 10000000".into());
        }
        let scan = &self.scan;
        if !(1..=100).contains(&scan.max_pages_per_vendor) || !scan.min_score.is_finite() || scan.min_score < 0.0 {
            return Err("scan: max_pages_per_vendor is 1 to 100, min_score a number from 0".into());
        }
        if scan.keywords.len() > 32 || scan.keywords.iter().any(|k| k.trim().is_empty() || k.chars().count() > 40) {
            return Err("scan.keywords takes up to 32 words of at most 40 characters".into());
        }
        Ok(())
    }

    /// Whether a model can read pages.
    pub fn can_extract(&self) -> bool {
        self.extraction_model.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_hold_and_nonsense_does_not() {
        assert_eq!(Settings::default().check(), Ok(()));
        let bad = |change: fn(&mut Settings)| {
            let mut s = Settings::default();
            change(&mut s);
            assert!(s.check().is_err(), "{s:?}");
        };
        bad(|s| s.schedule = "every day".into());
        bad(|s| s.scan_schedule = "61 * * * *".into());
        bad(|s| s.time_zone = "Mars/Olympus".into());
        bad(|s| s.max_extractions_per_run = 0);
        bad(|s| s.extraction_context_window = 10);
        bad(|s| s.scan.min_score = -1.0);
        bad(|s| s.scan.keywords = vec![" ".into()]);
    }

    #[test]
    fn a_stored_document_may_lack_what_a_later_release_added() {
        let s: Settings = serde_json::from_str(r#"{"enabled": false}"#).unwrap();
        assert!(!s.enabled);
        assert_eq!(s.schedule, Settings::default().schedule);
    }
}
