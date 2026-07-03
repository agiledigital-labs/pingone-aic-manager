//! Background actions for the Mappings tab.

use std::time::Duration;
use std::time::Instant;

use crate::app::event::{AppEvent, ToastKind};
use crate::app::prod_confirm::PendingProdAction;
use crate::app::{App, InputMode};
use crate::mappings::api::MappingSummary;
use crate::mappings::screen::Event;
use crate::mappings::state::LoadState;
use crate::scripts::sync::{self, Selector};
use crate::scripts::{Kind, RemoteRef};

const RECON_POLL_DELAY: Duration = Duration::from_secs(2);
const RECON_MAX_POLLS: usize = 150;

pub fn pull_scripts(app: &mut App) {
    let Some((tenant, mapping)) = selected_mapping(app) else {
        return;
    };
    if !app.is_unlocked() {
        return;
    }

    let key = (tenant.clone(), mapping.clone());
    if app.mappings.in_flight_pull.contains(&key) {
        app.push_toast(
            ToastKind::Info,
            format!("pull already in progress: {mapping}"),
        );
        return;
    }
    if !crate::scripts::workspace::ensure_workspace_ready(app, &tenant) {
        return;
    }
    app.mappings.in_flight_pull.insert(key);

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = pull_mapping_scripts(&tenant, &mapping)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Mappings(Event::PullResult {
            tenant,
            mapping,
            result,
        }));
    });
}

pub fn refresh(app: &mut App, force: bool) {
    let Some(name) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };
    if !app.is_unlocked()
        || app.mappings.refreshing.contains(&name)
        || (!force && app.mappings.data.contains_key(&name))
    {
        return;
    }

    app.mappings.data.insert(name.clone(), LoadState::Loading);
    app.mappings.refreshing.insert(name.clone());
    app.mappings.last_poll = Instant::now();

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let result = crate::mappings::api::list_mappings(&name)
            .await
            .map_err(|error| error.to_string());
        let _ = tx.send(AppEvent::Mappings(Event::Listed {
            tenant: name,
            result,
        }));
    });
}

pub fn apply_refresh(
    app: &mut App,
    tenant: String,
    result: std::result::Result<Vec<MappingSummary>, String>,
) {
    match result {
        Ok(mappings) => {
            app.mappings
                .data
                .insert(tenant.clone(), LoadState::Loaded(mappings));
        }
        Err(error) => {
            app.mappings
                .data
                .insert(tenant.clone(), LoadState::Failed(error.clone()));
            if app
                .active_tenant()
                .is_some_and(|active| active.name == tenant)
            {
                app.push_toast(ToastKind::Error, format!("Mappings list failed: {error}"));
            }
        }
    }

    if app
        .active_tenant()
        .is_some_and(|active| active.name == tenant)
    {
        let count = crate::mappings::screen::row_count(app);
        app.mappings.clamp_selection(count);
    }
}

pub fn run_recon(app: &mut App) {
    let Some((tenant, mapping)) = selected_mapping(app) else {
        return;
    };
    if !app.is_unlocked() {
        return;
    }

    if app.active_tenant().is_some_and(|tenant| tenant.is_prod()) {
        app.prod_confirm.pending = Some(PendingProdAction::Mappings(ProdAction::Recon {
            tenant,
            mapping,
        }));
        app.input_mode = InputMode::ProdConfirm;
        return;
    }

    execute_recon(app, tenant, mapping, false);
}

