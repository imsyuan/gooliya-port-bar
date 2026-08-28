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

fn get_docker_container_name(port: u16) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = Command::new("docker")
            .args(["ps", "--format", "{{.Names}}\t{{.Ports}}"])
            .output();
        let _ = tx.send(result);
    });
    let output = rx.recv_timeout(std::time::Duration::from_secs(2)).ok()?.ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() == 2 && parts[1].contains(&format!(":{port}->")) {
            return Some(parts[0].to_string());
        }
    }
    None
}

fn scan_ports() -> Vec<PortEntry> {
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
            let full_cmd = Command::new("ps")
                .args(["-p", pid_str, "-o", "command="])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();

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
            });
        } else if is_docker {
            let container_name = get_docker_container_name(port)
                .unwrap_or_else(|| "docker".to_string());

            seen_ports.insert(port);
            results.push(PortEntry {
                port,
                port_type: "docker".to_string(),
                project: container_name.clone(),
                cmd: "docker container".to_string(),
                pid,
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
            PortEntry { port: 5173, port_type: "npm".into(), project: "a".into(), cmd: "vite dev".into(), pid: 1 },
            PortEntry { port: 3000, port_type: "npm".into(), project: "b".into(), cmd: "next dev".into(), pid: 2 },
            PortEntry { port: 5173, port_type: "npm".into(), project: "dup".into(), cmd: "vite dev".into(), pid: 3 },
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
}
