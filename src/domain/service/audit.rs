//! Audit lines for the operations that change stored data.
//!
//! An audit line is an ordinary tracing event written under [`AUDIT_TARGET`], so
//! a deployment routes the trail to a log or a sink of its own with one
//! directive instead of picking it out of the request log. Each line names the
//! resource, the identifier the write addressed, and the outcome; a failure also
//! carries the code the client was answered with. Nothing else is recorded — no
//! request body, no attachment bytes, no stored path — so an audit trail can be
//! kept on without keeping a secret.
//!
//! Only the storage write that performs a mutation is audited, not the request
//! that asked for it. A request refused by validation, by an unresolvable
//! identifier or by a conflict never wrote anything and produces no audit line,
//! and a composite operation — a case placed into a suite, a project's run
//! imported into — is audited once, at the write, rather than once per layer it
//! passes through. A caller therefore reads exactly one line per attempted
//! mutation, and its outcome is the truth about the store.

use crate::storage::{Placement, Resource};

use super::error::DomainError;

/// The label an attachment is audited under.
///
/// An attachment is not one of the layout's resource collections — it is a file
/// inside a case folder, named by the case it belongs to — so it is audited
/// under its own noun rather than under a [`Resource`].
pub const ATTACHMENT_RESOURCE: &str = "attachment";

/// Target every audit line is written under, so a deployment can route the audit
/// trail away from its request log with a single directive.
pub const AUDIT_TARGET: &str = "tucano.audit";

/// The noun an audit line names a [`Resource`] by, matching the vocabulary of
/// the routes and of a response message.
pub const fn resource_noun(resource: Resource) -> &'static str {
    match resource {
        Resource::Projects => "project",
        Resource::Suites => "test_suite",
        Resource::Cases => "test_case",
        Resource::Runs => "test_run",
        Resource::Milestones => "milestone",
        Resource::Configurations => "test_configuration",
    }
}

/// The action an audit line names for a placement, spelled the way the response
/// message spells it.
pub const fn placement_action(mode: Placement) -> &'static str {
    match mode {
        Placement::Copy => "copy",
        Placement::Move => "move",
    }
}

/// Runs `operation` and writes one audit line whatever it answers.
///
/// The line carries the resource, the identifier and the outcome, plus the code
/// the client saw when the operation failed. `resource` is a label rather than a
/// [`Resource`] because an attachment is audited under its own noun with the
/// name it was stored as, which no resource collection names.
pub fn audited<T>(
    resource: &'static str,
    action: &'static str,
    id: &str,
    operation: impl FnOnce() -> Result<T, DomainError>,
) -> Result<T, DomainError> {
    let outcome = operation();
    match &outcome {
        Ok(_) => tracing::info!(target: AUDIT_TARGET, action, resource, id, outcome = "success"),
        Err(error) => tracing::info!(
            target: AUDIT_TARGET,
            action,
            resource,
            id,
            outcome = "failure",
            code = error.code()
        ),
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audited_returns_whatever_the_operation_answered() {
        let written = audited("project", "create", "p-1", || {
            Ok::<_, DomainError>("created".to_owned())
        });
        assert_eq!(written.as_deref().ok(), Some("created"));

        let refused = audited("project", "create", "p-1", || {
            Err::<String, _>(DomainError::Conflict("Resource already exists".to_owned()))
        });
        assert_eq!(refused.err().map(|error| error.code()), Some("conflict"));
    }

    #[test]
    fn every_resource_and_placement_has_a_label() {
        for resource in Resource::ALL {
            assert!(!resource_noun(resource).is_empty(), "{resource:?}");
        }
        assert_eq!(resource_noun(Resource::Suites), "test_suite");
        assert_eq!(resource_noun(Resource::Cases), "test_case");
        assert_eq!(placement_action(Placement::Copy), "copy");
        assert_eq!(placement_action(Placement::Move), "move");
    }
}
