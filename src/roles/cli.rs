//! `aic role` parser and command implementation.

use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::{confirm_destructive, ensure_prod_confirmed, print_json, print_table, tenant_for};
use crate::managed;
use crate::roles::{api, spec};
use crate::{Error, Result};

#[derive(Subcommand, Debug)]
pub enum RoleCommand {
    /// List IDM internal roles.
    List {
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show one IDM internal role.
    Show {
        /// Caller-chosen internal-role id.
        id: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Create an IDM internal role with a caller-chosen id.
    Create {
        /// The role's _id; name defaults to this exact value.
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Delete an IDM internal role, prompting unless --force is supplied.
    Delete {
        id: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// List or amend one role's managed-object privileges.
    Privilege {
        #[command(subcommand)]
        command: PrivilegeCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum PrivilegeCommand {
    /// List one role's privileges.
    List {
        role_id: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Add or replace a privilege, keyed by its path.
    Add {
        role_id: String,
        /// Managed-object endpoint, for example managed/alpha_user.
        #[arg(long)]
        path: String,
        /// Comma-separated permissions such as VIEW,UPDATE.
        #[arg(long, required = true, value_delimiter = ',')]
        permissions: Vec<String>,
        /// Attribute access in <name>:<ro|rw> form; repeat for each attribute.
        #[arg(long = "attr")]
        access_flags: Vec<spec::AccessFlag>,
        #[arg(long)]
        privilege_name: Option<String>,
        /// Comma-separated action names; defaults to an empty list.
        #[arg(long, value_delimiter = ',')]
        actions: Vec<String>,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
    /// Remove the privilege with an exact path match.
    Rm {
        role_id: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        tenant: Option<String>,
        /// Confirm a write to a production-themed tenant.
        #[arg(long)]
        yes: bool,
    },
}

pub async fn run(command: RoleCommand) -> Result<()> {
    match command {
        RoleCommand::List { tenant, json } => list(tenant, json).await,
        RoleCommand::Show { id, tenant, json } => show(&id, tenant, json).await,
        RoleCommand::Create {
            id,
            name,
            description,
            tenant,
            yes,
        } => create(&id, name, description, tenant, yes).await,
        RoleCommand::Delete {
            id,
            force,
            tenant,
            yes,
        } => delete(&id, force, tenant, yes).await,
        RoleCommand::Privilege { command } => privilege(command).await,
    }
}

async fn list(tenant_arg: Option<String>, json_output: bool) -> Result<()> {
    let tenant = tenant_for(tenant_arg)?;
    let roles = api::list_roles(&tenant).await?;
    if json_output {
        print_json(&roles)
    } else {
        print_roles(&roles);
        Ok(())
    }
}

async fn show(id: &str, tenant_arg: Option<String>, json_output: bool) -> Result<()> {
    validate_id(id)?;
    let tenant = tenant_for(tenant_arg)?;
    let role = api::read_role(&tenant, id).await?;
    if json_output {
        print_json(&role)
    } else {
        print_roles(std::slice::from_ref(&role));
        Ok(())
    }
}

async fn create(
    id: &str,
    name: Option<String>,
    description: Option<String>,
    tenant_arg: Option<String>,
    yes: bool,
) -> Result<()> {
    validate_id(id)?;
    let tenant = tenant_for(tenant_arg)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    match api::read_role(&tenant, id).await {
        Ok(_) => {
            return Err(Error::Config(format!(
                "internal role {id:?} already exists; refusing a destructive full replacement — amend its privileges with `aic role privilege add {id} ...`"
            )));
        }
        Err(Error::Api { status: 404, .. }) => {}
        Err(error) => return Err(error),
    }

    let name = name.unwrap_or_else(|| id.to_string());
    let mut body = json!({"name": name});
    if let Some(description) = description {
        body["description"] = Value::String(description);
    }
    let created = api::put_role(&tenant, id, body, ok.confirmed_prod).await?;
    let created_id = string_field(&created, "_id");
    let created_name = string_field(&created, "name");
    println!("created internal role {created_id} (name: {created_name})");
    Ok(())
}

async fn delete(id: &str, force: bool, tenant_arg: Option<String>, yes: bool) -> Result<()> {
    validate_id(id)?;
    let tenant = tenant_for(tenant_arg)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    if !force && !confirm_delete(id)? {
        return Err(Error::Config(format!(
            "internal role {id:?} was not deleted"
        )));
    }
    api::delete_role(&tenant, id, ok.confirmed_prod).await?;
    println!("deleted internal role {id}");
    Ok(())
}

async fn privilege(command: PrivilegeCommand) -> Result<()> {
    match command {
        PrivilegeCommand::List {
            role_id,
            tenant,
            json,
        } => privilege_list(&role_id, tenant, json).await,
        PrivilegeCommand::Add {
            role_id,
            path,
            permissions,
            access_flags,
            privilege_name,
            actions,
            tenant,
            yes,
        } => {
            privilege_add(
                &role_id,
                spec::PrivilegeSpec {
                    name: privilege_name,
                    path,
                    actions,
                    permissions,
                    access_flags,
                },
                tenant,
                yes,
            )
            .await
        }
        PrivilegeCommand::Rm {
            role_id,
            path,
            tenant,
            yes,
        } => privilege_rm(&role_id, &path, tenant, yes).await,
    }
}

async fn privilege_list(
    role_id: &str,
    tenant_arg: Option<String>,
    json_output: bool,
) -> Result<()> {
    validate_id(role_id)?;
    let tenant = tenant_for(tenant_arg)?;
    let role = api::read_role(&tenant, role_id).await?;
    let privileges = role_privileges(&role)?;
    if json_output {
        print_json(privileges)
    } else {
        print_privileges(privileges);
        Ok(())
    }
}

async fn privilege_add(
    role_id: &str,
    privilege_spec: spec::PrivilegeSpec,
    tenant_arg: Option<String>,
    yes: bool,
) -> Result<()> {
    validate_id(role_id)?;
    let privilege = spec::build_privilege(privilege_spec)?;
    warn_unknown_permissions(&privilege.permissions);
    let object_name = spec::object_name(&privilege.path)?;
    let tenant = tenant_for(tenant_arg)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;

    // Resolve and validate the live schema before reading or writing the role:
    // IDM's privilege-policy 403 cannot identify a bad path or attribute.
    let (_, object) = managed::api::get_managed_with_object(&tenant, object_name).await?;
    spec::validate_attributes(&object, &privilege.access_flags, &privilege.path)?;

    let role = api::read_role(&tenant, role_id).await?;
    let path = privilege.path.clone();
    let merged = spec::merge_privilege(&role, privilege)?;
    privilege_write(&tenant, role_id, merged.amendment, ok.confirmed_prod).await?;
    let verb = if merged.replaced { "replaced" } else { "added" };
    println!("{verb} privilege {path} on internal role {role_id}");
    Ok(())
}

async fn privilege_rm(
    role_id: &str,
    path: &str,
    tenant_arg: Option<String>,
    yes: bool,
) -> Result<()> {
    validate_id(role_id)?;
    let tenant = tenant_for(tenant_arg)?;
    let ok = ensure_prod_confirmed(&tenant, yes)?;
    let role = api::read_role(&tenant, role_id).await?;
    let (amendment, removed) = spec::remove_privilege(&role, path)?;
    if !removed {
        return Err(Error::Config(format!(
            "internal role {role_id:?} has no privilege with path {path:?}"
        )));
    }
    privilege_write(&tenant, role_id, amendment, ok.confirmed_prod).await?;
    println!("removed privilege {path} from internal role {role_id}");
    Ok(())
}

async fn privilege_write(
    tenant: &str,
    role_id: &str,
    amendment: spec::RoleAmendment,
    confirmed_prod: bool,
) -> Result<Value> {
    match api::put_role_if_match(
        tenant,
        role_id,
        amendment.body,
        &amendment.revision,
        confirmed_prod,
    )
    .await
    {
        Err(Error::Api { status: 403, .. }) => Err(Error::Config(format!(
            "AM rejected the privilege for internal role {role_id:?}; it validates the --path, --permissions values, and --attr names, but reports all such failures as an opaque policy-validation 403"
        ))),
        Err(Error::Api { status: 412, .. }) => Err(Error::Config(format!(
            "internal role {role_id:?} changed since it was read; nothing was written — re-run the command"
        ))),
        other => other,
    }
}

fn warn_unknown_permissions(permissions: &[String]) {
    for permission in spec::unknown_permissions(permissions) {
        eprintln!(
            "warning: unrecognised permission {permission:?}; known values are {}; proceeding because AIC does not publish an authoritative permission enum",
            spec::KNOWN_PERMISSIONS.join(", ")
        );
    }
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '?' | '#'))
    {
        return Err(Error::Config(format!(
            "internal role id {id:?} is empty or contains a URL path separator"
        )));
    }
    Ok(())
}

fn confirm_delete(id: &str) -> Result<bool> {
    confirm_destructive(
        "role deletion",
        &format!("Delete internal role {id:?}?"),
        "--force",
    )
}

fn role_privileges(role: &Value) -> Result<&Vec<Value>> {
    role.get("privileges")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: format!("internal role has no `privileges` array: {role}"),
        })
}

fn string_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn print_roles(roles: &[Value]) {
    let rows = roles
        .iter()
        .map(|role| {
            let id = string_field(role, "_id");
            let name = string_field(role, "name");
            vec![
                id.to_string(),
                name.to_string(),
                string_field(role, "description").to_string(),
                role.get("privileges")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
                    .to_string(),
                if id == name { "" } else { "!" }.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        &["ID", "NAME", "DESCRIPTION", "PRIVILEGES", "ID!=NAME"],
        &rows,
    );
}

fn print_privileges(privileges: &[Value]) {
    let rows = privileges
        .iter()
        .map(|privilege| {
            let permissions = string_array(privilege.get("permissions"));
            let actions = string_array(privilege.get("actions"));
            let attributes = privilege
                .get("accessFlags")
                .and_then(Value::as_array)
                .map(|flags| {
                    flags
                        .iter()
                        .map(|flag| {
                            let attribute = string_field(flag, "attribute");
                            let mode =
                                if flag.get("readOnly").and_then(Value::as_bool) == Some(true) {
                                    "ro"
                                } else {
                                    "rw"
                                };
                            format!("{attribute}:{mode}")
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            vec![
                string_field(privilege, "name").to_string(),
                string_field(privilege, "path").to_string(),
                permissions,
                actions,
                attributes,
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        &["NAME", "PATH", "PERMISSIONS", "ACTIONS", "ATTRIBUTES"],
        &rows,
    );
}

fn string_array(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::config::TenantTheme;

    fn parsed(args: &[&str]) -> RoleCommand {
        let cli = crate::cli::Cli::try_parse_from(args).expect("role command should parse");
        let Some(crate::cli::Command::Role { command }) = cli.command else {
            panic!("expected role command");
        };
        command
    }

    fn assert_production_consent(yes: bool) {
        let result = crate::cli::prod_write_ok(TenantTheme::Production, "prod", yes);
        if yes {
            assert!(
                result
                    .expect("--yes should authorize production")
                    .confirmed_prod
            );
        } else {
            let Err(error) = result else {
                panic!("production write without --yes was authorized");
            };
            assert!(error.to_string().contains("--yes"));
        }
    }

    #[test]
    fn every_role_mutation_parses_and_forwards_production_consent() {
        let commands = [
            parsed(&["aic", "role", "create", "support", "--yes"]),
            parsed(&["aic", "role", "delete", "support", "--force", "--yes"]),
            parsed(&[
                "aic",
                "role",
                "privilege",
                "add",
                "support",
                "--path",
                "managed/alpha_user",
                "--permissions",
                "VIEW",
                "--yes",
            ]),
            parsed(&[
                "aic",
                "role",
                "privilege",
                "rm",
                "support",
                "--path",
                "managed/alpha_user",
                "--yes",
            ]),
        ];

        for command in commands {
            let yes = match command {
                RoleCommand::Create { yes, .. } | RoleCommand::Delete { yes, .. } => yes,
                RoleCommand::Privilege {
                    command: PrivilegeCommand::Add { yes, .. } | PrivilegeCommand::Rm { yes, .. },
                } => yes,
                other => panic!("expected role mutation, got {other:?}"),
            };
            assert_production_consent(yes);
        }

        assert_production_consent(false);
    }

    #[test]
    fn role_reads_do_not_accept_production_consent() {
        assert!(crate::cli::Cli::try_parse_from(["aic", "role", "list", "--yes"]).is_err());
        assert!(
            crate::cli::Cli::try_parse_from([
                "aic",
                "role",
                "privilege",
                "list",
                "support",
                "--yes",
            ])
            .is_err()
        );
    }
}
