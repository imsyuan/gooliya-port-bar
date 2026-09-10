use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::Command;
use tauri::{menu::{Menu, MenuItem}, tray::TrayIconEvent, Manager, PhysicalPosition};
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortEntry {
    pub port: u16,
    pub port_type: String,
    pub project: String,
    pub cmd: String,
    pub pid: u32,
    pub uptime_seconds: u64,
    pub is_idle: bool,
}

/// A service is flagged as idle once it has been running longer than this
/// many seconds (4 hours). Hard-coded on purpose — see design.md decision
/// "殭屍門檻寫死為 4 小時，不做設定選項".
const IDLE_THRESHOLD_SECONDS: u64 = 4 * 60 * 60;

pub fn is_idle_from_uptime(uptime_seconds: u64) -> bool {
    uptime_seconds > IDLE_THRESHOLD_SECONDS
}

/// Parses a `ps -o etime=` value ("mm:ss", "hh:mm:ss", or "dd-hh:mm:ss")
/// into a total number of seconds. Returns `None` if the format is not
/// recognized, so callers can fall back to a safe default instead of
/// failing the whole scan.
fn parse_etime_seconds(etime: &str) -> Option<u64> {
    let etime = etime.trim();
    if etime.is_empty() {
        return None;
    }

    let (days, rest) = match etime.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, etime),
    };

    let parts: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [h, m, s] => (h.parse::<u64>().ok()?, m.parse::<u64>().ok()?, s.parse::<u64>().ok()?),
        [m, s] => (0, m.parse::<u64>().ok()?, s.parse::<u64>().ok()?),
        _ => return None,
    };

    Some(days * 86400 + hours * 3600 + minutes * 60 + seconds)
}

fn infer_cmd_label(full_cmd: &str) -> String {
    if full_cmd.contains("vite") {
        "vite dev".to_string()
    } else if full_cmd.contains("astro") {
        "astro dev".to_string()
    } else if full_cmd.contains("next") {
        "next dev".to_string()
    } else if full_cmd.contains("tsx") || full_cmd.contains("ts-node") {
        "tsx / ts-node server".to_string()
    } else if full_cmd.contains("hipki") {
        "hipkiLocalServer".to_string()
    } else {
        full_cmd
            .split_whitespace()
            .last()
            .unwrap_or("")
            .split('/')
            .last()
            .unwrap_or("")
            .to_string()
    }
}

fn infer_project_name(full_cmd: &str) -> String {
    let re = regex::Regex::new(r"/workspace/([^/]+)/").unwrap();
    re.captures(full_cmd)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// Returns `(container_name, status)` for the docker container publishing
/// `port`, where `status` is docker's raw `Status` string (e.g. "Up 3 hours").
fn get_docker_container_info(port: u16) -> Option<(String, String)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = Command::new("docker")
            .args(["ps", "--format", "{{.Names}}\t{{.Ports}}\t{{.Status}}"])
            .output();
        let _ = tx.send(result);
    });
    let output = rx.recv_timeout(std::time::Duration::from_secs(2)).ok()?.ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() == 3 && parts[1].contains(&format!(":{port}->")) {
            return Some((parts[0].to_string(), parts[2].to_string()));
        }
    }
    None
}

/// Parses a docker `Status` string (e.g. "Up 3 hours", "Up 2 days",
/// "Up About a minute") into a number of seconds. Returns `0` for anything
/// unrecognized (including a stopped container's "Exited ..." status),
/// which also yields `is_idle = false` — a safe default.
fn parse_docker_uptime_seconds(status: &str) -> u64 {
    let status = status.trim();
    let Some(rest) = status.strip_prefix("Up") else {
        return 0;
    };
    let rest = rest.split('(').next().unwrap_or(rest).trim();

    if rest.eq_ignore_ascii_case("About a minute") {
        return 60;
    }
    if rest.eq_ignore_ascii_case("Less than a second") {
        return 0;
    }

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let [amount, unit, ..] = parts.as_slice() else {
        return 0;
    };
    let Ok(amount) = amount.parse::<u64>() else {
        return 0;
    };
    let unit = unit.to_lowercase();

    let multiplier = if unit.starts_with("second") {
        1
    } else if unit.starts_with("minute") {
        60
    } else if unit.starts_with("hour") {
        3600
    } else if unit.starts_with("day") {
        86400
    } else if unit.starts_with("week") {
        604800
    } else if unit.starts_with("month") {
        2_592_000
    } else if unit.starts_with("year") {
        31_536_000
    } else {
        return 0;
    };

    amount * multiplier
}

