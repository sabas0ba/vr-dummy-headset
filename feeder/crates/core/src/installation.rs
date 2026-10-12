//! SteamVR driver registration shared by the CLI and GUI.
//!
//! Only vrpathreg writes SteamVR's registry. Registration does not launch or
//! terminate SteamVR, and does not imply that its runtime has loaded the driver.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Requested registration operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Read the current registration without changing it.
    Status,
    /// Register the selected driver directory.
    Register,
    /// Remove only the selected directory's registration.
    Unregister,
}

/// Optional locations; otherwise use SteamVR's registry and the local build.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Directory containing driver.vrdrivermanifest.
    pub driver_dir: Option<PathBuf>,
    /// SteamVR installation directory, overriding the registered runtime.
    pub steamvr_dir: Option<PathBuf>,
}

/// Registration state, separate from the feeder's live connection state.
#[derive(Clone, Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)] // Independent facts in the public JSON schema.
pub struct Status {
    /// Selected driver directory.
    pub driver_dir: PathBuf,
    /// SteamVR installation used for vrpathreg.
    pub steamvr_dir: PathBuf,
    /// Whether the selected directory is registered.
    pub registered: bool,
    /// All registered directories whose manifest identifies vrdummy.
    pub registered_paths: Vec<PathBuf>,
    /// Whether the selected directory has the expected manifest and DLL.
    pub package_valid: bool,
    /// Whether vrpathreg exists at the selected runtime location.
    pub tool_available: bool,
    /// Whether this request actually changed the registration.
    pub changed: bool,
    /// A changed registration takes effect on the next SteamVR start.
    pub restart_required: bool,
}

impl Status {
    /// Serialize the result for the CLI's JSON envelope.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| e.to_string())
    }
}

#[derive(Deserialize)]
struct Registry {
    runtime: Vec<PathBuf>,
    #[serde(default)]
    external_drivers: Option<Vec<PathBuf>>,
}

fn registry_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("VR_PATHREG_OVERRIDE").filter(|p| !p.is_empty()) {
        return std::path::absolute(path).map_err(|e| e.to_string());
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("openvr/openvrpaths.vrpath"))
        .ok_or_else(|| "LOCALAPPDATA is unavailable; driver management requires Windows".to_owned())
}

fn read_registry(path: &Path) -> Result<Registry, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn manifest_name(path: &Path) -> Result<String, String> {
    let file = path.join("driver.vrdrivermanifest");
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: missing driver name", file.display()))
}

fn validate_package(path: &Path) -> Result<(), String> {
    if manifest_name(path)? != "vrdummy" {
        return Err("selected manifest is not the vrdummy driver".to_owned());
    }
    if !path.join("bin/win64/driver_vrdummy.dll").is_file() {
        return Err("bin/win64/driver_vrdummy.dll is missing; build the driver first".to_owned());
    }
    Ok(())
}

