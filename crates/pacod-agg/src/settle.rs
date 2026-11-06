//! What the pages of a vendor say, settled into the catalogue.
//!
//! A vendor settles once its pages of a run were checked, from the last successful reading of
//! every active page: settling after each page would publish a mix of new and old readings, and a
//! page that did not change or could not be read this time still says what it said. Pages are
//! taken by priority: the first page that states a price or a property wins, so that a vendor's
//! own page goes before a list.
//!
//! What a consumer relies on:
//!
//! - A price's `checked_at` is when its page was last read as it should, from the page's own
//!   record. An unchanged page moves it on; a failed fetch does not. A consumer counts a price as
//!   stale from this date.
//! - A page that failed takes no model away. A model counts as gone only when every page of the
//!   vendor was read, and none names it.
//! - A vendor is published as complete only when an administrator says its pages list everything
//!   and every one of them was read at its last check.
//! - `api_id_confirmed` comes from a vendor's own page alone. A list holds every model anyone ever
//!   priced, so what is known from a list alone is not proposed to gateways.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use pacod_agg_catalog::ids::{model_slug, normalise_model_id};
use pacod_agg_catalog::{ModelKind, Parameter, Price, Properties, Provenance};
use pacod_agg_jobs::extraction::{PageExtraction, PageModel};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::api::{ApiError, ApiResult, AppState};
use crate::settings::Settings;
use crate::store::models::{ModelRow, ModelWrite, PriceRow, PriceWrite};
use crate::store::pages::{Page, PageState};

/// The modalities a page may give a model. Anything else is a misreading.
const MODALITIES: &[&str] = &["text", "image", "document", "audio", "video", "embedding", "ranking"];

/// What came of one model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settled {
    pub model: String,
    /// `created`, `changed` (its price or a property) or `unchanged`.
    pub outcome: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// One model as the vendor's pages describe it together.
#[derive(Debug, Default)]
struct Merged {
    /// The id, as the vendor's page or else a list writes it.
    api_id: String,
    aliases: BTreeSet<String>,
    confirmed: bool,
    display_name: Option<String>,
    kind: Option<ModelKind>,
    properties: Properties,
    parameters: BTreeMap<String, Parameter>,
    /// The price, with when its page was last read as it should.
    price: Option<(Price, DateTime<Utc>)>,
    provenance: BTreeMap<String, Provenance>,
}

impl Merged {
    /// Every way this model is written.
    fn names(&self) -> impl Iterator<Item = String> + '_ {
        let id = normalise_model_id(&self.api_id);
        std::iter::once(id).chain(self.aliases.iter().map(|a| normalise_model_id(a)))
    }

    /// Whether a page's naming of a model is this model: by an id, or by the name written as a
    /// slug (`GPT-5 mini` for `gpt-5-mini`).
    fn is(&self, m: &PageModel) -> bool {
        let id = normalise_model_id(&m.model);
        let slug = model_slug(&m.model);
        let display = m.display_name.as_deref().map(model_slug);
        let own_display = self.display_name.as_deref().map(model_slug);
        self.names().any(|n| n == id || model_slug(&n) == slug || display.as_deref() == Some(model_slug(&n).as_str()))
            || own_display.as_deref() == Some(slug.as_str())
            || (own_display.is_some() && own_display == display)
    }

    /// What a page adds to what the pages before it said.
    fn add(&mut self, m: &PageModel, stated_by_vendor: bool, page: &Page, method: &str, read_at: DateTime<Utc>) {
        let stated = stated_by_vendor && m.api_id_stated;
        // The id a vendor states goes before a name, or another page's spelling.
        if self.api_id.is_empty() || (stated && !self.confirmed) {
            if !self.api_id.is_empty() && normalise_model_id(&self.api_id) != normalise_model_id(&m.model) {
                self.aliases.insert(std::mem::take(&mut self.api_id));
            }
            self.api_id.clone_from(&m.model);
        }
        self.confirmed |= stated;
        self.aliases.extend(m.aliases.iter().cloned());
        if self.display_name.is_none() {
            self.display_name.clone_from(&m.display_name);
        }
        if self.kind.is_none() {
            self.kind = m.kind;
        }
        let known = fields(&self.properties);
        self.properties.fill_from(&clean(&m.properties));
        for field in fields(&self.properties).into_iter().filter(|f| !known.contains(f)) {
            self.provenance.insert(field, provenance(page, method, read_at));
        }
        for (name, p) in &m.parameters {
            self.parameters.entry(name.clone()).or_insert_with(|| p.clone());
        }
        if self.price.is_none()
            && let Some(p) = m.price.as_ref().filter(|p| valid_price(p))
        {
            self.price = Some((p.clone(), read_at));
            self.provenance.insert("price".into(), provenance(page, method, read_at));
        }
    }
}

