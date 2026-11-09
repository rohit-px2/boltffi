use std::time::Duration;

use boltffi::{CustomTypeConversionError, *};

custom_type!(
    pub FixtureInstant,
    remote = std::time::Duration,
    repr = i64,
    into_ffi = instant_into_ffi,
    try_from_ffi = instant_try_from_ffi,
);

pub fn instant_into_ffi(value: &Duration) -> i64 {
    value.as_millis() as i64
}

pub fn instant_try_from_ffi(value: i64) -> Result<Duration, CustomTypeConversionError> {
    u64::try_from(value)
        .map(Duration::from_millis)
        .map_err(|_| CustomTypeConversionError)
}

#[export]
pub fn shift_instant(when: FixtureInstant, by: i64) -> FixtureInstant {
    let millis = instant_into_ffi(&when).saturating_add(by).max(0);
    instant_try_from_ffi(millis).unwrap_or_default()
}

#[export]
pub fn maybe_instant(present: bool) -> Option<FixtureInstant> {
    present.then(|| Duration::from_millis(1234))
}

#[export]
pub fn instants(count: u32) -> Vec<FixtureInstant> {
    (0..count)
        .map(|index| Duration::from_millis(u64::from(index) * 1000))
        .collect()
}

/// A transparent newtype: every spelling of `FixtureKey` crosses as `u32`.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixtureKey(pub u32);

#[export]
pub fn echo_key(key: FixtureKey) -> FixtureKey {
    key
}

#[export]
pub fn shift_key(key: FixtureKey, by: u32) -> FixtureKey {
    FixtureKey(key.0 + by)
}

#[export]
pub fn keys(count: u32) -> Vec<FixtureKey> {
    (0..count).map(FixtureKey).collect()
}

#[export]
pub fn sum_keys(keys: Vec<FixtureKey>) -> u32 {
    keys.into_iter().map(|key| key.0).sum()
}
