use std::{fs, path::Path, process::Command, sync::Mutex, time::UNIX_EPOCH};

use boltffi_ast::PackageInfo;
use boltffi_backend::{GeneratedOutput, Target, bridge::c::CBridge, target::csharp::CSharpHost};
use boltffi_binding::{Bindings, Native, lower};

// NuGet first-run setup uses a process-global migration mutex.
static DOTNET_BUILD_LOCK: Mutex<()> = Mutex::new(());

fn bindings(source: &str) -> Bindings<Native> {
    let source = boltffi_scan::scan_file(
        syn::parse_str(source).expect("valid source"),
        PackageInfo::new("demo", None),
    )
    .expect("source should scan");
    lower::<Native>(&source).expect("source should lower")
}

fn target(host: CSharpHost) -> Target<CSharpHost, CBridge> {
    host.into_target().expect("C# target")
}

const CUSTOM_TYPE_DEFAULT: &str = include_str!("fixtures/source/records/custom_type_default.rs");

#[test]
fn csharp_target_compiles_a_regular_vec_u8_wire_api() {
    let bindings = bindings(
        r#"
        #[export]
        pub fn echo_bytes(value: Vec<u8>) -> Vec<u8> { value }
        "#,
    );
    let output = target(
        CSharpHost::new()
            .namespace("Company.Bindings")
            .expect("valid namespace")
            .native_library("demo_native"),
    )
    .render(&bindings)
    .expect("a regular Vec<u8> API should render");

    let module = output
        .files()
        .iter()
        .find(|file| file.path().as_path() == Path::new("Demo.cs"))
        .map(|file| file.contents())
        .expect("generated Demo.cs");
    assert!(module.contains("internal static extern FfiBuf BufFromBytes"));

    compile_csharp_with_dotnet_when_available(&output, "csharp-vec-u8-wire-runtime");
}

#[test]
fn csharp_target_compiles_standalone_wire_types() {
    let bindings = bindings(
        r#"
        #[data]
        pub struct Profile {
            pub name: String,
        }

        #[data]
        pub enum Shape {
            Empty,
            Label(String),
        }
        "#,
    );
    let output = target(
        CSharpHost::new()
            .namespace("Company.Bindings")
            .expect("valid namespace")
            .native_library("demo_native"),
    )
    .render(&bindings)
    .expect("standalone wire types should render");

    let module = output
        .files()
        .iter()
        .find(|file| file.path().as_path() == Path::new("Demo.cs"))
        .map(|file| file.contents())
        .expect("generated Demo.cs");
    assert!(module.contains("internal static extern FfiBuf BufFromBytes"));

    compile_csharp_with_dotnet_when_available(&output, "csharp-standalone-wire-types");
}

#[test]
fn csharp_target_compiles_records_containing_class_handles() {
    let bindings = bindings(
        r#"
        pub struct Token { value: i32 }

        #[export]
        impl Token {
            pub fn new() -> Self { Self { value: 42 } }
            pub fn value(&self) -> i32 { self.value }
        }

        #[data]
        pub struct Response {
            pub token: Token,
        }

        #[export]
        pub fn respond() -> Response { Response { token: Token } }
        "#,
    );
    let output = target(CSharpHost::new())
        .render(&bindings)
        .expect("record with class handle should render");
    assert!(output.diagnostics().is_empty());
    run_csharp_handle_lifetime_when_available(&output);
}

