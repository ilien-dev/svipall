//! Where a schema's fingerprints live between visits.
//!
//! One `kv` row per selector, under `selector/<domain>/<schema>/<field>`, the same store and the
//! same zero-migration shape as watches. Scoped by domain: the `.price` on one shop says nothing
//! about the `.price` on another.

use crate::cache::Store;
use crate::extraction::{Fingerprint, Fingerprints};

pub const PREFIX: &str = "selector/";

fn scope(domain: &str, schema: &str) -> String {
    format!("{PREFIX}{domain}/{schema}/")
}

/// Everything remembered for this schema on this domain.
pub fn load(store: &Store, domain: &str, schema: &str) -> Fingerprints {
    let scope = scope(domain, schema);
    store
        .kv_list(&scope)
        .into_iter()
        .filter_map(|(k, v)| {
            let field = k.strip_prefix(&scope)?.to_string();
            let fp: Fingerprint = serde_json::from_str(&v).ok()?;
            Some((field, fp))
        })
        .collect()
}

/// Remember what the selectors found. Unchanged rows are still written; the store's UPSERT is
/// cheaper than a read-compare on a path that runs once per fetch.
pub fn save(store: &Store, domain: &str, schema: &str, found: &Fingerprints) {
    let scope = scope(domain, schema);
    for (field, fp) in found {
        if let Ok(v) = serde_json::to_string(fp) {
            let _ = store.kv_set(&format!("{scope}{field}"), &v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::schema::BASE_KEY;

    fn fp(tag: &str) -> Fingerprint {
        Fingerprint {
            tag: tag.into(),
            id: None,
            classes: vec!["x".into()],
            attr_keys: Vec::new(),
            text_len_bucket: 3,
            digit_tenths: 0,
            depth: 4,
            sibling_index: 0,
            ancestors: vec!["div".into()],
        }
    }

    #[test]
    fn fingerprints_are_scoped_by_domain_and_schema() {
        let store = Store::open_memory().unwrap();
        let mut a = Fingerprints::new();
        a.insert(BASE_KEY.into(), fp("div"));
        a.insert("price".into(), fp("span"));
        save(&store, "shop.example", "products", &a);

        assert_eq!(load(&store, "shop.example", "products"), a);
        assert!(load(&store, "other.example", "products").is_empty());
        assert!(load(&store, "shop.example", "reviews").is_empty());
    }

    #[test]
    fn a_later_save_updates_only_the_fields_it_saw() {
        let store = Store::open_memory().unwrap();
        let mut first = Fingerprints::new();
        first.insert("price".into(), fp("span"));
        first.insert("title".into(), fp("h2"));
        save(&store, "d", "s", &first);
        let mut later = Fingerprints::new();
        later.insert("price".into(), fp("b"));
        save(&store, "d", "s", &later);
        let got = load(&store, "d", "s");
        assert_eq!(got["price"].tag, "b");
        assert_eq!(got["title"].tag, "h2", "an unseen field keeps its memory");
    }
}
