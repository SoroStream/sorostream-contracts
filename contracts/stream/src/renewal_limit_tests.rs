//! Shared renewal-cap semantics for all stream creation inputs.

#![cfg(test)]

use crate::types::{CreateStreamOptions, CreateStreamParams, RenewalLimit};

fn effective_for_any_input<T: RenewalLimit>(input: &T) -> Option<u32> {
    input.effective_renew_count()
}

#[test]
fn recurrence_precedence_is_shared_by_live_creation_types() {
    let options = CreateStreamOptions {
        renew_count: Some(2),
        recurrence: Some(5),
        ..Default::default()
    };
    let params = CreateStreamParams {
        renew_count: Some(2),
        recurrence: Some(5),
        ..Default::default()
    };
    assert_eq!(effective_for_any_input(&options), Some(5));
    assert_eq!(effective_for_any_input(&params), Some(5));
}

#[test]
fn legacy_renew_count_is_used_when_recurrence_is_absent() {
    let options = CreateStreamOptions {
        renew_count: Some(2),
        ..Default::default()
    };
    let params = CreateStreamParams {
        renew_count: Some(2),
        ..Default::default()
    };
    assert_eq!(options.effective_renew_count(), Some(2));
    assert_eq!(params.effective_renew_count(), Some(2));
}
