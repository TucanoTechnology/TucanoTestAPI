//! Fuzz target: no document decoder panics on arbitrary bytes.
//!
//! Every type in `tucano_test::models` is fed the same input through
//! `serde_json::from_slice`. The contract is the one the property suite in
//! `tests/property.rs` pins for smaller inputs: a decoder answers `Ok` or
//! `Err`, and never panics, however malformed the document is.
//!
//! Run it with `cargo fuzz run deserialize_models` from `fuzz/`; the property
//! suite fails if a model is added here or there without the other side, so the
//! two lists cannot drift apart. See
//! `docs/testing/fuzz-and-property-tests.md` for the full procedure.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tucano_test::models::{
    Attachment, CaseHistoryEntry, CoverageReport, DefectLink, DefectLinkRequest, ImportCounts,
    ImportSummary, Milestone, MilestoneProgress, Project, StepAttachment, SuiteCoverage,
    SummaryReport, TestCase, TestCaseResult, TestCaseStep, TestConfiguration, TestRun, TestStep,
    TestSuite,
};

fuzz_target!(|data: &[u8]| {
    macro_rules! decode {
        ($model:ty) => {
            let answered: Result<$model, serde_json::Error> = serde_json::from_slice(data);
            let _ = answered;
        };
    }

    decode!(Attachment);
    decode!(StepAttachment);
    decode!(TestStep);
    decode!(TestCaseStep);
    decode!(TestCase);
    decode!(TestSuite);
    decode!(Project);
    decode!(DefectLink);
    decode!(DefectLinkRequest);
    decode!(TestCaseResult);
    decode!(ImportCounts);
    decode!(ImportSummary);
    decode!(TestRun);
    decode!(TestConfiguration);
    decode!(Milestone);
    decode!(MilestoneProgress);
    decode!(CaseHistoryEntry);
    decode!(CoverageReport);
    decode!(SuiteCoverage);
    decode!(SummaryReport);
});
