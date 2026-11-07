//! Readings of pages. The scraper reports each page of a check run; when a page changed, the
//! server has its text read by the extraction model and keeps what the page says. Once every page
//! of a vendor in the run was checked, the vendor settles into the catalogue.
//!
//! The extraction model is reached through an OpenAI-compatible endpoint, by the mode in use: a
//! provider's own (`AGG_LLM_URL`), or a pacod-gw (`AGG_GW_URL`) with a key of its own, where its
//! cost is accounted like any other call. This is the only call of a model anywhere in the
//! aggregator.
//!
//! Moved from pacod-gw (commit 50fe4b7, `crates/pacod-gw-core/src/mgmt/readings.rs`).

use std::collections::HashSet;

use axum::Json;
use pacod_agg_jobs::extraction::{PageExtraction, PageReading, merge, models_from_answer};
use pacod_agg_jobs::jobs::JOB_CHECK;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppState};
use crate::settings::Settings;
use crate::settle::{Settled, settle_vendor};
use crate::store::jobs::JobRun;
use crate::store::pages::{Page, Recording};
use crate::store::vendors::Vendor;

/// The largest part of a page the extraction model is given at once, in characters.
const MAX_CHUNK_CHARS: usize = 120_000;
/// The smallest, whatever the model's context window.
const MIN_CHUNK_CHARS: usize = 8_000;
/// The output an extraction may take: a model page lists many models with many fields.
const EXTRACTION_OUTPUT_TOKENS: u32 = 16_000;
/// How long one call of the extraction model may take.
const EXTRACTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Takes the scraper's account of one page of a check run.
pub async fn reading(st: &AppState, run_id: &str, reading: &PageReading) -> ApiResult<Json<Value>> {
    let run = st.store.job_run(run_id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    if run.finished_at.is_some() {
        return Err(ApiError::conflict("this run is already finished"));
    }
    if run.job != JOB_CHECK || !run.covers.contains(&reading.page_id) {
        return Err(ApiError::conflict("the page is not one this run checks"));
    }
    let Some(page) = st.store.page(&reading.page_id).await?.filter(|p| p.status == "active") else {
        let entry =
            json!({ "page_id": reading.page_id, "outcome": "not_read", "message": "the page is no longer confirmed" });
        st.store.job_run_append(run_id, "pages", &json!([entry])).await?;
        return Ok(Json(entry));
    };
    let (settings, _) = st.settings().await?;
    let states = st.store.page_states().await?;
    let state = states.get(&page.id);
    let force = state.is_none_or(|s| s.force_read || s.last_read_at.is_none());
    let mut rec = Recording {
        http_status: reading.http_status.map(i32::from),
        final_url: reading.final_url.clone(),
        ..Default::default()
    };
    let read = |rec: &mut Recording, ex: &PageExtraction| -> ApiResult<()> {
        rec.outcome = "read".into();
        rec.content_hash = Some(reading.content_hash.clone());
        rec.text_hash = Some(reading.text_hash.clone());
        rec.extraction = Some(serde_json::to_value(ex).map_err(|e| ApiError::invalid(e.to_string()))?);
        if !ex.notes.is_empty() {
            rec.message = ex.notes.join("; ");
        }
        Ok(())
    };
    let mut models = 0;
    match (reading.outcome.as_str(), page.kind.as_str()) {
        ("failed", _) => {
            rec.outcome = "failed".into();
            rec.message = if reading.message.is_empty() {
                "the page could not be fetched".into()
            } else {
                reading.message.clone()
            };
        }
        (_, "feed") => match &reading.extraction {
            None => (rec.outcome, rec.message) = ("failed".into(), "the scraper sent no models for the list".into()),
            Some(_) if !force && state.is_some_and(|s| s.content_hash == reading.content_hash) => {
                rec.outcome = "unchanged".into();
            }
            Some(ex) => {
                models = ex.models.len();
                read(&mut rec, ex)?;
            }
        },
        ("unchanged", _) => rec.outcome = "unchanged".into(),
        _ => match reading.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            None => (rec.outcome, rec.message) = ("failed".into(), "the page has no text".into()),
            Some(_) if st.config.extraction_model(&settings).is_none() => {
                (rec.outcome, rec.message) = ("not_read".into(), "no extraction model is set".into());
            }
            Some(_) if st.config.link(&settings).is_none() => {
                (rec.outcome, rec.message) = ("not_read".into(), no_endpoint(st, &settings));
            }
            Some(text) => match extract_page(st, &settings, &run, &page, text).await {
                Ok(ex) => {
                    models = ex.models.len();
                    read(&mut rec, &ex)?;
                }
                Err(Unread::Budget(why)) => (rec.outcome, rec.message) = ("not_read".into(), why),
                Err(Unread::Failed(why)) => (rec.outcome, rec.message) = ("failed".into(), why),
            },
        },
    }
    if rec.outcome == "failed" {
        tracing::warn!(url = %page.url, message = %rec.message, "a page could not be read");
    }
    st.store.page_record(&page.id, &rec).await?;
    let entry = json!({ "page_id": page.id, "url": page.url, "vendor_id": page.vendor_id,
                        "outcome": rec.outcome, "message": rec.message, "models": models,
                        "http_status": reading.http_status, "elapsed_ms": reading.elapsed_ms });
    st.store.job_run_append(run_id, "pages", &json!([entry])).await?;
    let settled = settle_when_checked(st, run_id, &page.vendor_id).await?;
    Ok(Json(json!({ "page_id": page.id, "outcome": rec.outcome, "message": rec.message, "settled": settled })))
}

