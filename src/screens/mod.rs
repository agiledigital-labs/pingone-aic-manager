//! Per-screen / per-tab modules. Each one owns its state struct, exposes
//! free-function handlers that take `&mut App` (or a more focused borrow),
//! and lives in its own file so `app.rs` can stay coordinator-shaped as
//! Scripts / OAuth / SAML / Journeys land.
//!
//! Rendering still lives in `src/ui/*.rs`; this is state + behaviour. New
//! tab? Add a module here, hang its state off `App` as a single field,
//! wire its key handler into the dispatch table in `app::handle_key`, and
//! add a draw function in `ui/`. Try not to put new state on `App`
//! directly — the whole point of this split is to keep `App` thin.

pub mod auth_settings;
pub mod auth_setup;
pub mod list_state;
pub mod prod_confirm;
pub mod undo_history;
pub mod unlock;
