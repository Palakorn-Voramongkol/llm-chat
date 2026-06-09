//! Dashboard fan-out: per-area `totalResult` counts from existing `_search`
//! endpoints (design §10) + the string-or-number parse trap (§14.6). Each count
//! is independent: a single failing call degrades that card to `null`, it never
//! blanks the page (§12).

use serde_json::Value;

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
