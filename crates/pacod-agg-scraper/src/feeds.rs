//! Lists that fill gaps: OpenRouter's, LiteLLM's and a pacod price feed, read without a browser or
//! a model, by the catalogue crate's readers, so that each format has one parser. A list is read
//! into the shape a page's reading has, for one vendor's models. It never states a vendor's API
//! id: it holds every model anyone ever priced, and proposes none.

use anyhow::Context;
use pacod_agg_catalog::{Evidence, Format, parse};
use pacod_agg_jobs::extraction::{PageExtraction, PageModel, merge};
use pacod_agg_jobs::jobs::PageTask;

/// What a list says about the vendor of `task`. Models a pacod feed names without a vendor count
/// as the vendor's: such a feed is the vendor's own.
pub fn read(task: &PageTask, body: &[u8]) -> anyhow::Result<PageExtraction> {
    let format = Format::from_name(&task.format).with_context(|| format!("no such list format: {}", task.format))?;
    let catalog = parse(body, Some(format)).map_err(|e| anyhow::anyhow!("{}: {e}", task.url))?;
    let mut out = PageExtraction::default();
    for m in catalog.models {
        if m.vendor.as_deref().is_some_and(|v| v != task.vendor_id) {
            continue;
        }
        let mut price = m.price.filter(|p| !p.is_empty());
        if let Some(p) = &mut price {
            // Nothing here changes with the day: what a list says is compared by its fingerprint.
            p.evidence =
                Some(Evidence { url: Some(task.url.clone()), method: Some("feed".into()), ..Evidence::default() });
        }
        merge(
            &mut out.models,
            PageModel {
                model: m.api_id,
                aliases: m.aliases,
                display_name: m.display_name,
                api_id_stated: false,
                kind: m.kind,
                price,
                properties: m.properties,
                parameters: m.parameters,
            },
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(format: &str, vendor: &str) -> PageTask {
        PageTask {
            vendor_id: vendor.into(),
            url: "https://lists.example/models.json".into(),
            kind: "feed".into(),
            format: format.into(),
            ..PageTask::default()
        }
    }

    #[test]
    fn a_list_is_read_for_one_vendor_and_states_no_api_id() {
        let body = br#"{ "data": [
            { "id": "openai/gpt-a", "context_length": 400000, "pricing": { "prompt": "0.000002", "completion": "0.00001" } },
            { "id": "anthropic/claude-a", "pricing": { "prompt": "0.000003", "completion": "0.000015" } }
        ] }"#;
        let found = read(&task("openrouter", "openai"), body).unwrap();
        assert_eq!(found.models.len(), 1, "{found:?}");
        let m = &found.models[0];
        assert_eq!((m.model.as_str(), m.api_id_stated), ("gpt-a", false));
        assert_eq!(m.properties.context_window, Some(400_000));
        let price = m.price.as_ref().unwrap();
        assert_eq!(price.rates.input, Some(2.0));
        let evidence = price.evidence.as_ref().unwrap();
        assert_eq!(
            (evidence.url.as_deref(), evidence.method.as_deref()),
            (Some(task("", "").url.as_str()), Some("feed"))
        );
        assert!(evidence.fetched_at.is_none(), "the fingerprint of a list does not change with the day");
    }

    #[test]
    fn litellm_and_a_pacod_feed_read_alike() {
        let litellm = br#"{ "gpt-a": { "litellm_provider": "openai", "mode": "chat", "input_cost_per_token": 0.000002,
                                        "output_cost_per_token": 0.00001 },
                            "azure/gpt-a": { "litellm_provider": "azure", "input_cost_per_token": 0.000009 } }"#;
        let found = read(&task("litellm", "openai"), litellm).unwrap();
        assert_eq!(found.models.len(), 1, "the clouds' prices are theirs: {found:?}");
        assert_eq!(found.models[0].price.as_ref().unwrap().rates.input, Some(2.0));

        let feed = br#"{ "currency": "USD", "models": { "bare-model": { "input": 1.0, "output": 2.0 },
                                                         "xai/grok-a": { "input": 3.0, "output": 4.0 } } }"#;
        let found = read(&task("pacod", "openai"), feed).unwrap();
        let ids: Vec<&str> = found.models.iter().map(|m| m.model.as_str()).collect();
        assert_eq!(ids, ["bare-model"], "another vendor's stays out");
    }

    #[test]
    fn what_cannot_be_read_says_where() {
        let err = read(&task("litellm", "openai"), b"not json").unwrap_err().to_string();
        assert!(err.contains("lists.example"), "{err}");
        assert!(read(&task("yaml", "openai"), b"{}").is_err());
    }
}