/// Settles a vendor once the run has checked every one of its pages: settling after each page
/// would publish a mix of new and old readings when two pages changed at once.
async fn settle_when_checked(st: &AppState, run_id: &str, vendor_id: &str) -> ApiResult<Option<Vec<Settled>>> {
    let Some(run) = st.store.job_run(run_id).await? else { return Ok(None) };
    let covered: HashSet<&String> = run.covers.iter().collect();
    let pages = st.store.pages(Some(vendor_id), Some("active")).await?;
    let states = st.store.page_states().await?;
    let checked = pages
        .iter()
        .filter(|p| covered.contains(&p.id))
        .all(|p| states.get(&p.id).and_then(|s| s.last_checked_at).is_some_and(|t| t >= run.started_at));
    // Taken once: of two readings that finish together, one settles.
    if !checked || !st.store.job_run_settling(run_id, vendor_id).await? {
        return Ok(None);
    }
    let results = settle_vendor(st, vendor_id, Some(run_id)).await?;
    let list = serde_json::to_value(&results).map_err(|e| ApiError::invalid(e.to_string()))?;
    st.store.job_run_append(run_id, "models", &list).await?;
    Ok(Some(results))
}

/// Why a page was not read.
enum Unread {
    /// The run used up the calls of the extraction model it may make.
    Budget(String),
    Failed(String),
}

/// Has the extraction model read a page's text, in parts where it is long, within the run's
/// budget of calls.
async fn extract_page(
    st: &AppState,
    settings: &Settings,
    run: &JobRun,
    page: &Page,
    text: &str,
) -> Result<PageExtraction, Unread> {
    // About three characters a token, with room for the prompt and the answer.
    let window = settings.extraction_context_window as usize;
    let size = (window.saturating_sub(24_000) * 3).clamp(MIN_CHUNK_CHARS, MAX_CHUNK_CHARS);
    let chunks = chunk_text(text, size);
    let budget = i64::from(settings.max_extractions_per_run);
    let fresh = st.store.job_run(&run.id).await.ok().flatten().and_then(|r| r.result["extractions"].as_i64());
    if fresh.unwrap_or(0) + chunks.len() as i64 > budget {
        return Err(Unread::Budget(format!(
            "the run's budget of {budget} extractions is used up; the page is read at the next run"
        )));
    }
    let _ = st.store.job_run_count(&run.id, "extractions", chunks.len() as i64).await;
    let failed = |e: sqlx::Error| Unread::Failed(e.to_string());
    let vendor: Option<Vendor> = st.store.vendor(&page.vendor_id).await.map_err(failed)?;
    let vendor = vendor.map(|v| v.display_name).unwrap_or_default();
    let known = st.store.known_model_ids(&page.vendor_id).await.map_err(failed)?.join(", ");
    let mut all = PageExtraction { chunks: chunks.len() as u32, ..Default::default() };
    for (i, chunk) in chunks.iter().enumerate() {
        let answer = call_extraction(st, settings, &vendor, &known, chunk, (i + 1, chunks.len()))
            .await
            .map_err(Unread::Failed)?;
        let found = models_from_answer(&answer, &page.url);
        all.notes.extend(found.notes);
        for m in found.models {
            merge(&mut all.models, m);
        }
    }
    Ok(all)
}

