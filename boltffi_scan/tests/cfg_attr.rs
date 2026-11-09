use std::fs;
use std::path::{Path, PathBuf};

use boltffi_ast::{DefaultValue, IntegerLiteral, PackageInfo, Primitive, SourceContract, TypeExpr};
use boltffi_scan::{ActiveCfg, ScanError, ScanInput, scan_package};

const SINGLE_MANIFEST: &str = r#"
[package]
name = "cfg-attr-fixture"
version = "0.1.0"
edition = "2024"

[features]
ffi = []

[workspace]
"#;

struct PackageFixture {
    manifest_dir: PathBuf,
    source: PathBuf,
}

impl PackageFixture {
    fn create(name: &str, manifest: &str, source: &str) -> Self {
        let manifest_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("cfg-attr")
            .join(name);
        fs::create_dir_all(manifest_dir.join("src")).expect("fixture directories");
        Self::write(manifest_dir.join("Cargo.toml"), manifest);
        let source_path = manifest_dir.join("src").join("lib.rs");
        Self::write(&source_path, source);
        Self {
            manifest_dir,
            source: source_path,
        }
    }

    fn scan(&self, package: &str, cfg: ActiveCfg) -> Result<SourceContract, ScanError> {
        scan_package(
            &ScanInput::new(
                &self.source,
                PackageInfo::new(package, Some("0.1.0".to_owned())),
            )
            .with_manifest_dir(&self.manifest_dir)
            .with_cfg(cfg),
        )
        .map(|scan| scan.complete().clone())
    }

    fn write(path: impl AsRef<Path>, contents: &str) {
        fs::write(path, contents).expect("fixture file");
    }
}

#[test]
fn active_cfg_attr_exports_the_record_and_inactive_does_not() {
    let active = scan_single(
        "active",
        "#[cfg_attr(all(), data)] pub struct Active { pub value: i32 }",
    );
    assert_eq!(record_ids(&active), ["cfg_attr_fixture::Active"]);

    let inactive = scan_single(
        "inactive",
        "#[cfg_attr(any(), data)] pub struct Inactive { pub value: i32 }",
    );
    assert!(record_ids(&inactive).is_empty());
}

#[test]
fn dependency_cfg_attr_uses_the_dependency_resolved_features() {
    let enabled = dependency_fixture(
        "dependency-enabled",
        "default = [\"feature-dependency/enabled-api\"]",
    )
    .scan("root-package", ActiveCfg::default())
    .expect("enabled scan");
    let ids = record_ids(&enabled);
    assert!(ids.contains(&"feature_dependency::Enabled".to_owned()));

    let disabled = dependency_fixture("dependency-disabled", "default = []")
        .scan("root-package", ActiveCfg::default())
        .expect("disabled scan");
    let ids = record_ids(&disabled);
    assert!(!ids.contains(&"feature_dependency::Enabled".to_owned()));
}

#[test]
fn multiple_and_nested_cfg_attrs_are_expanded() {
    let contract = scan_single(
        "multiple",
        "#[cfg_attr(all(), data, repr(C))] pub struct Multiple { pub value: i32 } \
         #[cfg_attr(all(), cfg_attr(all(), data))] pub struct Nested { pub value: i32 }",
    );
    let ids = record_ids(&contract);

    assert!(ids.contains(&"cfg_attr_fixture::Multiple".to_owned()));
    assert!(ids.contains(&"cfg_attr_fixture::Nested".to_owned()));
}

#[test]
fn cfg_attr_field_and_parameter_helpers_apply() {
    let contract = scan_single(
        "helpers",
        "#[data] pub struct Settings { #[cfg_attr(all(), default(41))] pub retries: u32 } \
         #[data(impl)] impl Settings { \
             pub fn retries(&self) -> u32 { self.retries } \
             #[cfg_attr(all(), skip)] pub fn hidden(&self) -> u32 { 0 } \
         } \
         #[export] pub fn configure(#[cfg_attr(all(), default(3))] level: u32) {}",
    );
    let settings = contract
        .records
        .iter()
        .find(|record| record.id.as_str() == "cfg_attr_fixture::Settings")
        .expect("Settings record");

    assert_eq!(
        settings.fields[0].default,
        Some(DefaultValue::Integer(IntegerLiteral::new(41, "41")))
    );
    assert_eq!(
        settings
            .methods
            .iter()
            .map(|method| method.name.spelling())
            .collect::<Vec<_>>(),
        ["retries"]
    );
    assert_eq!(
        contract.functions[0].parameters[0].default,
        Some(DefaultValue::Integer(IntegerLiteral::new(3, "3")))
    );
}

