// llm-chat-admin-api — Backend-For-Frontend for the Zitadel user-management
// admin. Owns the operator OIDC session + the least-privilege admin service
// account; the browser only ever holds an opaque session cookie.
//
// This file is fleshed out in Task 13 (startup: config fail-fast + issuer-match
// guard + router + serve). For now it is a compiling placeholder so the crate
// is a real workspace member and `cargo test -p llm-chat-admin-api` runs.

mod config;
mod zitadel;

fn main() {
    eprintln!("llm-chat-admin-api: not yet wired (see Task 13)");
}

#[cfg(test)]
mod scaffold_smoke {
    #[test]
    fn crate_compiles_and_tests_run() {
        assert_eq!(2 + 2, 4);
    }
}