/// Splits a page's text into parts of at most `max` characters, at line ends. A table that is
/// split carries its first row (its header) into the next part, so that its columns keep their
/// names. A line longer than a part is cut.
pub fn chunk_text(text: &str, max: usize) -> Vec<String> {
    if text.chars().count() <= max {
        return vec![text.to_owned()];
    }
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut header: Option<String> = None;
    for line in text.lines() {
        let row = line.contains(" | ");
        if !row {
            header = None;
        }
        let pieces: Vec<String> = if line.chars().count() > max {
            line.chars().collect::<Vec<_>>().chunks(max).map(|c| c.iter().collect()).collect()
        } else {
            vec![line.to_owned()]
        };
        for piece in pieces {
            if !part.is_empty() && part.chars().count() + piece.chars().count() + 1 > max {
                parts.push(std::mem::take(&mut part));
                if let Some(h) = header.as_deref().filter(|h| *h != piece && h.len() + piece.len() + 2 <= max) {
                    part.push_str(h);
                    part.push('\n');
                }
            }
            part.push_str(&piece);
            part.push('\n');
        }
        if row && header.is_none() {
            header = Some(line.to_owned());
        }
    }
    if !part.trim().is_empty() {
        parts.push(part);
    }
    parts
}

/// The JSON schema the extraction model answers in. Strict: every member present, null where the
/// page says nothing, nothing else. Without it a model may answer in a shape of its own (prices
/// keyed by model, seen live); with it every provider with structured output holds to the shape.
fn extraction_schema() -> Value {
    let rate = json!({ "type": ["number", "null"] });
    let object = |properties: Value| {
        let required: Vec<String> = properties.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
    };
    let nullable = |schema: Value| json!({ "anyOf": [schema, { "type": "null" }] });
    let token_rates = json!({
        "input": rate, "output": rate, "cache_read": rate, "cache_write_5m": rate, "cache_write_1h": rate,
    });
    // The variants are fixed properties, not an open map, which structured output may refuse.
    let variant = nullable(object(token_rates.clone()));
    // Hours as the page states them; they are converted to UTC by code, not by the model.
    let hours = nullable(object(json!({
        "weekdays": { "type": "array", "items": { "type": "integer" } },
        "hours": { "type": "array", "items": { "type": "string" } },
        "utc_offset": { "type": "string" }
    })));
    let flag = json!({ "type": ["boolean", "null"] });
    let count = json!({ "type": ["integer", "null"] });
    let words = nullable(json!({ "type": "array", "items": { "type": "string" } }));
    let mut properties = json!({
        "model": { "type": "string" }, "display_name": { "type": ["string", "null"] },
        "api_id_stated": { "type": "boolean" }, "currency": { "type": ["string", "null"] },
        "reasoning": rate, "per_character": rate, "per_request": rate, "per_server_tool_call": rate,
        "input_audio": rate, "output_audio": rate, "input_image": rate, "output_image": rate,
        "per_image": rate, "per_audio_second": rate, "per_search_unit": rate,
        "variants": object(json!({ "batch": variant, "priority": variant, "flex": variant })),
        "tiers": { "type": "array", "items": object(json!({
            "above_input_tokens": { "type": "integer" }, "input": rate, "output": rate, "cache_read": rate
        })) },
        "off_peak": nullable(object(json!({
            "input": rate, "output": rate, "cache_read": rate, "peak": hours, "off_peak": hours
        }))),
        "context_window": count, "max_output_tokens": count,
        "input_modalities": words, "output_modalities": words,
        "vision": flag, "tools": flag, "structured_output": flag, "reasons": flag,
        "evidence": { "type": "string" }
    });
    for (k, v) in token_rates.as_object().into_iter().flatten() {
        properties[k] = v.clone();
    }
    object(json!({ "models": { "type": "array", "items": object(properties) } }))
}

