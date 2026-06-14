//! Background loads for the read-only OAuth2 TUI tab.

use crate::app::App;
use crate::app::event::AppEvent;
use crate::oauth::screen::Event;
use crate::oauth::state::{LoadState, REALM, State};

pub fn load_list(app: &mut App, force: bool) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked()
        || app.oauth.refreshing.contains(&tenant)
        || (!force && app.oauth.data.contains_key(&tenant))
    {
        return;
    }

    app.oauth.data.insert(tenant.clone(), LoadState::Loading);
    app.oauth.refreshing.insert(tenant.clone());

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        match crate::oauth::api::list_clients(&tenant, REALM).await {
            Ok(clients) => {
                let _ = tx.send(AppEvent::Oauth(Event::ListLoaded { tenant, clients }));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Oauth(Event::LoadFailed {
                    tenant,
                    id: None,
                    message: error.to_string(),
                }));
            }
        }
    });
}

pub fn load_client(app: &mut App, id: String, force: bool) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked() {
        return;
    }

    let key = State::detail_key(&tenant, &id);
    if app.oauth.detail_loading.contains(&key)
        || (!force && app.oauth.detail_cache.contains_key(&key))
    {
        return;
    }

    app.oauth.detail_failed.remove(&key);
    app.oauth.detail_loading.insert(key);

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        match crate::oauth::api::read_client(&tenant, REALM, &id).await {
            Ok(client) => {
                let _ = tx.send(AppEvent::Oauth(Event::ClientLoaded { tenant, id, client }));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Oauth(Event::LoadFailed {
                    tenant,
                    id: Some(id),
                    message: error.to_string(),
                }));
            }
        }
    });
}
