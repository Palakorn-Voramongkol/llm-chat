//! The only module that touches Zitadel APIs. Submodules are added across
//! Phase C: error (gRPC->HTTP mapping), token (SA JWT-bearer + cache), model,
//! users, grants, keys. Task 12 lands `error`.

pub mod error;
