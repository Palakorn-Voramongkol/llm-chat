//! Dashboard fan-out: per-area `totalResult` counts from existing `_search`
//! endpoints (design §10) + the string-or-number parse trap (§14.6). Each count
//! is independent: a single failing call degrades that card to `null`, it never
//! blanks the page (§12).

use serde_json::{json, Value};

use super::ZitadelClient;

/// PURE: extract `details.totalResult` as a count. Zitadel serializes this field
/// as a JSON **number** in some builds and a JSON **string** in others (§14.6);
/// try `as_u64` first, then `as_str().parse`. Returns `None` when the field is
/// absent or unparseable so the card degrades to an em-dash, never a false `0`.
pub fn count_from_total(v: &Value) -> Option<u64> {
    let total = v.get("details")?.get("totalResult")?;
    total
        .as_u64()
        .or_else(|| total.as_str().and_then(|s| s.parse::<u64>().ok()))
}

impl ZitadelClient {
    /// Count human users via v2 (`POST /v2/users`, type-filtered like §3.1).
    /// `None` on its own failure so the card degrades to an em-dash (§12).
    pub async fn count_humans(&self) -> Option<u64> {
        let url = format!("{}/v2/users", self.cfg.issuer);
        let body = json!({ "queries": [{ "typeQuery": { "type": "TYPE_HUMAN" } }] });
        self.post_json(&url, &body).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count machine users via v2 (`POST /v2/users`, type-filtered).
    pub async fn count_machines(&self) -> Option<u64> {
        let url = format!("{}/v2/users", self.cfg.issuer);
        let body = json!({ "queries": [{ "typeQuery": { "type": "TYPE_MACHINE" } }] });
        self.post_json(&url, &body).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count project roles (`POST .../projects/{pid}/roles/_search`, §7).
    pub async fn count_roles(&self) -> Option<u64> {
        let url = format!("{}/management/v1/projects/{}/roles/_search", self.cfg.issuer, self.cfg.project_id);
        self.post_json(&url, &json!({})).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count user grants (`POST /management/v1/users/grants/_search`, §7).
    pub async fn count_grants(&self) -> Option<u64> {
        let url = format!("{}/management/v1/users/grants/_search", self.cfg.issuer);
        self.post_json(&url, &json!({})).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count this project's apps (`POST .../projects/{pid}/apps/_search`, §8).
    pub async fn count_apps(&self) -> Option<u64> {
        let url = format!("{}/management/v1/projects/{}/apps/_search", self.cfg.issuer, self.cfg.project_id);
        self.post_json(&url, &json!({})).await.ok().and_then(|v| count_from_total(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::count_from_total;
    use serde_json::json;

    #[test]
    fn count_from_total_reads_number_form() {
        // Some Zitadel builds serialize totalResult as a JSON number.
        let v = json!({ "details": { "totalResult": 42 }, "result": [] });
        assert_eq!(count_from_total(&v), Some(42));
    }

    #[test]
    fn count_from_total_reads_string_form() {
        // Other builds serialize the SAME field as a JSON string (§14.6).
        let v = json!({ "details": { "totalResult": "42" }, "result": [] });
        assert_eq!(count_from_total(&v), Some(42));
    }

    #[test]
    fn count_from_total_missing_is_none() {
        // No details/totalResult (or a non-numeric string) -> None, not 0, so the
        // card shows an em-dash rather than a misleading zero.
        assert_eq!(count_from_total(&json!({ "result": [] })), None);
        assert_eq!(count_from_total(&json!({ "details": { "totalResult": "abc" } })), None);
    }
}

#[cfg(test)]
mod method_contract {
    use super::ZitadelClient;

    // Compile-time contract: the fan-out methods exist with the Option<u64>
    // shape the /api/stats handler relies on. Awaiting each and binding the
    // result to an explicit Option<u64> pins both the &ZitadelClient receiver
    // and the return type at compile time. Their live values are exercised by
    // tests/integration.rs under ADMIN_IT=1 (the real instance is the source of
    // truth, not a mocked _search body).
    #[allow(dead_code)]
    async fn signatures_compile(z: &ZitadelClient) {
        let _: Option<u64> = z.count_humans().await;
        let _: Option<u64> = z.count_machines().await;
        let _: Option<u64> = z.count_roles().await;
        let _: Option<u64> = z.count_grants().await;
        let _: Option<u64> = z.count_apps().await;
    }
}