#[test]
fn a_literal_marker_and_an_active_gated_marker_conflict() {
    let error = scan_single_result(
        "conflict",
        "#[data] #[cfg_attr(all(), error)] pub struct Confused { pub code: i32 }",
    )
    .expect_err("conflicting markers must reject");

    assert_eq!(
        error,
        ScanError::ConflictingMarkers {
            first: "data".to_owned(),
            second: "error".to_owned(),
        }
    );
}

#[test]
fn cfg_attr_custom_ffi_marker_is_recognised_when_the_feature_is_on() {
    let contract = scan_single_with_cfg(
        "custom-ffi",
        "pub struct Email(pub String); \
         #[cfg_attr(feature = \"ffi\", boltffi::custom_ffi)] impl CustomFfiConvertible for Email { \
             type FfiRepr = String; \
             type Error = String; \
             fn into_ffi(&self) -> String { self.0.clone() } \
             fn try_from_ffi(value: String) -> Result<Self, String> { Ok(Self(value)) } \
         }",
        ActiveCfg::default().with_feature("ffi"),
    );

    assert_eq!(contract.customs.len(), 1);
    assert_eq!(contract.customs[0].id.as_str(), "cfg_attr_fixture::Email");
}

#[test]
fn cfg_attr_data_marker_on_a_tuple_newtype_produces_a_custom_type() {
    let contract = scan_single_with_cfg(
        "newtype",
        "#[cfg_attr(feature = \"ffi\", boltffi::data)] pub struct Key(pub u32); \
         #[export] pub fn echo(key: Key) -> Key { key }",
        ActiveCfg::default().with_feature("ffi"),
    );

    assert!(contract.records.is_empty());
    assert_eq!(contract.customs.len(), 1);
    assert_eq!(contract.customs[0].id.as_str(), "cfg_attr_fixture::Key");
    assert_eq!(
        contract.customs[0].repr,
        TypeExpr::Primitive(Primitive::U32)
    );
}

fn scan_single(name: &str, source: &str) -> SourceContract {
    scan_single_result(name, source).expect("scan")
}

fn scan_single_with_cfg(name: &str, source: &str, cfg: ActiveCfg) -> SourceContract {
    single_fixture(name, source)
        .scan("cfg-attr-fixture", cfg)
        .expect("scan")
}

fn scan_single_result(name: &str, source: &str) -> Result<SourceContract, ScanError> {
    single_fixture(name, source).scan("cfg-attr-fixture", ActiveCfg::default())
}

fn single_fixture(name: &str, source: &str) -> PackageFixture {
    PackageFixture::create(name, SINGLE_MANIFEST, source)
}

fn record_ids(contract: &SourceContract) -> Vec<String> {
    contract
        .records
        .iter()
        .map(|record| record.id.as_str().to_owned())
        .collect()
}

fn dependency_fixture(name: &str, root_features: &str) -> PackageFixture {
    let manifest = format!(
        r#"
[package]
name = "root-package"
version = "0.1.0"
edition = "2024"

[features]
{root_features}

[dependencies]
feature-dependency = {{ path = "dependency" }}

[workspace]
members = ["dependency"]
resolver = "2"
"#
    );
    let fixture = PackageFixture::create(name, &manifest, "#[export] pub fn ping() -> u32 { 1 }");
    let dependency_dir = fixture.manifest_dir.join("dependency");
    fs::create_dir_all(dependency_dir.join("src")).expect("dependency directories");
    PackageFixture::write(
        dependency_dir.join("Cargo.toml"),
        r#"
[package]
name = "feature-dependency"
version = "0.1.0"
edition = "2024"

[features]
enabled-api = []
"#,
    );
    PackageFixture::write(
        dependency_dir.join("src").join("lib.rs"),
        r#"
#[cfg_attr(feature = "enabled-api", data)]
pub struct Enabled {
    pub value: u32,
}
"#,
    );
    fixture
}
