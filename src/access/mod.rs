//! IDM `config/access` API, validation, and pure document transforms.
//!
//! Endpoint shapes and safety constraints are documented in
//! `docs/api/19-config-access.md`. The document has no `_rev`, so callers must
//! use content snapshots as their write precondition. Its `configs` rules are
//! a disjunction: appending a rule can grant access, but can never revoke it.

pub mod api;
pub mod cli;
pub mod ops;
pub mod spec;

#[cfg(test)]
pub(crate) fn six_rule_fixture() -> serde_json::Value {
    serde_json::json!({
        "_id": "access",
        "configs": [
            {
                "pattern": "managed/alpha_user/*",
                "roles": "internal/role/user-reader",
                "methods": "read,query",
                "unknownRuleKey": {"preserve": true}
            },
            {
                "pattern": "managed/alpha_user/*",
                "roles": "internal/role/user-owner",
                "methods": "update,patch",
                "actions": "*",
                "customAuthz": "ownDataOnly()"
            },
            {
                "pattern": "endpoint/report/*",
                "roles": "internal/role/report-reader",
                "methods": "read",
                "actions": "*",
                "excludePatterns": "endpoint/report/private/*"
            },
            {
                "pattern": "internal/role/*",
                "roles": "internal/role/role-admin",
                "methods": "read,query,create,update,delete",
                "actions": "*"
            },
            {
                "pattern": "endpoint/duplicate/*",
                "roles": "internal/role/duplicate-reader",
                "methods": "read",
                "actions": "*"
            },
            {
                "pattern": "endpoint/duplicate/*",
                "roles": "internal/role/duplicate-reader",
                "methods": "read",
                "actions": "*"
            }
        ],
        "unknownTopLevelKey": "preserve me"
    })
}