fn path_key(path: &Path) -> String {
    let absolute = path
        .canonicalize()
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    let text = absolute.to_string_lossy().to_string();
    if cfg!(windows) {
        text.trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    } else {
        text
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    path_key(left) == path_key(right)
}

fn inspect(options: &Options, registry: &Registry) -> Result<Status, String> {
    let paths = registry.external_drivers.as_deref().unwrap_or_default();
    let registered_paths: Vec<_> = paths
        .iter()
        .filter(|p| manifest_name(p).is_ok_and(|name| name == "vrdummy"))
        .cloned()
        .collect();
    let selected = if let Some(path) = &options.driver_dir {
        path.clone()
    } else {
        match registered_paths.as_slice() {
            [] => PathBuf::from("driver/build/msvc-x64/vrdummy"),
            [path] => path.clone(),
            _ => return Err("multiple vrdummy registrations; select one with --path".to_owned()),
        }
    };
    let driver_dir = std::path::absolute(selected).map_err(|e| e.to_string())?;
    let steamvr_dir = options
        .steamvr_dir
        .clone()
        .or_else(|| registry.runtime.first().cloned())
        .ok_or("SteamVR runtime path is not registered; specify --steamvr-dir")?;
    Ok(Status {
        registered: paths.iter().any(|p| same_path(p, &driver_dir)),
        package_valid: validate_package(&driver_dir).is_ok(),
        tool_available: steamvr_dir.join("bin/win64/vrpathreg.exe").is_file(),
        driver_dir,
        steamvr_dir,
        registered_paths,
        changed: false,
        restart_required: false,
    })
}

/// Execute one operation. Mutations are idempotent and verified by rereading
/// SteamVR's registry. The timeout bounds the vrpathreg child process.
pub fn execute(action: Action, options: &Options, timeout: Duration) -> Result<Status, String> {
    let registry = registry_path()?;
    execute_with(action, options, &registry, |tool, verb, path| {
        run_tool(tool, verb, path, &registry, timeout)
    })
}

fn execute_with(
    action: Action,
    options: &Options,
    registry_file: &Path,
    run: impl FnOnce(&Path, &str, &Path) -> Result<(), String>,
) -> Result<Status, String> {
    let before = inspect(options, &read_registry(registry_file)?)?;
    if action == Action::Status {
        return Ok(before);
    }
    // A readable manifest for another driver must never be modified.
    if before.driver_dir.join("driver.vrdrivermanifest").exists()
        && manifest_name(&before.driver_dir)? != "vrdummy"
    {
        return Err("selected manifest is not the vrdummy driver".to_owned());
    }
    let wanted = action == Action::Register;
    if wanted {
        validate_package(&before.driver_dir)?;
        if before
            .registered_paths
            .iter()
            .any(|p| !same_path(p, &before.driver_dir))
        {
            return Err(
                "vrdummy is registered at another path; unregister that path first".to_owned(),
            );
        }
    }
    if before.registered == wanted {
        return Ok(before);
    }
    let verb = if wanted { "adddriver" } else { "removedriver" };
    run(
        &before.steamvr_dir.join("bin/win64/vrpathreg.exe"),
        verb,
        &before.driver_dir,
    )?;
    // Keep the same target even after removal changes automatic selection.
    let pinned = Options {
        driver_dir: Some(before.driver_dir),
        steamvr_dir: Some(before.steamvr_dir),
    };
    let mut after = inspect(&pinned, &read_registry(registry_file)?)?;
    if after.registered != wanted {
        return Err(
            "vrpathreg completed but registration did not match; refresh status".to_owned(),
        );
    }
    after.changed = true;
    after.restart_required = true;
    Ok(after)
}

fn run_tool(
    tool: &Path,
    verb: &str,
    path: &Path,
    registry: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut command = Command::new(tool);
    // Bind reads and writes to the same file, including isolated integration tests.
    command.env("VR_PATHREG_OVERRIDE", registry);
    command
        .arg(verb)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", tool.display()))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "vrpathreg {verb} failed ({status}); refresh status"
                ));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => format!("vrpathreg: {e}"),
                    _ => "vrpathreg timed out; registration may have changed; refresh status"
                        .to_owned(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        root: PathBuf,
        driver: PathBuf,
        registry: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../.tmp")
                .join(format!(
                    "installation-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            let driver = root.join("driver with spaces 日本語");
            std::fs::create_dir_all(driver.join("bin/win64")).unwrap();
            std::fs::write(
                driver.join("driver.vrdrivermanifest"),
                r#"{"name":"vrdummy"}"#,
            )
            .unwrap();
            std::fs::write(driver.join("bin/win64/driver_vrdummy.dll"), []).unwrap();
            let fixture = Self {
                registry: root.join("openvrpaths.vrpath"),
                root,
                driver,
            };
            fixture.write_paths(&[]);
            fixture
        }

        fn options(&self) -> Options {
            Options {
                driver_dir: Some(self.driver.clone()),
                steamvr_dir: None,
            }
        }

        fn write_paths(&self, paths: &[PathBuf]) {
            let value = serde_json::json!({"runtime": [self.root.join("SteamVR")], "external_drivers": paths});
            std::fs::write(&self.registry, value.to_string()).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn register_and_unregister_are_idempotent_and_preserve_other_paths() {
        let f = Fixture::new();
        let other = f.root.join("other-driver");
        f.write_paths(std::slice::from_ref(&other));
        let result = execute_with(
            Action::Register,
            &f.options(),
            &f.registry,
            |_, verb, path| {
                assert_eq!(verb, "adddriver");
                assert!(same_path(path, &f.driver));
                f.write_paths(&[other.clone(), path.to_owned()]);
                Ok(())
            },
        )
        .unwrap();
        assert!(result.registered && result.changed && result.restart_required);
        let repeat = execute_with(Action::Register, &f.options(), &f.registry, |_, _, _| {
            panic!("idempotent operation must not spawn")
        })
        .unwrap();
        assert!(!repeat.changed);
        let removed = execute_with(
            Action::Unregister,
            &f.options(),
            &f.registry,
            |_, verb, _| {
                assert_eq!(verb, "removedriver");
                f.write_paths(std::slice::from_ref(&other));
                Ok(())
            },
        )
        .unwrap();
        assert!(!removed.registered && removed.changed);
        assert_eq!(
            read_registry(&f.registry)
                .unwrap()
                .external_drivers
                .unwrap(),
            vec![other]
        );
        assert!(
            !execute_with(
                Action::Unregister,
                &f.options(),
                &f.registry,
                |_, _, _| panic!("already absent")
            )
            .unwrap()
            .changed
        );
    }

    #[test]
    fn finds_registered_package_without_explicit_path() {
        let f = Fixture::new();
        f.write_paths(std::slice::from_ref(&f.driver));
        let status = execute_with(
            Action::Status,
            &Options::default(),
            &f.registry,
            |_, _, _| panic!("read only"),
        )
        .unwrap();
        assert!(status.registered && status.package_valid);
        assert!(same_path(&status.driver_dir, &f.driver));
        let json: serde_json::Value = serde_json::from_str(&status.to_json().unwrap()).unwrap();
        assert_eq!(json["registered"], true);
    }

    #[test]
    fn rejects_other_driver_and_missing_dll() {
        let f = Fixture::new();
        std::fs::remove_file(f.driver.join("bin/win64/driver_vrdummy.dll")).unwrap();
        assert!(
            execute_with(
                Action::Register,
                &f.options(),
                &f.registry,
                |_, _, _| panic!("invalid package")
            )
            .is_err()
        );
        std::fs::write(
            f.driver.join("driver.vrdrivermanifest"),
            r#"{"name":"other"}"#,
        )
        .unwrap();
        f.write_paths(std::slice::from_ref(&f.driver));
        assert!(
            execute_with(
                Action::Unregister,
                &f.options(),
                &f.registry,
                |_, _, _| panic!("wrong driver")
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_duplicate_driver_locations() {
        let f = Fixture::new();
        let other = f.root.join("other-vrdummy");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(
            other.join("driver.vrdrivermanifest"),
            r#"{"name":"vrdummy"}"#,
        )
        .unwrap();
        f.write_paths(std::slice::from_ref(&other));
        assert!(
            execute_with(
                Action::Register,
                &f.options(),
                &f.registry,
                |_, _, _| panic!("duplicate")
            )
            .is_err()
        );
        f.write_paths(&[other, f.driver.clone()]);
        assert!(
            execute_with(
                Action::Status,
                &Options::default(),
                &f.registry,
                |_, _, _| panic!("read only")
            )
            .is_err()
        );
    }

    #[test]
    fn unregisters_explicit_stale_path_and_verifies_tool_result() {
        let f = Fixture::new();
        f.write_paths(std::slice::from_ref(&f.driver));
        std::fs::remove_dir_all(&f.driver).unwrap();
        assert!(
            execute_with(Action::Unregister, &f.options(), &f.registry, |_, _, _| Ok(
                ()
            ))
            .is_err()
        );
        let status = execute_with(Action::Unregister, &f.options(), &f.registry, |_, _, _| {
            f.write_paths(&[]);
            Ok(())
        })
        .unwrap();
        assert!(!status.registered && status.changed);
    }

    #[test]
    fn malformed_registry_is_an_error_not_unregistered() {
        let f = Fixture::new();
        std::fs::write(&f.registry, "invalid JSON").unwrap();
        assert!(
            execute_with(Action::Status, &f.options(), &f.registry, |_, _, _| panic!(
                "read only"
            ))
            .is_err()
        );
    }
}
