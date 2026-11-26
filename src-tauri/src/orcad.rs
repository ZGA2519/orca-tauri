use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Url, WebviewWindow};

/// Preloaded into orcad so a crashed or force-quit shell never leaves it holding the profile lock.
/// Drops itself from execArgv first: forked daemons/workers would otherwise inherit it.
/// Polls the parent pid because orcad ends its own stdin stream, so a stdin-EOF watch misfires.
// ponytail: 1s pid poll; a recycled shell pid inside that window would keep orcad alive.
const EXIT_WHEN_SHELL_EXITS: &str = "data:text/javascript,\
const i=process.execArgv.indexOf('--import');if(i>=0)process.execArgv.splice(i,2);\
const p=process.ppid;\
setInterval(()=>{try{process.kill(p,0)}catch{process.kill(process.pid,'SIGTERM')}},1000).unref()";

/// The orcad Node process that serves Orca's runtime RPC and web client to this shell.
pub struct Sidecar(Mutex<Option<Child>>);

pub fn spawn(
    app: &AppHandle,
    window: WebviewWindow,
) -> Result<Sidecar, Box<dyn std::error::Error>> {
    let base = artifacts_root(app)?;
    let node = find_node(&base.join("runtimes"))
        .ok_or("pinned Node runtime missing; run `pnpm build:orcad`")?;
    // Own profile so this never contends with a running Electron Orca for locks or SQLite files.
    let profile = app.path().app_data_dir()?.join("profile");
    std::fs::create_dir_all(&profile)?;

    let mut command = Command::new(node);
    // A GUI-subsystem parent would otherwise flash a console window for node.exe.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .args(["--import", EXIT_WHEN_SHELL_EXITS])
        .arg(base.join("orcad").join("orcad.js"))
        .args([
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--json",
            "--web-client-root",
        ])
        .arg(base.join("web"))
        .env("ORCA_USER_DATA", &profile)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("orcad stdout unavailable")?;

    let app = app.clone();
    std::thread::spawn(move || {
        let mut navigated = false;
        // Keep draining after readiness so orcad never blocks on a full stdout pipe.
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if navigated {
                continue;
            }
            if let Some(url) = web_client_url(&line) {
                navigated = grant_native_access(&app, &url).is_ok() && window.navigate(url).is_ok();
            }
        }
        if !navigated {
            let _ = window.eval(
                "document.getElementById('status').textContent = 'Orca runtime failed to start. See logs.'",
            );
        }
    });

    Ok(Sidecar(Mutex::new(Some(child))))
}

impl Sidecar {
    pub fn stop(&self) {
        let Some(mut child) = self.0.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        #[cfg(unix)]
        {
            // SIGTERM lets orcad release its instance lock and stop its terminal daemon.
            unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        // ponytail: hard kill on Windows; use orcad's managed-stop command if lock cleanup matters there.
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Dev runs from the repo's build output; bundles ship the same tree as resources.
fn artifacts_root(app: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("out"));
    }
    Ok(app.path().resource_dir()?)
}

// ponytail: first runtime wins; `pnpm build:orcad` may leave older pins beside it until `out/` is cleaned.
fn find_node(runtimes: &Path) -> Option<PathBuf> {
    std::fs::read_dir(runtimes)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("node-"))
        .flat_map(|entry| {
            [
                entry.path().join("bin").join("node"),
                entry.path().join("node.exe"),
            ]
        })
        .find(|candidate| candidate.is_file())
}

/// Tauri IPC for this run's orcad origin only; the port changes every launch, so it can't be static config.
fn grant_native_access(app: &AppHandle, url: &Url) -> tauri::Result<()> {
    let origin = format!(
        "{}://{}:{}/*",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or_default()
    );
    let capability = serde_json::json!({
        "identifier": "orcad-web-client",
        "windows": ["main"],
        "remote": { "urls": [origin] },
        "permissions": [
            "core:event:default",
            "core:window:default",
            "core:window:allow-minimize",
            "core:window:allow-toggle-maximize",
            "core:webview:allow-set-webview-zoom",
            "dialog:allow-open",
            "notification:default",
            "allow-move-to-trash",
            "opener:allow-open-url",
            "opener:allow-reveal-item-in-dir",
            { "identifier": "opener:allow-open-path", "allow": [{ "path": "**" }] }
        ]
    });
    app.add_capability(capability.to_string())
}

fn web_client_url(line: &str) -> Option<Url> {
    let ready: serde_json::Value = serde_json::from_str(line).ok()?;
    if ready["type"] != "orca_server_ready" {
        return None;
    }
    if let Some(paired) = ready["pairing"]["webClientUrl"].as_str() {
        return Url::parse(paired).ok();
    }
    let endpoint = ready["endpoint"].as_str()?.replacen("ws", "http", 1);
    Url::parse(&format!("{endpoint}/web-index.html")).ok()
}

#[cfg(test)]
mod tests {
    use super::web_client_url;

    #[test]
    fn readiness_line_yields_paired_web_client_url() {
        let line = r#"{"type":"orca_server_ready","endpoint":"ws://127.0.0.1:5","pairing":{"webClientUrl":"http://127.0.0.1:5/web-index.html#pairing=x"}}"#;
        assert_eq!(
            web_client_url(line).unwrap().as_str(),
            "http://127.0.0.1:5/web-index.html#pairing=x"
        );
        let unpaired = r#"{"type":"orca_server_ready","endpoint":"wss://127.0.0.1:5","pairing":{"webClientUrl":null}}"#;
        assert_eq!(
            web_client_url(unpaired).unwrap().as_str(),
            "https://127.0.0.1:5/web-index.html"
        );
        assert!(web_client_url("[daemon] log line").is_none());
    }
}