fn extraction_prompt(vendor: &str, known: &str, text: &str, (part, parts): (usize, usize)) -> String {
    let part = if parts > 1 { format!(" This is part {part} of {parts} of the page.") } else { String::new() };
    format!(
        "The text below is a page of the provider \"{vendor}\" that describes models, what they can do, and \
         their prices.{part} List every model the page describes.\n\
         For \"model\" give the id the provider's API takes for the model, where the page states it, and set \
         api_id_stated to true; otherwise give the model's name as the page writes it and set api_id_stated to \
         false. These are the provider's model ids already known; where the page describes one of them, give \
         that id exactly: {known}.\n\
         Prices: give token rates (input, output, cache_read, cache_write_5m, cache_write_1h, and reasoning only \
         where reasoning tokens are priced apart from output) in the page's currency per one million tokens, and the \
         currency as a three-letter code. Give per_character for speech per character of input text, per_request per \
         call, and per_server_tool_call per call of a tool the provider runs, such as a web search. Give input_audio, \
         output_audio, input_image and output_image per one million audio or image tokens only where the page prices \
         them apart from text tokens. Give per_image per image made (at the model's standard size and quality), \
         per_audio_second per second of transcribed audio (a price per minute divided by 60), and per_search_unit per \
         search unit of a rerank model. Under \"variants\" give the rates of the batch, priority and flex tiers where \
         the page prices them apart. Under \"tiers\" give the rates that apply instead when a prompt is longer than a \
         threshold (for example above 200,000 input tokens), with the threshold in tokens as above_input_tokens. \
         Under \"off_peak\" give discounted rates that apply at certain hours, and the hours exactly as the page \
         states them: \"peak\" for the hours the normal rates apply or \"off_peak\" for the discounted ones, each \
         with weekdays (1 is Monday, 7 is Sunday), spans \"HH:MM-HH:MM\" in the page's time zone, and that zone's \
         offset from UTC with its sign as utc_offset (\"+08:00\" for Beijing time, \"+00:00\" for UTC). Do not \
         convert the hours. Use an empty list for tiers and null for off_peak when the page has none.\n\
         Properties: context_window is the most tokens the model takes in one request, max_output_tokens the most it \
         writes in one answer; input_modalities and output_modalities use the words text, image, document, audio, \
         video, embedding and ranking; vision is whether it takes images, tools whether it calls functions, \
         structured_output whether its answers can be held to a JSON schema, reasons whether it thinks before it \
         answers.\n\
         Use null for anything the page does not state; do not guess. In \"evidence\" quote the words of the page \
         the prices come from.\n\n\
         --- page ---\n{text}"
    )
}

/// What a page says when the mode in use has no endpoint.
fn no_endpoint(st: &AppState, settings: &Settings) -> String {
    format!("{} is not set", st.config.mode(settings).url_variable())
}

/// One call of the extraction model, on its OpenAI-compatible endpoint. The key goes to the
/// address the operator named with it, and nowhere else. All of a page is read with one snapshot of
/// the settings, so a switch of the mode during a run applies from the next page.
async fn call_extraction(
    st: &AppState,
    settings: &Settings,
    vendor: &str,
    known: &str,
    text: &str,
    part: (usize, usize),
) -> Result<Value, String> {
    let link = st.config.link(settings).ok_or_else(|| no_endpoint(st, settings))?;
    let model = st.config.extraction_model(settings).ok_or("no extraction model is set")?;
    let mut body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": extraction_prompt(vendor, known, text, part) }],
        "response_format": { "type": "json_schema",
                             "json_schema": { "name": "price_page", "schema": extraction_schema(), "strict": true } },
    });
    body[link.limit_field] = json!(EXTRACTION_OUTPUT_TOKENS);
    let mut request = st.http.post(format!("{}/chat/completions", link.url)).timeout(EXTRACTION_TIMEOUT).json(&body);
    if let Some(key) = &link.key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|e| format!("the extraction model could not be reached: {e}"))?;
    let status = response.status();
    let answer: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let said = answer["error"]["message"].as_str().unwrap_or("the extraction call failed");
        return Err(format!("the extraction model answered {status}: {said}"));
    }
    let text = answer["choices"][0]["message"]["content"].as_str().unwrap_or_default();
    json_in(text)
        .map(models_as_list)
        .filter(|v| v["models"].is_array())
        .ok_or_else(|| "the extraction model did not answer with a JSON object holding \"models\"".into())
}