pub fn execute_recon(app: &mut App, tenant: String, mapping: String, confirmed_prod: bool) {
    if !app.is_unlocked() {
        return;
    }

    let key = (tenant.clone(), mapping.clone());
    if app.mappings.in_flight_recon.contains(&key) {
        app.push_toast(
            ToastKind::Info,
            format!("reconciliation already in progress: {mapping}"),
        );
        return;
    }

    app.mappings.in_flight_recon.insert(key);
    app.push_toast(
        ToastKind::Info,
        format!("reconciliation started: {mapping}"),
    );

    let tx = app.events.tx.clone();
    tokio::spawn(async move {
        let recon_id =
            match crate::mappings::api::start_recon(&tenant, &mapping, confirmed_prod).await {
                Ok(id) => id,
                Err(error) => {
                    let _ = tx.send(AppEvent::Mappings(Event::ReconStatus {
                        tenant,
                        mapping,
                        status: Err(error.to_string()),
                    }));
                    return;
                }
            };

        for attempt in 0..RECON_MAX_POLLS {
            match crate::mappings::api::recon_status(&tenant, &recon_id).await {
                Ok(status) => {
                    let terminal = crate::mappings::api::state_is_terminal(&status.state);
                    let _ = tx.send(AppEvent::Mappings(Event::ReconStatus {
                        tenant: tenant.clone(),
                        mapping: mapping.clone(),
                        status: Ok(status),
                    }));
                    if terminal {
                        return;
                    }
                }
                Err(error) => {
                    let _ = tx.send(AppEvent::Mappings(Event::ReconStatus {
                        tenant,
                        mapping,
                        status: Err(error.to_string()),
                    }));
                    return;
                }
            }

            if attempt + 1 < RECON_MAX_POLLS {
                tokio::time::sleep(RECON_POLL_DELAY).await;
            }
        }

        let _ = tx.send(AppEvent::Mappings(Event::ReconStatus {
            tenant,
            mapping,
            status: Err(format!(
                "reconciliation did not finish within {} seconds",
                RECON_POLL_DELAY.as_secs() * RECON_MAX_POLLS as u64
            )),
        }));
    });
}

fn selected_mapping(app: &App) -> Option<(String, String)> {
    let tenant = app.active_tenant()?.name.clone();
    let mapping = app.mappings.selected_mapping(&tenant)?.name.clone();
    Some((tenant, mapping))
}

#[derive(Debug)]
pub enum ProdAction {
    Recon { tenant: String, mapping: String },
}

pub fn execute_prod_action(app: &mut App, action: ProdAction) {
    match action {
        ProdAction::Recon { tenant, mapping } => execute_recon(app, tenant, mapping, true),
    }
}

pub fn resume_mode(_app: &App, _action: &ProdAction) -> InputMode {
    InputMode::Normal
}

pub fn describe_prod_action(action: &ProdAction) -> Option<String> {
    match action {
        ProdAction::Recon { mapping, .. } => Some(format!(
            "run reconciliation on {mapping} - creates/updates/deletes target objects"
        )),
    }
}

async fn pull_mapping_scripts(tenant: &str, mapping: &str) -> crate::Result<String> {
    let refs = Kind::IdmSyncMapping.list(tenant, "").await?;
    let names = script_names_for_mapping(refs, mapping);
    if names.is_empty() {
        return Ok(format!("{mapping} has no inline scripts"));
    }

    let mut pulled = 0usize;
    for name in names {
        let selector = Selector::Name(name);
        let outcomes = sync::pull(tenant, "", Kind::IdmSyncMapping, &selector, false).await?;
        pulled += outcomes.len();
    }
    Ok(format!("pulled {pulled} scripts for {mapping}"))
}

fn script_names_for_mapping(refs: Vec<RemoteRef>, mapping: &str) -> Vec<String> {
    let prefix = format!("{mapping}.");
    refs.into_iter()
        .filter(|remote| remote.name.starts_with(&prefix))
        .map(|remote| remote.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str) -> RemoteRef {
        RemoteRef {
            kind: Kind::IdmSyncMapping,
            id: format!("sync/{name}"),
            name: name.into(),
            context: None,
            is_default: false,
            evaluator_version: None,
        }
    }

    #[test]
    fn script_names_for_mapping_filters_on_mapping_prefix() {
        let names = script_names_for_mapping(
            vec![
                remote("map.onCreate"),
                remote("map.transform.name"),
                remote("map_two.onCreate"),
                remote("mapish.onUpdate"),
            ],
            "map",
        );

        assert_eq!(names, ["map.onCreate", "map.transform.name"]);
    }
}