#[test]
fn csharp_record_transfers_nonempty_class_to_rust() {
    let source = r#"
        use boltffi::*;
        use std::sync::atomic::{AtomicU32, Ordering};

        static DROPS: AtomicU32 = AtomicU32::new(0);
        pub struct Token { value: i32 }
        impl Drop for Token {
            fn drop(&mut self) { DROPS.fetch_add(1, Ordering::SeqCst); }
        }
        #[export]
        impl Token {
            pub fn new(value: i32) -> Self { Self { value } }
            pub fn value(&self) -> i32 { self.value }
        }
        #[data]
        pub struct Response { pub token: Token, pub marker: i32 }
        #[data]
        pub struct Batch { pub tokens: Vec<Token>, pub maybe: Option<Token> }
        #[data]
        pub struct Keyed { pub tokens: std::collections::BTreeMap<String, Token> }
        #[export]
        pub fn consume_keyed(keyed: Keyed) -> i32 {
            keyed.tokens.get("first").map_or(0, |token| token.value)
                + keyed.tokens.get("second").map_or(0, |token| token.value)
        }
        #[export]
        pub fn make_keyed() -> Keyed {
            Keyed { tokens: std::collections::BTreeMap::from([
                ("first".to_string(), Token { value: 40 }),
                ("second".to_string(), Token { value: 50 }),
            ]) }
        }
        #[export]
        pub fn consume_batch(batch: Batch) -> i32 {
            batch.tokens.iter().map(|token| token.value).sum::<i32>()
                + batch.maybe.as_ref().map_or(0, |token| token.value)
        }
        #[export]
        pub fn make_batch() -> Batch {
            Batch {
                tokens: vec![Token { value: 10 }, Token { value: 20 }],
                maybe: Some(Token { value: 30 }),
            }
        }
        #[export]
        pub fn consume(response: Response) -> i32 { response.token.value + response.marker }
        #[export]
        pub fn make_response(value: i32) -> Response {
            Response { token: Token { value }, marker: 5 }
        }
        #[export]
        pub fn drop_count() -> u32 { DROPS.load(Ordering::SeqCst) }
        #[export]
        pub fn rejects_zero_handle() -> bool {
            matches!(
                boltffi::__private::wire::decode::<Response>(&[0; 12]),
                Err(boltffi::__private::wire::DecodeError::InvalidValue(
                    boltffi::__private::wire::InvalidWireValue::ClassHandle
                ))
            )
        }
        #[export]
        pub fn failed_collection_decode_releases_handles() -> bool {
            let before = drop_count();
            let handle = __boltffi_expansion::__BoltffiTokenHandle::new(Token { value: 99 });
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&2u32.to_le_bytes());
            bytes.extend_from_slice(&(handle as usize as u64).to_le_bytes());
            bytes.extend_from_slice(&0u64.to_le_bytes());
            let rejected = matches!(
                boltffi::__private::wire::decode::<Vec<Token>>(&bytes),
                Err(boltffi::__private::wire::DecodeError::InvalidValue(
                    boltffi::__private::wire::InvalidWireValue::ClassHandle
                ))
            );
            rejected && drop_count() == before + 1
        }
    "#;
    let bindings = bindings(source);
    let output = target(CSharpHost::new())
        .render(&bindings)
        .expect("class record API should render");
    assert!(output.diagnostics().is_empty());
    run_csharp_rust_handoff_when_available(&output, source);
}

#[test]
fn csharp_target_qualifies_a_class_method_named_after_its_return_record() {
    let bindings = bindings(
        r#"
        #[data]
        pub struct ServerInfo {
            pub version: String,
        }

        pub struct ParseClient {
            id: i32,
        }

        #[export]
        impl ParseClient {
            pub fn new(id: i32) -> Self { Self { id } }

            pub fn server_info(&self) -> Result<ServerInfo, String> {
                Ok(ServerInfo { version: "1".to_string() })
            }
        }

        #[export]
        pub fn apply_server_info(
            f: impl Fn(ServerInfo) -> ServerInfo,
            value: ServerInfo,
        ) -> ServerInfo {
            f(value)
        }
        "#,
    );
    let output = target(
        CSharpHost::new()
            .namespace("Company.Bindings")
            .expect("valid namespace")
            .native_library("demo_native"),
    )
    .render(&bindings)
    .expect("a class method named after its return record should still render");

    assert!(
        output.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        output.diagnostics()
    );

    let class = output
        .files()
        .iter()
        .find(|file| file.path().as_path() == Path::new("ParseClient.cs"))
        .map(|file| file.contents())
        .expect("generated ParseClient.cs");

    assert!(
        class.contains("return global::Company.Bindings.ServerInfo.Decode(resultReader);"),
        "expected the self-named ServerInfo() method to qualify its own Decode call:\n{class}"
    );
    assert!(
        !class.contains("return ServerInfo.Decode(resultReader);"),
        "an unqualified Decode call inside ServerInfo() would resolve to the method group, not \
         the type (CS0119):\n{class}"
    );

    compile_csharp_with_dotnet_when_available(&output, "csharp-sibling-shadow-smoke");
}

