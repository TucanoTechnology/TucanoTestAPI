//! Release and environment metadata: the distinct names the GUI's context bar
//! offers as releases and environments.

use std::collections::BTreeSet;

use super::*;

impl<R: Repository + 'static> TestService<R> {
    // --- metadata ------------------------------------------------------

    /// The distinct, sorted `name` of every milestone the caller can reach.
    ///
    /// These are the releases a test run can be filed under, named the way a
    /// milestone names itself rather than by its derived identifier, so the GUI
    /// can offer them without resolving every milestone document.
    ///
    /// `reachable`, when set, is the authorisation filter: a milestone stored
    /// outside it contributes nothing. A trusted caller passes `None` and sees
    /// every milestone.
    pub fn release_names(&self, reachable: Option<&[String]>) -> Result<Vec<String>, DomainError> {
        self.cached_names("releases", Resource::Milestones, reachable)
    }

    /// The distinct, sorted `name` of every test configuration the caller can
    /// reach.
    ///
    /// These are the environments a test run can be executed against, named the
    /// way a configuration names itself rather than by its derived identifier.
    ///
    /// `reachable` filters exactly as it does for [`Self::release_names`].
    pub fn environment_names(
        &self,
        reachable: Option<&[String]>,
    ) -> Result<Vec<String>, DomainError> {
        self.cached_names("environments", Resource::Configurations, reachable)
    }

    /// The context bar (#415): names derive from every document of one
    /// resource, and the GUI fetches them on every navigation — served from
    /// the derivation cache, keyed by the caller's scope, retired by every
    /// local write and age-bounded across replicas.
    fn cached_names(
        &self,
        kind: &str,
        resource: Resource,
        reachable: Option<&[String]>,
    ) -> Result<Vec<String>, DomainError> {
        let key = format!("{kind}|{}", super::cache::scope_key(reachable));
        let value = self.cached_derivations(&key, || {
            Ok(Value::Array(
                self.distinct_names(resource, reachable)?
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ))
        })?;
        Ok(value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Every `name` a reachable project stores for `resource`, deduplicated and
    /// ordered byte-wise.
    ///
    /// A document governs itself through the project that stores it, so the
    /// filter names the home project alone: a name two projects repeat is
    /// reported once, and one an unreachable project holds is not reported at
    /// all. A document that cannot be read or carries no `name` is skipped
    /// rather than failing the listing, matching how
    /// [`Self::summary_report`] treats the runs it walks.
    fn distinct_names(
        &self,
        resource: Resource,
        reachable: Option<&[String]>,
    ) -> Result<Vec<String>, DomainError> {
        let mut names = BTreeSet::new();
        for project in self
            .repository
            .list(Resource::Projects)
            .map_err(error::read_error)?
        {
            if let Some(reachable) = reachable
                && !reachable.iter().any(|candidate| candidate == &project)
            {
                continue;
            }
            let home = Parent::Project(project);
            let identifiers = self
                .repository
                .list_children(&home, resource)
                .map_err(error::read_error)?;
            for id in identifiers {
                let Ok(value) = self.repository.read_at(resource, Some(&home), &id) else {
                    continue;
                };
                if let Some(name) = value.get("name").and_then(Value::as_str) {
                    names.insert(name.to_owned());
                }
            }
        }
        Ok(names.into_iter().collect())
    }
}
