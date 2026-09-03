# Principles

These priorities apply whenever rules or requirements conflict and a judgement call is needed.

- **Security is a priority over new features.** A feature is not done if it introduces an
  unresolved security risk. Delay or descope a feature rather than ship a known vulnerability,
  and raise a ticket for any security work deferred to a later change.
- **Accessibility must be maintained.** Do not merge a change that regresses accessibility
  conformance already achieved by a repository (for example, TucanoTestGUI's WCAG 2.1 AA gate).
  Treat an accessibility regression the same as a failing test.
- **Automate over manual.** Prefer a CI check, script, or workflow over a manual step or a
  documented human process, wherever one can be built. If a manual step must remain, document why
  automation is not yet possible and raise a ticket to close the gap.