/// `models` as a list: a model that ignores the schema may key them by model instead.
fn models_as_list(mut v: Value) -> Value {
    if let Some(by_model) = v["models"].as_object().cloned() {
        let list: Vec<Value> = by_model
            .into_iter()
            .map(|(model, mut p)| {
                if let Some(o) = p.as_object_mut() {
                    o.entry("model").or_insert(json!(model));
                }
                p
            })
            .collect();
        v["models"] = json!(list);
    }
    v
}

/// The JSON object in a model's answer: the whole text, the inside of a code fence, or the
/// span from the first `{` to the last `}`.
fn json_in(text: &str) -> Option<Value> {
    let parse = |t: &str| serde_json::from_str::<Value>(t.trim()).ok().filter(Value::is_object);
    if let Some(v) = parse(text) {
        return Some(v);
    }
    if let Some(start) = text.find("```") {
        let inner = &text[start + 3..];
        let inner = inner.strip_prefix("json").unwrap_or(inner);
        if let Some(end) = inner.find("```")
            && let Some(v) = parse(&inner[..end])
        {
            return Some(v);
        }
    }
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    (start < end).then(|| parse(&text[start..=end])).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_read_with_or_without_a_fence() {
        assert_eq!(json_in(r#"{"models":[]}"#), Some(json!({ "models": [] })));
        assert_eq!(json_in("Here you go:\n```json\n{\"models\": [1]}\n```\nDone."), Some(json!({ "models": [1] })));
        assert_eq!(json_in("The models are {\"models\": []} as asked."), Some(json!({ "models": [] })));
        assert_eq!(json_in("I could not find any prices."), None);
        assert_eq!(json_in("[1, 2]"), None);
        // Models keyed by id, as a model that ignores the schema answers.
        let keyed = models_as_list(json!({ "models": { "deepseek-flash": { "currency": "USD", "input": 0.3 } } }));
        assert_eq!(keyed["models"][0]["model"], "deepseek-flash");
    }

    #[test]
    fn a_long_page_is_read_in_parts_that_keep_a_tables_header() {
        assert_eq!(chunk_text("short", 100), ["short"]);
        let mut text = String::from("Pricing\nModel | Input | Output\n");
        for i in 0..40 {
            text.push_str(&format!("model-{i:02} | ${i}.00 | ${i}.50\n"));
        }
        text.push_str("Footnotes follow.\n");
        let parts = chunk_text(&text, 200);
        assert!(parts.len() > 3, "{parts:?}");
        assert!(parts.iter().all(|p| p.chars().count() <= 200), "{parts:?}");
        for p in &parts[1..parts.len() - 1] {
            assert!(p.starts_with("Model | Input | Output\n"), "the header comes along: {p}");
        }
        // Every row is in exactly one part.
        let rows: usize = parts.iter().map(|p| p.lines().filter(|l| l.starts_with("model-")).count()).sum();
        assert_eq!(rows, 40);
        // A line longer than a part is cut.
        assert_eq!(chunk_text(&"x".repeat(450), 200).len(), 3);
    }

    #[test]
    fn the_schema_is_strict_all_the_way_down() {
        fn check(v: &Value) {
            if let Some(o) = v.as_object() {
                if let Some(props) = o.get("properties").and_then(Value::as_object) {
                    assert_eq!(o["additionalProperties"], false, "{v}");
                    let required: Vec<&str> =
                        o["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
                    assert_eq!(required.len(), props.len(), "{v}");
                }
                o.values().for_each(check);
            } else if let Some(a) = v.as_array() {
                a.iter().for_each(check);
            }
        }
        check(&extraction_schema());
    }
}
