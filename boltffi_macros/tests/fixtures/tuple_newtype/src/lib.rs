use boltffi::*;

scaffolding!();

/// A transparent newtype: every signature spelling `Key` crosses as `u32`.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key(pub u32);

/// A newtype whose representation owns its storage.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label(pub String);

/// The marker also arrives through `cfg_attr`, which rustc expands first.
#[cfg_attr(all(), boltffi::data)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gated(pub i64);

#[export]
pub fn echo_key(key: Key) -> Key {
    key
}

#[export]
pub fn keys(count: u32) -> Vec<Key> {
    (0..count).map(Key).collect()
}

#[export]
pub fn sum_keys(keys: Vec<Key>) -> u32 {
    keys.iter().map(|key| key.0).sum()
}

#[export]
pub fn echo_label(label: Label) -> Label {
    label
}

#[export]
pub fn echo_gated(gated: Gated) -> Gated {
    gated
}
