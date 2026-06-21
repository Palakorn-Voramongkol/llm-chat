//! Hand-rolled OIDC Authorization Code + PKCE for the end-user login, ported
//! from admin-api/src/auth.rs (gate changed to chat.user in Task 4). Not the
//! openidconnect crate: it rejects the plain-HTTP dev issuer. The callback JWT
//! is verified by the SHARED zitadel_auth::JwksCache and gated on `chat.user`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

use crate::config::KabyConfig;

fn b64url(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

/// Deterministic (verifier, challenge) with S256. `seed` is random per-login at
/// the call site; deterministic here so it is unit-testable.
pub fn pkce_pair(seed: &str) -> (String, String) {
    let verifier = b64url(Sha256::digest(format!("verifier:{seed}").as_bytes()).as_slice());
    let challenge = b64url(Sha256::digest(verifier.as_bytes()).as_slice());
    (verifier, challenge)
}

/// Build the /oauth/v2/authorize URL with PKCE + the project-aud + roles scopes.
/// redirect_uri uses cfg.public_origin (the FRONTEND origin) + /callback.
pub fn build_authorize_url(cfg: &KabyConfig, challenge: &str, state: &str, nonce: &str) -> String {
    let scope = format!(
        "openid profile email offline_access \
         urn:zitadel:iam:org:project:id:{}:aud \
         urn:zitadel:iam:org:projects:roles",
        cfg.project_id
    );
    let redirect_uri = format!("{}/callback", cfg.public_origin);
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &cfg.oidc_client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("nonce", nonce)
        .append_pair("prompt", "login")
        .finish();
    format!("{}/oauth/v2/authorize?{}", cfg.issuer, q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KabyConfig;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};

    fn cfg() -> KabyConfig {
        KabyConfig {
            issuer: "http://h:8080".into(),
            project_id: "p1".into(),
            audience: "p1".into(),
            oidc_client_id: "c1".into(),
            oidc_client_secret: "s1".into(),
            bind_addr: "0.0.0.0:7670".into(),
            public_origin: "http://localhost:3001".into(),
            allowed_origin: "http://localhost:3001".into(),
            session_key: "k".into(),
            cookie_secure: true,
        }
    }

    #[test]
    fn pkce_challenge_is_s256_of_verifier_and_url_safe() {
        let (verifier, challenge) = pkce_pair("seed-abc");
        assert_eq!(challenge, URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())));
        assert!(!verifier.contains('=') && !verifier.contains('+') && !verifier.contains('/'));
    }

    #[test]
    fn pkce_is_deterministic_per_seed() {
        assert_eq!(pkce_pair("s1"), pkce_pair("s1"));
        assert_ne!(pkce_pair("s1").0, pkce_pair("s2").0);
    }

    #[test]
    fn authorize_url_carries_pkce_state_nonce_scopes_and_frontend_redirect() {
        let url = build_authorize_url(&cfg(), "CHAL", "STATE", "NONCE");
        assert!(url.starts_with("http://h:8080/oauth/v2/authorize?"));
        assert!(url.contains("client_id=c1"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=STATE"));
        assert!(url.contains("nonce=NONCE"));
        // redirect_uri is the FRONTEND origin (:3001), URL-encoded
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A3001%2Fcallback"));
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aproject%3Aid%3Ap1%3Aaud"));
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aprojects%3Aroles"));
    }
}