fn provenance(page: &Page, method: &str, read_at: DateTime<Utc>) -> Provenance {
    Provenance { method: Some(method.to_owned()), url: Some(page.url.clone()), checked_at: Some(read_at) }
}

/// The names of the properties that say something.
fn fields(p: &Properties) -> BTreeSet<String> {
    match serde_json::to_value(p) {
        Ok(Value::Object(map)) => map.into_iter().map(|(k, _)| k).collect(),
        _ => BTreeSet::new(),
    }
}

/// A price that can be one: a misreading (a price per thousand tokens taken for one per million)
/// is left out, not published.
fn valid_price(p: &Price) -> bool {
    !p.is_empty()
        && p.currency.len() == 3
        && p.rates.implausible().is_none()
        && p.variants.values().all(|r| r.implausible().is_none())
        && p.tiers.iter().all(|t| t.rates.implausible().is_none())
}

/// The properties without what cannot be true of a model at all: a price read as a context
/// window, a modality nobody knows.
fn clean(p: &Properties) -> Properties {
    let mut p = p.clone();
    p.context_window = p.context_window.filter(|n| (1_000..=100_000_000).contains(n));
    let window = p.context_window;
    p.max_output_tokens =
        p.max_output_tokens.filter(|n| (1..=10_000_000).contains(n) && window.is_none_or(|w| *n <= w));
    if let Some(m) = &mut p.modalities {
        let known = |list: &[String]| list.iter().all(|x| MODALITIES.contains(&x.as_str()));
        if !known(&m.input) {
            m.input.clear();
        }
        if !known(&m.output) {
            m.output.clear();
        }
    }
    if p.modalities.as_ref().is_some_and(|m| m.input.is_empty() && m.output.is_empty()) {
        p.modalities = None;
    }
    p
}

/// The kind of a model a page does not name one for, from what it says about it.
fn infer_kind(p: &Properties, price: Option<&Price>) -> Option<ModelKind> {
    let output = p.modalities.as_ref().map(|m| m.output.as_slice()).unwrap_or_default();
    let has = |word: &str| output.iter().any(|m| m == word);
    let rates = price.map(|p| &p.rates);
    let rate = |f: fn(&pacod_agg_catalog::Rates) -> Option<f64>| rates.and_then(f).is_some();
    if has("embedding") {
        Some(ModelKind::Embedding)
    } else if has("ranking") || rate(|r| r.per_search_unit) {
        Some(ModelKind::Rerank)
    } else if rate(|r| r.per_image) {
        Some(ModelKind::Image)
    } else if rate(|r| r.per_audio_second) {
        Some(ModelKind::Transcription)
    } else if rate(|r| r.per_character) {
        Some(ModelKind::Speech)
    } else if rate(|r| r.output) {
        Some(ModelKind::Chat)
    } else {
        None
    }
}

/// The price without what changes with every reading: two readings are the same price when this
/// is the same.
fn figures(p: &Price) -> Price {
    Price { effective_from: None, changed_at: None, checked_at: None, evidence: None, ..p.clone() }
}

/// Whether a page counts as read: it answered, and what it says is known.
fn read(state: &PageState) -> bool {
    matches!(state.outcome.as_str(), "read" | "unchanged")
}

/// The stored row of a merged model.
fn row_of<'a>(rows: &'a [ModelRow], m: &Merged) -> Option<&'a ModelRow> {
    let names: BTreeSet<String> = m.names().collect();
    let slugs: BTreeSet<String> = names.iter().map(|n| model_slug(n)).collect();
    rows.iter().find(|r| names.contains(&r.key)).or_else(|| {
        rows.iter().find(|r| {
            slugs.contains(&model_slug(&r.key)) || r.aliases.iter().any(|a| names.contains(&normalise_model_id(a)))
        })
    })
}

fn name_of(v: &impl Serialize) -> Option<String> {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned))
}

