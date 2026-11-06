//! What the aggregator reads on a page: the scraper's account of one page of a check run, and
//! what a page says about each model it lists, in the catalogue's own types.
//!
//! The scraper renders a page and fingerprints it; when the page changed, the server has its text
//! read by the extraction model and turns the answer into [`PageExtraction`] here. A feed is read
//! by the scraper itself into the same shape.
//!
//! Copied from pacod-gw (commit 50fe4b7, `crates/pacod-gw-events/src/extraction.rs`).

use std::collections::BTreeMap;

use chrono::Utc;
use pacod_agg_catalog::ids::normalise_model_id;
use pacod_agg_catalog::{Evidence, Modalities, Price, Properties, Rates, Tier};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hours::{StatedHours, canonical_tiers, off_peak_windows};

/// What the scraper found at one page of a check run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageReading {
    pub page_id: String,
    /// `changed` (the fingerprint differs from the last reading, or the page is to be read
    /// anyway), `unchanged`, or `failed` (the page could not be fetched or rendered).
    pub outcome: String,
    pub http_status: Option<u16>,
    /// Where the page ended up after redirects.
    pub final_url: String,
    pub title: String,
    /// Over the lines that carry models, prices and limits: what decides whether a page is read.
    pub content_hash: String,
    /// Over the whole text, for diagnosis.
    pub text_hash: String,
    /// A page's text, sent when the page is to be read.
    pub text: Option<String>,
    /// A feed's models, which the scraper reads itself.
    pub extraction: Option<PageExtraction>,
    /// Why the page failed, in a sentence.
    pub message: String,
    pub elapsed_ms: u64,
    pub bytes: u64,
}

/// What a page says about the models it lists.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageExtraction {
    pub models: Vec<PageModel>,
    /// What could not be read, in sentences.
    pub notes: Vec<String>,
    /// How many calls of the extraction model the page took.
    pub chunks: u32,
}

/// One model as a page describes it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageModel {
    /// The model as the page names it: the id the vendor's API takes, where the page states one,
    /// or an id the catalogue knows the model by, or its name.
    pub model: String,
    /// Other ids the source gives the same model.
    pub aliases: Vec<String>,
    pub display_name: Option<String>,
    /// Whether the page states the id the vendor's API takes, rather than only a name. Only a
    /// vendor's own page can say so; a feed never does.
    pub api_id_stated: bool,
    pub kind: Option<pacod_agg_catalog::ModelKind>,
    pub price: Option<Price>,
    pub properties: Properties,
    pub parameters: BTreeMap<String, pacod_agg_catalog::Parameter>,
}

/// The models in what the extraction model answered for a page (`{"models": [...]}`): rates,
/// variants, long-context tiers, off-peak rates with their hours converted to UTC windows, and
/// properties. A part that cannot be read is left out and said so in the price's evidence; the
/// rest stays. A model the answer names twice is one model, the first naming filled from the
/// second.
pub fn models_from_answer(answer: &Value, url: &str) -> PageExtraction {
    let mut out = PageExtraction::default();
    for item in answer["models"].as_array().into_iter().flatten() {
        let named = item["model"].as_str().unwrap_or_default().trim();
        if named.is_empty() {
            out.notes.push("a model without a name was left out".into());
            continue;
        }
        let model = PageModel {
            model: named.to_owned(),
            aliases: Vec::new(),
            display_name: item["display_name"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned),
            api_id_stated: item["api_id_stated"].as_bool().unwrap_or(false),
            kind: None,
            price: price_of(item, url),
            properties: properties_of(item),
            parameters: BTreeMap::new(),
        };
        merge(&mut out.models, model);
    }
    out
}

/// Adds a model to a list: one the list names already fills what it left open.
pub fn merge(models: &mut Vec<PageModel>, model: PageModel) {
    let id = normalise_model_id(&model.model);
    match models.iter_mut().find(|m| normalise_model_id(&m.model) == id) {
        Some(first) => {
            first.properties.fill_from(&model.properties);
            if first.price.is_none() {
                first.price = model.price;
            }
        }
        None => models.push(model),
    }
}

fn properties_of(item: &Value) -> Properties {
    let modalities = |key: &str| -> Option<Vec<String>> {
        let list: Vec<String> = item[key]
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        (!list.is_empty()).then_some(list)
    };
    let (input, output) = (modalities("input_modalities"), modalities("output_modalities"));
    Properties {
        context_window: item["context_window"].as_u64().filter(|n| *n > 0),
        max_output_tokens: item["max_output_tokens"].as_u64().filter(|n| *n > 0),
        modalities: (input.is_some() || output.is_some())
            .then(|| Modalities { input: input.unwrap_or_default(), output: output.unwrap_or_default() }),
        vision: item["vision"].as_bool(),
        tools: item["tools"].as_bool(),
        structured_output: item["structured_output"].as_bool(),
        reasoning: item["reasons"].as_bool(),
        ..Properties::default()
    }
}

