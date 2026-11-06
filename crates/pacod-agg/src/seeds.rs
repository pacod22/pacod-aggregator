//! What a new installation starts from: the vendors the catalogue crate names, the pages each one
//! is known to publish its prices on, and the public lists that fill gaps until pages are read.
//!
//! The vendors' own pages are not read at once: the scan looks at them and proposes them, and an
//! administrator confirms each. The lists are active from the first check, at a lower priority,
//! so that the catalogue is not empty on the first day. Each vendor is seeded once: what an
//! administrator removes or dismisses stays that way, and a vendor a later release adds arrives.

use pacod_agg_catalog::{litellm, openrouter, vendors};

use crate::store::Store;
use crate::store::pages::NewPage;
use crate::store::vendors::VendorInput;

/// The pages that list a vendor's models and prices, as far as they are known. Vendors billed in
/// another currency have none: their pages are in another language, and the aggregator reads the
/// international ones.
const SITES: &[(&str, &[&str])] = &[
    ("openai", &["https://developers.openai.com/api/docs/pricing"]),
    ("anthropic", &["https://platform.claude.com/docs/en/about-claude/pricing"]),
    ("google", &["https://aistudio.google.com/docs/pricing"]),
    ("zai", &["https://docs.z.ai/guides/overview/pricing"]),
    ("deepseek", &["https://api-docs.deepseek.com/quick_start/pricing"]),
    ("xai", &["https://docs.x.ai/developers/models"]),
    ("moonshot", &["https://platform.kimi.ai/docs/pricing/chat"]),
];

/// Where a list ranks: after every page an administrator confirms.
pub const LIST_PRIORITY: i32 = 100;

/// Adds the vendors not seeded yet, each with its lists. A vendor with pages of its own counts
/// as complete once they are read; one known from the lists alone never does.
pub async fn ensure(store: &Store) -> sqlx::Result<()> {
    let seeded = store.seeded().await?;
    for v in vendors::VENDORS.iter().filter(|v| !seeded.iter().any(|s| s == v.id)) {
        let sites = SITES.iter().find(|(id, _)| *id == v.id).map(|(_, s)| s.iter().map(|u| (*u).to_owned()).collect());
        let input = VendorInput {
            id: v.id.into(),
            display_name: v.display_name.into(),
            complete: sites.is_some(),
            sites: sites.unwrap_or_default(),
            currency: v.currency.into(),
            enabled: true,
        };
        // An administrator may have added it under this id already: its pages are theirs.
        let created = store.vendor_create(&input).await?.is_some();
        store.seeded_add(v.id).await?;
        if !created {
            continue;
        }
        for (i, (url, format)) in [(openrouter::URL, "openrouter"), (litellm::URL, "litellm")].iter().enumerate() {
            let page = NewPage {
                vendor_id: v.id.into(),
                url: (*url).into(),
                kind: "feed".into(),
                format: (*format).into(),
                status: "active".into(),
                found_by: "seed".into(),
                priority: LIST_PRIORITY + i as i32,
                ..NewPage::default()
            };
            store.page_add(&page).await?;
        }
    }
    Ok(())
}