fn object(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

/// What changed between two sets of properties, field by field.
fn differences(old: &Value, new: &Value) -> Vec<(String, Option<Value>, Option<Value>)> {
    let (old, new) = (object(old), object(new));
    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    keys.into_iter()
        .filter(|k| old.get(*k) != new.get(*k))
        .map(|k| (k.clone(), old.get(k).cloned(), new.get(k).cloned()))
        .collect()
}

/// What the vendor's pages say together, by priority. The pages come sorted.
fn merge_pages(pages: &[&Page], states: &HashMap<String, PageState>, settings: &Settings) -> Vec<Merged> {
    let mut merged: Vec<Merged> = Vec::new();
    for page in pages {
        let Some(state) = states.get(&page.id) else { continue };
        // Never read: nothing to say. Read once and failed since: it says what it last said, as
        // of when it last read as it should.
        let Some(read_at) = state.last_ok_at else { continue };
        let Ok(extraction) = serde_json::from_value::<PageExtraction>(state.extraction.clone()) else { continue };
        let by_vendor = page.kind == "page";
        let method = if by_vendor { "rendered_page" } else { "feed" };
        // OpenRouter's price for other vendors is usually a third party's.
        let priced = page.format != "openrouter" || settings.openrouter_price_vendors.contains(&page.vendor_id);
        for m in &extraction.models {
            let m = PageModel { price: m.price.clone().filter(|_| priced), ..m.clone() };
            match merged.iter_mut().find(|x| x.is(&m)) {
                Some(found) => found.add(&m, by_vendor, page, method, read_at),
                None => {
                    let mut new = Merged::default();
                    new.add(&m, by_vendor, page, method, read_at);
                    merged.push(new);
                }
            }
        }
    }
    merged
}

/// Writes what a vendor's active pages say into the catalogue.
pub async fn settle_vendor(st: &AppState, vendor_id: &str, run_id: Option<&str>) -> ApiResult<Vec<Settled>> {
    let (settings, _) = st.settings().await?;
    let pages = st.store.pages(Some(vendor_id), Some("active")).await?;
    let states = st.store.page_states().await?;
    let pages: Vec<&Page> = pages.iter().collect();
    let merged = merge_pages(&pages, &states, &settings);

    let rows = st.store.models_of(vendor_id).await?;
    let prices: HashMap<String, PriceRow> =
        st.store.prices_of(vendor_id).await?.into_iter().map(|p| (p.model_id.clone(), p)).collect();
    let mut writes: Vec<ModelWrite> = Vec::new();
    let mut out = Vec::new();
    let mut named: BTreeSet<String> = BTreeSet::new();
    for m in merged {
        let row = row_of(&rows, &m);
        if let Some(r) = row {
            // Two namings of one row come to one write: the first page's.
            if !named.insert(r.id.clone()) {
                continue;
            }
        }
        let kind = m.kind.or_else(|| infer_kind(&m.properties, m.price.as_ref().map(|(p, _)| p)));
        let properties = stored(&m.properties)?;
        let parameters = stored(&m.parameters)?;
        let mut changes = Vec::new();
        match row {
            None => changes.push(("created".to_owned(), None, Some(json!(m.api_id)))),
            Some(r) => {
                changes.extend(differences(&r.properties, &properties));
                if r.gone_at.is_some() {
                    changes.push(("back".to_owned(), None, None));
                }
            }
        }
        let current = row.and_then(|r| prices.get(&r.id));
        let price = match (&m.price, current) {
            (None, _) => None,
            (Some((p, read_at)), Some(c)) => {
                let same =
                    serde_json::from_value::<Price>(c.price.clone()).is_ok_and(|was| figures(&was) == figures(p));
                if same {
                    Some(PriceWrite::Confirm { checked_at: *read_at })
                } else {
                    let new = stored(p)?;
                    changes.push(("price".to_owned(), Some(c.price.clone()), Some(new.clone())));
                    Some(PriceWrite::New { price: new, checked_at: *read_at })
                }
            }
            (Some((p, read_at)), None) => Some(PriceWrite::New { price: stored(p)?, checked_at: *read_at }),
        };
        let key = normalise_model_id(&m.api_id);
        let was_confirmed = row.is_some_and(|r| r.api_id_confirmed);
        // An id a vendor stated stays, should a later reading only name the model.
        let keep_id = row.filter(|_| was_confirmed && !m.confirmed);
        let outcome = match (row, changes.is_empty()) {
            (None, _) => "created",
            (Some(_), false) => "changed",
            (Some(_), true) => "unchanged",
        };
        let detail = changes.iter().map(|(f, ..)| f.as_str()).collect::<Vec<_>>().join(", ");
        out.push(Settled { model: m.api_id.clone(), outcome: outcome.to_owned(), detail });
        let mut aliases: BTreeSet<String> = m.aliases.iter().cloned().collect();
        aliases.extend(row.iter().flat_map(|r| r.aliases.iter().cloned()));
        let api_id = keep_id.map_or(m.api_id.clone(), |r| r.api_id.clone());
        aliases.remove(&api_id);
        writes.push(ModelWrite {
            id: row.map(|r| r.id.clone()),
            key: keep_id.map_or(key, |r| r.key.clone()),
            api_id,
            aliases: aliases.into_iter().collect(),
            api_id_confirmed: m.confirmed || was_confirmed,
            display_name: m.display_name.clone().or_else(|| row.and_then(|r| r.display_name.clone())),
            kind: kind.and_then(|k| name_of(&k)),
            properties,
            parameters,
            provenance: stored(&m.provenance)?,
            price,
            changes,
        });
    }

    // A page that could not be read says nothing about what the vendor lists: a model goes only
    // when every page was read.
    let of_vendor: Vec<&&Page> = pages.iter().filter(|p| p.kind == "page").collect();
    let read_complete = !of_vendor.is_empty() && of_vendor.iter().all(|p| states.get(&p.id).is_some_and(read));
    let gone: Vec<String> = if read_complete {
        rows.iter().filter(|r| r.gone_at.is_none() && !named.contains(&r.id)).map(|r| r.id.clone()).collect()
    } else {
        Vec::new()
    };
    for r in rows.iter().filter(|r| gone.contains(&r.id)) {
        out.push(Settled { model: r.api_id.clone(), outcome: "gone".into(), detail: String::new() });
    }
    st.store.settle(vendor_id, &writes, &gone, read_complete, run_id).await?;
    out.sort_by(|a, b| a.model.cmp(&b.model));
    Ok(out)
}

/// A value as a write stores it.
fn stored(v: &impl Serialize) -> ApiResult<Value> {
    serde_json::to_value(v).map_err(|e| ApiError::invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use pacod_agg_catalog::{Modalities, Rates};

    use super::*;

    fn page_model(model: &str, display: Option<&str>) -> PageModel {
        PageModel { model: model.into(), display_name: display.map(str::to_owned), ..Default::default() }
    }

    #[test]
    fn a_page_names_a_model_by_id_or_by_name() {
        let model =
            Merged { api_id: "gpt-5-mini".into(), display_name: Some("GPT-5 mini".into()), ..Default::default() };
        assert!(model.is(&page_model("GPT-5-Mini", None)));
        assert!(model.is(&page_model("GPT-5 mini", None)));
        assert!(model.is(&page_model("mini", Some("GPT-5 mini"))));
        assert!(!model.is(&page_model("gpt-5", None)));
        assert!(!model.is(&page_model("gpt-5-mini-2025", None)));
    }

    #[test]
    fn facts_are_left_out_where_they_cannot_be_true() {
        let p = Properties {
            context_window: Some(15),
            max_output_tokens: Some(8192),
            modalities: Some(Modalities { input: vec!["text".into(), "smell".into()], output: vec!["text".into()] }),
            ..Properties::default()
        };
        let c = clean(&p);
        assert_eq!(c.context_window, None, "a price read as a window");
        assert_eq!(c.max_output_tokens, Some(8192));
        assert_eq!(c.modalities, Some(Modalities { input: vec![], output: vec!["text".into()] }));

        let p = Properties { context_window: Some(128_000), max_output_tokens: Some(256_000), ..Properties::default() };
        assert_eq!(clean(&p).max_output_tokens, None, "more output than fits the window");
    }

    #[test]
    fn a_price_must_be_one() {
        let price = |input: f64| Price { rates: Rates { input: Some(input), ..Rates::default() }, ..Price::default() };
        assert!(valid_price(&price(2.0)));
        assert!(!valid_price(&price(-1.0)));
        assert!(!valid_price(&price(50_000.0)), "a price per thousand read as one per million");
        assert!(!valid_price(&Price::default()), "no rate at all");
    }

    #[test]
    fn the_kind_follows_from_what_a_page_says() {
        let price = |r: Rates| Price { rates: r, ..Price::default() };
        let embedding = Properties {
            modalities: Some(Modalities { input: vec!["text".into()], output: vec!["embedding".into()] }),
            ..Properties::default()
        };
        assert_eq!(infer_kind(&embedding, None), Some(ModelKind::Embedding));
        let chat = price(Rates { input: Some(1.0), output: Some(2.0), ..Rates::default() });
        assert_eq!(infer_kind(&Properties::default(), Some(&chat)), Some(ModelKind::Chat));
        let image = price(Rates { per_image: Some(0.04), ..Rates::default() });
        assert_eq!(infer_kind(&Properties::default(), Some(&image)), Some(ModelKind::Image));
        assert_eq!(infer_kind(&Properties::default(), None), None);
    }
}