/// The price an answer states for one model, `None` when it states no rate.
fn price_of(item: &Value, url: &str) -> Option<Price> {
    let rates: Rates = serde_json::from_value(item.clone()).unwrap_or_default();
    let mut variants = BTreeMap::new();
    // The extraction names the variants it found as fixed properties.
    for (name, v) in item["variants"].as_object().into_iter().flatten() {
        let r: Rates = serde_json::from_value(v.clone()).unwrap_or_default();
        if !r.is_empty() {
            variants.insert(name.clone(), r);
        }
    }
    let mut notes = Vec::new();
    let stated: Vec<Tier> = item["tiers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some(Tier {
                above_input_tokens: t["above_input_tokens"].as_u64()?,
                rates: serde_json::from_value(t.clone()).ok()?,
                variants: BTreeMap::new(),
            })
        })
        .collect();
    let tiers = canonical_tiers(&stated);
    if tiers.len() < item["tiers"].as_array().map_or(0, Vec::len) {
        notes.push("a long-context tier without a threshold or rates was left out");
    }
    let mut windows = Vec::new();
    if let Some(off) = item["off_peak"].as_object() {
        let off_rates: Rates = serde_json::from_value(Value::Object(off.clone())).unwrap_or_default();
        let hours = |key: &str| off.get(key).and_then(|v| serde_json::from_value::<StatedHours>(v.clone()).ok());
        match off_peak_windows(hours("peak").as_ref(), hours("off_peak").as_ref()) {
            Some(w) if !off_rates.is_empty() => {
                variants.insert("off_peak".into(), off_rates);
                windows = w;
            }
            Some(_) => notes.push("off-peak hours without off-peak rates were left out"),
            None if !off_rates.is_empty() => notes.push("off-peak rates without readable hours were left out"),
            None => {}
        }
    }
    if rates.is_empty() && variants.is_empty() {
        return None;
    }
    let mut evidence: String = item["evidence"].as_str().unwrap_or_default().chars().take(2000).collect();
    for note in notes {
        evidence.push_str(&format!(" [{note}]"));
    }
    Some(Price {
        currency: item["currency"]
            .as_str()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .unwrap_or("USD")
            .to_ascii_uppercase(),
        rates,
        variants,
        tiers,
        windows,
        evidence: Some(Evidence {
            url: Some(url.to_owned()),
            text: Some(evidence),
            method: Some("rendered_page".into()),
            fetched_at: Some(Utc::now()),
        }),
        ..Price::default()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_answer_gives_prices_properties_and_hours_in_utc() {
        let answer = json!({ "models": [
            { "model": "deepseek-flash", "display_name": "DeepSeek Flash", "api_id_stated": true, "currency": "usd",
              "input": 0.3, "output": 1.2, "cache_read": 0.03, "reasoning": null,
              "variants": { "batch": { "input": 0.15, "output": 0.6 }, "priority": null, "flex": null },
              "tiers": [{ "above_input_tokens": 128000, "input": 0.6, "output": 2.4, "cache_read": null }, { "input": 9.0 }],
              "off_peak": { "input": 0.15, "output": 0.6, "cache_read": null,
                            "peak": { "weekdays": [1, 2, 3, 4, 5], "hours": ["09:00-18:00"], "utc_offset": "+08:00" },
                            "off_peak": null },
              "context_window": 128000, "max_output_tokens": 8192, "input_modalities": ["Text", " image "],
              "output_modalities": [], "vision": true, "tools": true, "structured_output": null, "reasons": false,
              "evidence": "$0.30 per 1M input tokens" },
            { "model": "DEEPSEEK-FLASH", "context_window": 1, "structured_output": true },
            { "model": "deepseek-embed", "api_id_stated": false, "input": null, "output": null },
            { "model": " ", "input": 1.0 }
        ] });
        let found = models_from_answer(&answer, "https://api-docs.deepseek.com/pricing");
        assert_eq!(found.models.len(), 2, "{found:?}");
        let flash = &found.models[0];
        let price = flash.price.as_ref().unwrap();
        assert_eq!(price.currency.as_str(), "USD");
        assert_eq!((price.rates.input, price.rates.cache_read), (Some(0.3), Some(0.03)));
        assert_eq!(price.variants["batch"].input, Some(0.15));
        assert_eq!(price.tiers.len(), 1);
        let evidence = price.evidence.as_ref().unwrap();
        assert!(evidence.text.as_ref().unwrap().contains("[a long-context tier"), "{evidence:?}");
        assert_eq!(evidence.url.as_deref(), Some("https://api-docs.deepseek.com/pricing"));
        // Peak hours 09:00 to 18:00 in Beijing are 01:00 to 10:00 UTC; off-peak is the rest.
        assert_eq!(price.variants["off_peak"].input, Some(0.15));
        assert!(price.windows.iter().any(|w| w.weekdays.contains(&6)), "{:?}", price.windows);
        assert_eq!(evidence.method.as_deref(), Some("rendered_page"));
        let p = &flash.properties;
        assert_eq!((p.context_window, p.max_output_tokens), (Some(128_000), Some(8192)), "the first naming wins");
        let modalities = p.modalities.as_ref().unwrap();
        assert_eq!(modalities.input, ["text", "image"]);
        assert!(modalities.output.is_empty());
        assert_eq!((p.vision, p.reasoning), (Some(true), Some(false)));
        assert_eq!(p.structured_output, Some(true), "filled from the second naming");
        // A model without rates has no price, and keeps what else the page says.
        assert!(found.models[1].price.is_none() && !found.models[1].api_id_stated);
        assert_eq!(found.notes.len(), 1);
    }
}