/// Scans currently listening ports. Exposed as `pub` (not just via the
/// `#[tauri::command]` wrapper below) so the standalone `port-bar-mcp`
/// binary can reuse this exact scanning + idle-detection logic through the
/// crate's `rlib` output, instead of duplicating it. `kill_port_impl` below
/// is deliberately NOT `pub` — the MCP binary is a separate compilation
/// unit consuming only this crate's public API, so it structurally cannot
/// call a private kill function, regardless of what its own code does.
/// Checks whether `path` can be spawned at all (distinct from spawning
/// successfully and getting a non-zero exit code). Used by
/// `scan_ports_checked` to tell "the required tool is missing" apart from
/// "the tool ran and there's simply nothing to report".
fn check_command_available(path: &str) -> Result<(), String> {
    match Command::new(path).arg("-v").output() {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("找不到必要的系統工具：{path}")),
    }
}

/// Same scan as `scan_ports`, but for callers (like `port-bar-mcp`) that
/// need to distinguish "no ports found" from "lsof isn't available on this
/// machine" instead of getting an empty list either way.
pub fn scan_ports_checked() -> Result<Vec<PortEntry>, String> {
    check_command_available("/usr/sbin/lsof")?;
    Ok(scan_ports())
}

pub fn scan_ports() -> Vec<PortEntry> {
    let output = match Command::new("/usr/sbin/lsof")
        .args(["-i", "-P", "-n"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results: Vec<PortEntry> = Vec::new();
    let mut seen_ports: HashSet<u16> = HashSet::new();

    for line in stdout.lines() {
        if !line.contains("LISTEN") {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        let process_name = cols[0];
        let pid_str = cols[1];
        let addr = cols[8];

        let port = match addr.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) {
            Some(p) => p,
            None => continue,
        };

        if seen_ports.contains(&port) {
            continue;
        }

        let pid: u32 = pid_str.parse().unwrap_or(0);

        let is_node = process_name == "node" || process_name == "node.js";
        let is_docker = process_name == "docker"
            || process_name.starts_with("com.docker")
            || process_name.starts_with("com.docke");

        if is_node {
            let ps_output = Command::new("ps")
                .args(["-p", pid_str, "-o", "etime=,command="])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            let mut ps_parts = ps_output.splitn(2, char::is_whitespace);
            let etime_str = ps_parts.next().unwrap_or("");
            let full_cmd = ps_parts.next().unwrap_or("").trim_start().to_string();

            let uptime_seconds = parse_etime_seconds(etime_str).unwrap_or(0);
            let is_idle = is_idle_from_uptime(uptime_seconds);

            let project = if full_cmd.contains("hipki") {
                "hipki".to_string()
            } else {
                infer_project_name(&full_cmd)
            };

            let cmd = infer_cmd_label(&full_cmd);

            seen_ports.insert(port);
            results.push(PortEntry {
                port,
                port_type: "npm".to_string(),
                project,
                cmd,
                pid,
                uptime_seconds,
                is_idle,
            });
        } else if is_docker {
            let (container_name, status) = get_docker_container_info(port)
                .unwrap_or_else(|| ("docker".to_string(), String::new()));
            let uptime_seconds = parse_docker_uptime_seconds(&status);
            let is_idle = is_idle_from_uptime(uptime_seconds);

            seen_ports.insert(port);
            results.push(PortEntry {
                port,
                port_type: "docker".to_string(),
                project: container_name.clone(),
                cmd: "docker container".to_string(),
                pid,
                uptime_seconds,
                is_idle,
            });
        }
    }

    results.sort_by_key(|e| e.port);
    results
}

#[tauri::command]
async fn get_ports() -> Result<Vec<PortEntry>, String> {
    Ok(scan_ports())
}

fn find_pid_for_port(port: u16) -> Option<u32> {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-i", &format!(":{port}"), "-P", "-n", "-t"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next()?.trim().parse::<u32>().ok()
}

fn kill_port_impl(port: u16, port_type: &str, project: &str) -> Result<(), String> {
    match port_type {
        "npm" => {
            let pid = find_pid_for_port(port)
                .ok_or_else(|| format!("port {port} 已無服務在監聽,可能已經關閉"))?;
            let status = Command::new("kill")
                .arg(pid.to_string())
                .status()
                .map_err(|e| e.to_string())?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("終止行程 {pid} 失敗"))
            }
        }
        "docker" => {
            if project.is_empty() {
                return Err("找不到容器名稱,無法停止服務".to_string());
            }
            let output = Command::new("docker")
                .args(["stop", project])
                .output()
                .map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(if stderr.is_empty() {
                    format!("停止容器 {project} 失敗")
                } else {
                    stderr
                })
            }
        }
        other => Err(format!("不支援的服務類型:{other}")),
    }
}

