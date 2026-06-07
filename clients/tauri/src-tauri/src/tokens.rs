//! Refresh-token storage in the OS keyring (service "lumina").

const SERVICE: &str = "lumina";

fn user(issuer: &str) -> String {
    format!("refresh:{issuer}")
}

pub fn save_refresh(issuer: &str, refresh_token: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, &user(issuer)) {
        let _ = e.set_password(refresh_token);
    }
}

pub fn load_refresh(issuer: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, &user(issuer))
        .ok()?
        .get_password()
        .ok()
}

pub fn clear_refresh(issuer: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, &user(issuer)) {
        let _ = e.delete_credential();
    }
}
