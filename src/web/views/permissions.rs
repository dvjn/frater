use maud::{Markup, html};

use crate::domain::{granted_scope, scope_allows};

/// Labels and role rules only. `domain::SCOPES` is the authority on what a scope
/// may be, and the domain holds the implications between them.
struct Permission {
    scope: &'static str,
    label: &'static str,
    requires_role: Option<&'static str>,
}

const REGISTRY: [Permission; 5] = [
    Permission {
        scope: "workouts:write",
        label: "Manage workouts",
        requires_role: None,
    },
    Permission {
        scope: "workouts:read",
        label: "Read workouts",
        requires_role: None,
    },
    Permission {
        scope: "catalogue:write",
        label: "Manage catalogue",
        requires_role: Some("superuser"),
    },
    Permission {
        scope: "catalogue:read",
        label: "Read catalogue",
        requires_role: None,
    },
    Permission {
        scope: "offline_access",
        label: "Stay connected",
        requires_role: None,
    },
];

fn permission_label(scope: &str) -> Option<&'static str> {
    REGISTRY
        .iter()
        .find(|permission| permission.scope == scope)
        .map(|permission| permission.label)
}

fn role_allows(scope: &str, role: &str) -> bool {
    REGISTRY
        .iter()
        .find(|permission| permission.scope == scope)
        .and_then(|permission| permission.requires_role)
        .is_none_or(|required| required == role)
}

fn coverable(scope: &str, requested_scope: &str, role: &str) -> bool {
    scope_allows(requested_scope, scope) && role_allows(scope, role)
}

fn default_scope(requested_scope: &str, role: &str) -> Vec<String> {
    REGISTRY
        .iter()
        .map(|permission| permission.scope)
        .filter(|scope| coverable(scope, requested_scope, role))
        .map(ToOwned::to_owned)
        .collect()
}

pub fn consent_grant(requested_scope: &str, role: &str) -> Option<String> {
    granted_scope(requested_scope, &default_scope(requested_scope, role), role)
}

fn permission_row(scope: &str) -> Markup {
    html! {
        li class="perm-row" {
            code class="perm-scope" { (scope) }
            @if let Some(label) = permission_label(scope) {
                span class="perm-label" { (label) }
            }
        }
    }
}

pub fn permissions(scope: &str) -> Markup {
    html! {
        ul class="perm-list" {
            @for item in scope.split_whitespace() { (permission_row(item)) }
        }
    }
}

fn group(title: &str, body: Markup) -> Markup {
    html! {
        div class="consent-group" {
            div class="consent-caption" { (title) }
            div class="consent-box" { (body) }
        }
    }
}

pub fn detail_row(label: &str, value: Markup) -> Markup {
    html! {
        div class="detail-row" {
            div class="detail-label" { (label) }
            div class="detail-value" { (value) }
        }
    }
}

pub fn application_group(rows: Markup) -> Markup {
    group("Application", rows)
}

pub fn permission_group(requested_scope: &str, role: &str) -> Markup {
    group(
        "Permissions",
        permissions(&consent_grant(requested_scope, role).unwrap_or_default()),
    )
}

const SWITCH_FORM_ID: &str = "oauth-switch";

pub fn account_entry(email: &str) -> Markup {
    html! {
        div class="consent-account" {
            div class="consent-avatar" aria-hidden="true" { "f" }
            div class="consent-account-body" {
                div class="consent-account-caption" { "Signed in as" }
                div class="consent-account-email" { (email) }
            }
            button type="submit" form=(SWITCH_FORM_ID) class="secondary compact" {
                "Switch"
            }
        }
    }
}

pub fn switch_form(csrf: &str, return_to: &str) -> Markup {
    html! {
        form method="post" action="/logout" id=(SWITCH_FORM_ID) {
            input type="hidden" name="csrf" value=(csrf);
            input type="hidden" name="return_to" value=(return_to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{consent_grant, permission_label};

    #[test]
    fn a_grant_holds_every_requested_scope_that_the_role_can_hold() {
        let requested = "workouts:read workouts:write catalogue:write offline_access";
        assert_eq!(
            consent_grant(requested, "superuser").as_deref(),
            Some("workouts:write catalogue:write offline_access")
        );
    }

    #[test]
    fn a_regular_user_never_grants_a_catalogue_write() {
        let requested = "workouts:write catalogue:write";
        assert_eq!(
            consent_grant(requested, "user").as_deref(),
            Some("workouts:write catalogue:read")
        );
    }

    #[test]
    fn staying_connected_follows_the_request() {
        assert_eq!(
            consent_grant("workouts:read offline_access", "user").as_deref(),
            Some("workouts:read offline_access")
        );
        assert_eq!(
            consent_grant("workouts:read", "user").as_deref(),
            Some("workouts:read")
        );
    }

    #[test]
    fn a_request_without_resource_access_grants_nothing() {
        assert_eq!(consent_grant("offline_access", "user"), None);
    }

    #[test]
    fn only_a_known_scope_carries_a_label() {
        assert_eq!(permission_label("workouts:write"), Some("Manage workouts"));
        assert_eq!(permission_label("reports:read"), None);
    }
}