#[tauri::command]
async fn kill_port(port: u16, port_type: String, project: String) -> Result<(), String> {
    kill_port_impl(port, &port_type, &project)
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}


#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PortPref {
    pub custom_name: Option<String>,
    pub pinned: bool,
}

#[tauri::command]
fn get_prefs(app: tauri::AppHandle) -> HashMap<String, PortPref> {
    let path = match app.path().app_config_dir() {
        Ok(dir) => dir.join("prefs.json"),
        Err(_) => return HashMap::new(),
    };
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn save_prefs(app: tauri::AppHandle, prefs: HashMap<String, PortPref>) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("prefs.json");
    let json = serde_json::to_string_pretty(&prefs).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                window.show().ok();
                window.set_focus().ok();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            let window = app.get_webview_window("main").unwrap();
            window.set_skip_taskbar(true).ok();
            apply_vibrancy(&window, NSVisualEffectMaterial::HudWindow, None, Some(14.0)).ok();

            let quit_item = MenuItem::with_id(app, "quit", "結束 Port Bar", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_item])?;

            let tray = app.tray_by_id("main").unwrap();
            tray.set_menu(Some(menu))?;
            tray.set_show_menu_on_left_click(false)?;

            app.on_menu_event(|app, event| {
                if event.id() == "quit" {
                    app.exit(0);
                }
            });

            tray.on_tray_icon_event(|tray_handle, event| {
                if let TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    button_state: tauri::tray::MouseButtonState::Up,
                    rect,
                    ..
                } = event
                {
                    let app = tray_handle.app_handle();
                    let window = app.get_webview_window("main").unwrap();

                    if window.is_visible().unwrap_or(false) {
                        window.hide().unwrap();
                    } else {
                        let scale = window.scale_factor().unwrap_or(2.0);
                        let win_width_physical = 360.0 * scale;

                        // rect.position / rect.size are enums (Physical or Logical), normalize to physical
                        let (rect_x, rect_y) = match rect.position {
                            tauri::Position::Physical(p) => (p.x as f64, p.y as f64),
                            tauri::Position::Logical(l) => (l.x * scale, l.y * scale),
                        };
                        let (rect_w, rect_h) = match rect.size {
                            tauri::Size::Physical(s) => (s.width as f64, s.height as f64),
                            tauri::Size::Logical(s) => (s.width * scale, s.height * scale),
                        };

                        // Center under tray icon, place just below menu bar bottom
                        let x = (rect_x + rect_w / 2.0 - win_width_physical / 2.0) as i32;
                        let y = (rect_y + rect_h + 5.0) as i32;

                        let pos = PhysicalPosition::new(x, y);
                        window.set_position(pos).unwrap();
                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(false) = event {
                if window.label() == "main" {
                    window.hide().ok();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_ports, get_prefs, save_prefs, quit_app, kill_port
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_cmd_label() {
        assert_eq!(infer_cmd_label("/path/to/vite/node_modules/.bin/vite"), "vite dev");
        assert_eq!(infer_cmd_label("node astro dev"), "astro dev");
        assert_eq!(infer_cmd_label("node next"), "next dev");
        assert_eq!(infer_cmd_label("npx tsx server.ts"), "tsx / ts-node server");
    }

    #[test]
    fn test_infer_project_name() {
        assert_eq!(
            infer_project_name("/Users/user/workspace/my-app/node_modules/.bin/vite"),
            "my-app"
        );
        assert_eq!(infer_project_name("/usr/local/bin/node server.js"), "");
    }

    #[test]
    fn test_dedup_and_sort() {
        let mut entries = vec![
            PortEntry { port: 5173, port_type: "npm".into(), project: "a".into(), cmd: "vite dev".into(), pid: 1, uptime_seconds: 0, is_idle: false },
            PortEntry { port: 3000, port_type: "npm".into(), project: "b".into(), cmd: "next dev".into(), pid: 2, uptime_seconds: 0, is_idle: false },
            PortEntry { port: 5173, port_type: "npm".into(), project: "dup".into(), cmd: "vite dev".into(), pid: 3, uptime_seconds: 0, is_idle: false },
        ];

        let mut seen: HashSet<u16> = HashSet::new();
        entries.retain(|e| seen.insert(e.port));
        entries.sort_by_key(|e| e.port);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].port, 3000);
        assert_eq!(entries[1].port, 5173);
        assert_eq!(entries[1].project, "a");
    }

    #[test]
    fn test_find_pid_for_port_resolves_current_holder() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let found = find_pid_for_port(port);
        assert_eq!(found, Some(std::process::id()));
        drop(listener);
    }

    fn free_port() -> u16 {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    #[test]
    fn test_find_pid_for_port_none_when_nothing_listening() {
        assert_eq!(find_pid_for_port(free_port()), None);
    }

    #[test]
    fn test_kill_port_impl_npm_missing_target_returns_error() {
        let result = kill_port_impl(free_port(), "npm", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_kill_port_impl_docker_missing_project_returns_error() {
        let result = kill_port_impl(3000, "docker", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_kill_port_impl_docker_nonexistent_container_returns_error() {
        // Uses a name that cannot collide with a real container, so this never
        // touches an actual running container even if docker is installed.
        let result = kill_port_impl(3000, "docker", "kill-port-service-test-nonexistent-container-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_kill_port_impl_unsupported_type_returns_error() {
        let result = kill_port_impl(3000, "unknown", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_etime_seconds_mm_ss() {
        assert_eq!(parse_etime_seconds("05:30"), Some(330));
    }

    #[test]
    fn test_parse_etime_seconds_hh_mm_ss() {
        assert_eq!(parse_etime_seconds("01:15:30"), Some(4530));
    }

    #[test]
    fn test_parse_etime_seconds_dd_hh_mm_ss() {
        assert_eq!(parse_etime_seconds("2-03:15:30"), Some(184530));
    }

    #[test]
    fn test_parse_etime_seconds_unparseable_returns_none() {
        assert_eq!(parse_etime_seconds(""), None);
        assert_eq!(parse_etime_seconds("not-a-time"), None);
    }

    #[test]
    fn test_parse_docker_uptime_seconds_hours_and_days() {
        assert_eq!(parse_docker_uptime_seconds("Up 3 hours"), 3 * 3600);
        assert_eq!(parse_docker_uptime_seconds("Up 2 days"), 2 * 86400);
    }

    #[test]
    fn test_parse_docker_uptime_seconds_unparseable_returns_zero() {
        assert_eq!(parse_docker_uptime_seconds(""), 0);
        assert_eq!(parse_docker_uptime_seconds("Exited (0) 5 minutes ago"), 0);
    }

    #[test]
    fn test_is_idle_from_uptime_boundary() {
        assert!(!is_idle_from_uptime(14399));
        assert!(!is_idle_from_uptime(14400));
        assert!(is_idle_from_uptime(14401));
    }

    #[test]
    fn test_check_command_available_missing_binary_returns_err() {
        let result = check_command_available("/definitely/not/a/real/path/xyz-test-binary");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_command_available_existing_binary_returns_ok() {
        // /usr/sbin/lsof is a hard runtime dependency of scan_ports() itself,
        // so it must exist wherever these tests run.
        let result = check_command_available("/usr/sbin/lsof");
        assert!(result.is_ok());
    }

    #[test]
    fn test_scan_ports_checked_returns_ok_when_lsof_present() {
        assert!(scan_ports_checked().is_ok());
    }
}