#[test]
fn csharp_target_qualifies_a_free_function_named_after_its_return_record() {
    let bindings = bindings(
        r#"
        #[data]
        pub struct ServerInfo {
            pub version: String,
        }

        #[export]
        pub fn server_info() -> Result<ServerInfo, String> {
            Ok(ServerInfo { version: "1".to_string() })
        }

        #[export]
        pub fn apply_server_info(
            f: impl Fn(ServerInfo) -> ServerInfo,
            value: ServerInfo,
        ) -> ServerInfo {
            f(value)
        }
        "#,
    );
    let output = target(
        CSharpHost::new()
            .namespace("Company.Bindings")
            .expect("valid namespace")
            .native_library("demo_native"),
    )
    .render(&bindings)
    .expect("a free function named after its return record should still render");

    assert!(
        output.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        output.diagnostics()
    );

    let module = output
        .files()
        .iter()
        .find(|file| file.path().as_path() == Path::new("Demo.cs"))
        .map(|file| file.contents())
        .expect("generated Demo.cs");

    assert!(
        module.contains("return global::Company.Bindings.ServerInfo.Decode(resultReader);"),
        "expected the self-named ServerInfo() free function to qualify its own Decode call:\n{module}"
    );

    compile_csharp_with_dotnet_when_available(&output, "csharp-free-function-shadow-smoke");
}

#[test]
fn csharp_target_renders_custom_type_defaults_through_representations() {
    let bindings = bindings(CUSTOM_TYPE_DEFAULT);
    let output = target(
        CSharpHost::new()
            .namespace("Company.Bindings")
            .expect("valid namespace")
            .native_library("demo_native"),
    )
    .render(&bindings)
    .expect("custom type defaults should render");
    let config = output
        .files()
        .iter()
        .find(|file| file.path().as_path() == Path::new("DeviationConfig.cs"))
        .map(|file| file.contents())
        .expect("generated DeviationConfig.cs");

    assert!(
        config.contains("public DeviationConfig()\n            : this(new LengthFfi(1500.0))"),
        "{config}"
    );
    compile_csharp_with_dotnet_when_available(&output, "csharp-custom-type-default");
}

fn run_csharp_rust_handoff_when_available(output: &GeneratedOutput, source: &str) {
    if Command::new("dotnet").arg("--version").output().is_err() {
        return;
    }
    let directory = std::env::temp_dir().join(format!(
        "csharp-rust-handoff-{}",
        UNIX_EPOCH.elapsed().expect("system clock").as_nanos()
    ));
    let crate_dir = directory.join("native");
    fs::create_dir_all(crate_dir.join("src")).expect("create native fixture");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    fs::write(
        crate_dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [lib]\ncrate-type = [\"cdylib\"]\n\
             [dependencies]\nboltffi = {{ path = {:?} }}\n",
            workspace.join("boltffi")
        ),
    )
    .expect("write fixture manifest");
    fs::write(
        crate_dir.join("build.rs"),
        r#"fn main() {
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-env=BOLTFFI_BINDING_EXPANSION=1");
    println!("cargo:rustc-env=BOLTFFI_BINDING_EXPANSION_ROOT={root}");
    println!("cargo:rustc-env=BOLTFFI_BINDING_EXPANSION_SOURCE={root}/src/lib.rs");
    println!("cargo:rustc-env=BOLTFFI_BINDING_EXPANSION_SURFACE=native");
}"#,
    )
    .expect("write fixture build script");
    fs::write(
        crate_dir.join("src/lib.rs"),
        format!("{source}\npub use __boltffi_expansion::*;\n"),
    )
    .expect("write fixture Rust source");
    let rust = Command::new("cargo")
        .args(["build", "--offline", "--manifest-path"])
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", workspace.join("target"))
        .output()
        .expect("build Rust fixture");
    assert!(
        rust.status.success(),
        "Rust fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );
    let src = directory.join("cs");
    fs::create_dir_all(&src).expect("create C# fixture");
    for file in output.files().iter().filter(|file| {
        file.path()
            .as_path()
            .extension()
            .is_some_and(|ext| ext == "cs")
    }) {
        let path = src.join(file.path().as_path());
        fs::create_dir_all(path.parent().expect("generated file parent")).unwrap();
        fs::write(path, file.contents()).expect("write generated C#");
    }
    fs::write(
        directory.join("Smoke.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <Nullable>enable</Nullable>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
  </PropertyGroup>
</Project>"#,
    )
    .expect("write C# fixture project");
    fs::write(
        src.join("Program.cs"),
        r#"using System;
using Demo;

var token = new Token(42);
var response = new Response(token, 3);
if (global::Demo.Demo.Consume(response) != 45) throw new Exception("Rust did not receive record fields");
if (token.Handle != 0) throw new Exception("C# retained transferred handle");
token.Dispose();
if (global::Demo.Demo.DropCount() != 1) throw new Exception("Rust did not drop token exactly once");
try { global::Demo.Demo.Consume(response); throw new Exception("double transfer succeeded"); }
catch (ObjectDisposedException) { }
if (global::Demo.Demo.DropCount() != 1) throw new Exception("double drop");
var returned = global::Demo.Demo.MakeResponse(77);
if (returned.Marker != 5 || returned.Token.Value() != 77) throw new Exception("C# did not receive Rust record fields");
if (global::Demo.Demo.DropCount() != 1) throw new Exception("Rust dropped transferred token early");
returned.Token.Dispose();
returned.Token.Dispose();
if (global::Demo.Demo.DropCount() != 2) throw new Exception("Rust token was not released exactly once");
if (!global::Demo.Demo.RejectsZeroHandle()) throw new Exception("Rust accepted a null class handle");
var one = new Token(1);
var two = new Token(2);
var three = new Token(3);
if (global::Demo.Demo.ConsumeBatch(new Batch(new[] { one, two }, three)) != 6)
    throw new Exception("Rust did not receive collection handles");
if (one.Handle != 0 || two.Handle != 0 || three.Handle != 0)
    throw new Exception("C# retained collection handles");
one.Dispose(); two.Dispose(); three.Dispose();
if (global::Demo.Demo.DropCount() != 5) throw new Exception("Rust did not drop collection handles");
var batch = global::Demo.Demo.MakeBatch();
if (batch.Tokens.Length != 2 || batch.Tokens[0].Value() != 10 ||
    batch.Tokens[1].Value() != 20 || batch.Maybe?.Value() != 30)
    throw new Exception("C# did not receive collection handles");
if (global::Demo.Demo.DropCount() != 5) throw new Exception("Rust dropped returned collection early");
foreach (var item in batch.Tokens) item.Dispose();
batch.Maybe?.Dispose();
if (global::Demo.Demo.DropCount() != 8) throw new Exception("returned handles not released once");
var first = new Token(4);
var second = new Token(5);
var keyed = new Keyed(new System.Collections.Generic.Dictionary<string, Token> {
    ["first"] = first, ["second"] = second
});
if (global::Demo.Demo.ConsumeKeyed(keyed) != 9) throw new Exception("Rust did not receive map handles");
if (first.Handle != 0 || second.Handle != 0 || global::Demo.Demo.DropCount() != 10)
    throw new Exception("map handles were not transferred");
var returnedMap = global::Demo.Demo.MakeKeyed();
if (returnedMap.Tokens["first"].Value() != 40 || returnedMap.Tokens["second"].Value() != 50)
    throw new Exception("C# did not receive map handles");
if (global::Demo.Demo.DropCount() != 10) throw new Exception("Rust dropped map handles early");
foreach (var item in returnedMap.Tokens.Values) item.Dispose();
if (global::Demo.Demo.DropCount() != 12) throw new Exception("map handles not released once");
if (!global::Demo.Demo.FailedCollectionDecodeReleasesHandles())
    throw new Exception("failed collection decode leaked or accepted a handle");
if (global::Demo.Demo.DropCount() != 13) throw new Exception("failed decode drop count mismatch");
"#,
    )
    .expect("write C# handoff assertions");
    let run = Command::new("dotnet")
        .args(["run", "--project"])
        .arg(directory.join("Smoke.csproj"))
        .env("LD_LIBRARY_PATH", workspace.join("target/debug"))
        .output()
        .expect("run C# Rust handoff");
    assert!(
        run.status.success(),
        "C# Rust handoff failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    fs::remove_dir_all(directory).expect("remove fixture");
}

fn run_csharp_handle_lifetime_when_available(output: &GeneratedOutput) {
    compile_csharp_with_dotnet(output, "csharp-record-class-handle", true);
}

fn compile_csharp_with_dotnet_when_available(output: &GeneratedOutput, prefix: &str) {
    compile_csharp_with_dotnet(output, prefix, false);
}

fn compile_csharp_with_dotnet(output: &GeneratedOutput, prefix: &str, run_lifetime: bool) {
    let _dotnet_build_guard = DOTNET_BUILD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if Command::new("dotnet").arg("--version").output().is_err() {
        return;
    }

    let directory = std::env::temp_dir().join(format!(
        "{prefix}-{}",
        UNIX_EPOCH.elapsed().expect("system clock").as_nanos()
    ));
    let src = directory.join("src");
    fs::create_dir_all(&src).expect("create dotnet smoke src directory");
    for file in output.files().iter().filter(|file| {
        file.path()
            .as_path()
            .extension()
            .is_some_and(|ext| ext == "cs")
    }) {
        let path = src.join(file.path().as_path());
        fs::create_dir_all(path.parent().expect("generated C# parent"))
            .expect("create generated C# parent directory");
        fs::write(&path, file.contents()).expect("write generated C# source");
    }
    fs::write(
        directory.join("Smoke.csproj"),
        format!(
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>{}</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <Nullable>enable</Nullable>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
  </PropertyGroup>
</Project>
"#,
            if run_lifetime { "Exe" } else { "Library" }
        ),
    )
    .expect("write smoke csproj");

    if run_lifetime {
        fs::write(
            src.join("Program.cs"),
            r#"using System;
using System.Runtime.InteropServices;
using Demo;

internal static class Program
{
    [DllImport("demo")] private static extern ulong make_token(int value);
    [DllImport("demo")] private static extern int release_count();
    [DllImport("demo")] private static extern int released_value();

    private static Response Decode(ulong handle)
    {
        nint memory = Marshal.AllocHGlobal(8);
        try
        {
            Marshal.WriteInt64(memory, unchecked((long)handle));
            return Response.Decode(new WireReader(memory, 8));
        }
        finally { Marshal.FreeHGlobal(memory); }
    }

    private static void Check(bool condition)
    {
        if (!condition) throw new Exception("handle ownership assertion failed");
    }

    private static void Main()
    {
        ulong raw = make_token(42);
        var record = Decode(raw);
        Check(record.Token.Handle == raw);
        var writer = new WireWriter();
        record.Encode(writer);
        Check(record.Token.Handle == 0);
        record.Token.Dispose();
        Check(release_count() == 0);
        Check(BitConverter.ToUInt64(writer.ToArray()) == raw);
        try { record.Encode(new WireWriter()); throw new Exception("double transfer succeeded"); }
        catch (ObjectDisposedException) { }
        var received = Decode(raw);
        Check(received.Token.Handle == raw);
        received.Token.Dispose();
        received.Token.Dispose();
        Check(release_count() == 1 && released_value() == 42);
        try { Decode(0); throw new Exception("null handle accepted"); }
        catch (ArgumentException) { }
    }
}
"#,
        )
        .expect("write handle lifetime program");
        fs::write(
            directory.join("native.c"),
            r#"#include <stdint.h>
#include <stdlib.h>
static int count;
static int last_value;
uint64_t make_token(int value) {
    int *token = malloc(sizeof(int));
    *token = value;
    return (uint64_t)(uintptr_t)token;
}
void boltffi_release_class_demo_token(uint64_t handle) {
    int *token = (int *)(uintptr_t)handle;
    last_value = *token;
    count++;
    free(token);
}
int release_count(void) { return count; }
int released_value(void) { return last_value; }
"#,
        )
        .expect("write native handle fixture");
        let cc = Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(directory.join("libdemo_native.so"))
            .arg(directory.join("native.c"))
            .output()
            .expect("compile native fixture");
        assert!(
            cc.status.success(),
            "{}",
            String::from_utf8_lossy(&cc.stderr)
        );
        fs::copy(
            directory.join("libdemo_native.so"),
            directory.join("libdemo.so"),
        )
        .expect("provide generated library name");
    }

    let build = Command::new("dotnet")
        .arg("build")
        .arg(directory.join("Smoke.csproj"))
        .arg("--nologo")
        .env("APPDATA", directory.join("appdata"))
        .output()
        .expect("dotnet build should execute");
    assert!(
        build.status.success(),
        "generated C# failed to build:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    if run_lifetime {
        let run = Command::new("dotnet")
            .arg("run")
            .arg("--no-build")
            .arg("--project")
            .arg(directory.join("Smoke.csproj"))
            .env("LD_LIBRARY_PATH", &directory)
            .output()
            .expect("run handle lifetime program");
        assert!(
            run.status.success(),
            "handle lifetime failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }
    fs::remove_dir_all(&directory).expect("remove dotnet smoke directory");
}
