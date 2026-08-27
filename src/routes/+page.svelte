<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { onMount } from 'svelte';

  interface PortEntry {
    port: number;
    port_type: 'npm' | 'docker';
    project: string;
    cmd: string;
    pid: number;
  }

  interface PortPref {
    custom_name?: string;
    pinned: boolean;
  }

  interface DisplayEntry extends PortEntry {
    displayName: string;
    pinned: boolean;
  }

  let rawPorts = $state<PortEntry[]>([]);
  let prefs = $state<Record<string, PortPref>>({});
  let portsLoading = $state(false);
  let scanError = $state(false);
  let editingPort = $state<number | null>(null);
  let editingValue = $state('');
  let autostart = $state(false);
  let autostartLoading = $state(false);

  let displayPorts = $derived.by(() => {
    const entries: DisplayEntry[] = rawPorts.map(p => {
      const pref = prefs[String(p.port)];
      return {
        ...p,
        displayName: pref?.custom_name || p.project || String(p.port),
        pinned: pref?.pinned ?? false,
      };
    });
    entries.sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      return a.port - b.port;
    });
    return entries;
  });

  async function fitWindow() {
    await new Promise(r => requestAnimationFrame(r));
    await new Promise(r => requestAnimationFrame(r));
    const appEl = document.querySelector('.app') as HTMLElement | null;
    const h = appEl?.getBoundingClientRect().height;
    if (!h) return;
    const win = getCurrentWindow();
    await win.setSize(new LogicalSize(360, Math.ceil(h)));
  }

  $effect(() => {
    // re-fit the window any time rendered content height could change,
    // not just after a port scan (pin/rename reorder the list too)
    portsLoading;
    scanError;
    displayPorts;
    fitWindow();
  });

  async function loadPorts() {
    portsLoading = true;
    scanError = false;
    try {
      rawPorts = await invoke<PortEntry[]>('get_ports');
    } catch {
      rawPorts = [];
      scanError = true;
    } finally {
      portsLoading = false;
    }
  }

  async function loadPrefs() {
    try {
      prefs = await invoke<Record<string, PortPref>>('get_prefs');
    } catch {
      prefs = {};
    }
  }

  async function savePrefs() {
    try { await invoke('save_prefs', { prefs }); } catch {}
  }

  async function togglePin(port: number) {
    const key = String(port);
    const current = prefs[key] ?? { pinned: false };
    prefs = { ...prefs, [key]: { ...current, pinned: !current.pinned } };
    await savePrefs();
  }

  function startEdit(entry: DisplayEntry) {
    editingPort = entry.port;
    editingValue = entry.displayName;
  }

  async function commitEdit(port: number) {
    const key = String(port);
    const current = prefs[key] ?? { pinned: false };
    const trimmed = editingValue.trim();
    prefs = { ...prefs, [key]: { ...current, custom_name: trimmed || undefined } };
    editingPort = null;
    await savePrefs();
  }

  function cancelEdit() { editingPort = null; }

  async function openPort(port: number) {
    await openUrl(`http://localhost:${port}`);
  }

  async function loadAutostart() {
    try { autostart = await isEnabled(); } catch { autostart = false; }
  }

  async function toggleAutostart() {
    autostartLoading = true;
    try {
      if (autostart) { await disable(); autostart = false; }
      else { await enable(); autostart = true; }
    } finally { autostartLoading = false; }
  }

  async function quitApp() { await invoke('quit_app'); }

  onMount(async () => {
    await Promise.all([loadAutostart(), loadPrefs()]);
    await loadPorts();
    const win = getCurrentWindow();
    win.onFocusChanged((event) => {
      if (event.payload) loadPorts();
    });
  });
</script>

<div class="app">
  <header>
    <div class="header-left">
      <span class="logo">⬡</span>
      <span class="title">Gooliya Port Bar</span>
      {#if !portsLoading && displayPorts.length > 0}
        <span class="badge">{displayPorts.length}</span>
      {/if}
    </div>
    <div class="header-actions">
      <button class="icon-btn" onclick={loadPorts} disabled={portsLoading} aria-label="Refresh">
        <svg class:spinning={portsLoading} xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/>
          <path d="M21 3v5h-5"/>
        </svg>
      </button>
      <button class="icon-btn quit" onclick={quitApp} aria-label="Quit">
        <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/>
          <polyline points="16 17 21 12 16 7"/>
          <line x1="21" y1="12" x2="9" y2="12"/>
        </svg>
      </button>
    </div>
  </header>

  <section>
    {#if portsLoading}
      <div class="state-center">
        <div class="spinner"></div>
        <span class="state-text">掃描中…</span>
      </div>
    {:else if scanError}
      <div class="state-center">
        <span class="state-text error">無法取得 port 資訊</span>
      </div>
    {:else if displayPorts.length === 0}
      <div class="state-center">
        <span class="state-text">目前沒有執行中的服務</span>
      </div>
    {:else}
      <ul class="port-list">
        {#each displayPorts as entry (entry.port)}
          <li class:pinned={entry.pinned}>
            <div class="port-row">
              <button class="pin-btn" class:active={entry.pinned} onclick={() => togglePin(entry.port)} aria-label={entry.pinned ? 'Unpin' : 'Pin'}>
                <svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill={entry.pinned ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/>
                </svg>
              </button>
              <div class="port-indicator {entry.port_type}"></div>
              <button class="port-main" onclick={() => openPort(entry.port)}>
                <div class="port-top">
                  <span class="port-num">:{entry.port}</span>
                  {#if editingPort === entry.port}
                    <!-- svelte-ignore a11y_autofocus -->
                    <input class="name-input" autofocus bind:value={editingValue}
                      onblur={() => commitEdit(entry.port)}
                      onkeydown={(e) => { if (e.key === 'Enter') commitEdit(entry.port); if (e.key === 'Escape') cancelEdit(); e.stopPropagation(); }}
                      onclick={(e) => e.stopPropagation()}
                    />
                  {:else}
                    <span class="project-name" title="點擊編輯名稱"
                      onclick={(e) => { e.stopPropagation(); startEdit(entry); }}
                      role="button" tabindex="0"
                      onkeydown={(e) => e.key === 'Enter' && startEdit(entry)}
                    >{entry.displayName}</span>
                  {/if}
                </div>
                <div class="port-sub">
                  <span class="cmd-pill {entry.port_type}">{entry.cmd}</span>
                  <span class="pid">PID {entry.pid}</span>
                </div>
              </button>
              <svg class="chevron" xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                <path d="M9 18l6-6-6-6"/>
              </svg>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <footer>
    <span class="footer-label">Launch at Login</span>
    <button class="toggle {autostart ? 'on' : ''}" onclick={toggleAutostart} disabled={autostartLoading} role="switch" aria-checked={autostart} aria-label="Launch at login">
      <span class="thumb"></span>
    </button>
  </footer>
</div>

<style>
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  :global(html, body) {
    background: transparent !important;
    width: 360px;
    overflow: hidden;
  }

  .app {
    width: 360px;
    font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Helvetica Neue', sans-serif;
    -webkit-font-smoothing: antialiased;
    background: rgba(26, 26, 28, 0.75);
    border-radius: 14px;
    border: 0.5px solid rgba(255, 255, 255, 0.1);
    overflow: hidden;
    color: #f5f5f7;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 11px 12px 10px;
    border-bottom: 0.5px solid rgba(255, 255, 255, 0.07);
  }

  .header-left { display: flex; align-items: center; gap: 6px; }
  .logo { font-size: 14px; color: #0a84ff; line-height: 1; }
  .title { font-size: 13px; font-weight: 700; letter-spacing: 0.05em; color: #f5f5f7; }

  .badge {
    font-size: 10px; font-weight: 600;
    background: rgba(10, 132, 255, 0.25); color: #0a84ff;
    border-radius: 99px; padding: 1px 6px;
  }

  .header-actions { display: flex; align-items: center; gap: 2px; }

  .icon-btn {
    display: flex; align-items: center; justify-content: center;
    width: 26px; height: 26px;
    border: none; background: none; border-radius: 7px;
    cursor: pointer; color: rgba(255, 255, 255, 0.3);
    transition: background 0.15s, color 0.15s;
  }
  .icon-btn:hover { background: rgba(255, 255, 255, 0.1); color: rgba(255, 255, 255, 0.8); }
  .icon-btn:disabled { opacity: 0.3; cursor: default; }
  .icon-btn.quit:hover { background: rgba(255, 59, 48, 0.15); color: #ff453a; }

  section { padding: 2px 0 4px; }

  .port-list { list-style: none; padding: 2px 0 2px; max-height: 400px; overflow-y: auto; }
  .port-list::-webkit-scrollbar { width: 3px; }
  .port-list::-webkit-scrollbar-track { background: transparent; }
  .port-list::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.15); border-radius: 3px; }

  li.pinned { background: rgba(255, 214, 10, 0.04); }

  .port-row {
    display: flex; align-items: center;
    padding: 6px 12px 6px 10px; gap: 7px;
    transition: background 0.12s;
  }
  .port-row:hover { background: rgba(255, 255, 255, 0.06); }
  .port-row:hover .chevron { color: rgba(255, 255, 255, 0.45); }

  .pin-btn {
    display: flex; align-items: center; justify-content: center;
    width: 20px; height: 20px; border: none; background: none;
    border-radius: 5px; cursor: pointer; color: rgba(255, 255, 255, 0.15);
    flex-shrink: 0; transition: color 0.15s, background 0.15s; padding: 0;
  }
  .pin-btn:hover { color: rgba(255, 214, 10, 0.7); background: rgba(255, 214, 10, 0.1); }
  .pin-btn.active { color: #ffd60a; }

  .port-indicator { width: 3px; height: 30px; border-radius: 2px; flex-shrink: 0; }
  .port-indicator.npm { background: linear-gradient(180deg, #0a84ff, #0055d4); }
  .port-indicator.docker { background: linear-gradient(180deg, #30d158, #1a9e3d); }

  .port-main {
    flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px;
    border: none; background: none; cursor: pointer; text-align: left; padding: 0; color: inherit;
  }
  .port-top { display: flex; align-items: baseline; gap: 7px; }

  .port-num {
    font-size: 13px; font-weight: 600; font-variant-numeric: tabular-nums;
    font-family: 'SF Mono', 'Menlo', monospace; color: #f5f5f7;
    letter-spacing: -0.3px; flex-shrink: 0;
  }

  .project-name {
    font-size: 12px; font-weight: 500; color: rgba(255, 255, 255, 0.55);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    cursor: text; border-radius: 3px; padding: 1px 3px; margin: -1px -3px;
    transition: background 0.12s, color 0.12s;
  }
  .project-name:hover { background: rgba(255, 255, 255, 0.1); color: rgba(255, 255, 255, 0.85); }

  .name-input {
    font-size: 12px; font-weight: 500; font-family: inherit;
    background: rgba(255, 255, 255, 0.12); border: 1px solid rgba(10, 132, 255, 0.6);
    border-radius: 4px; color: #f5f5f7; padding: 1px 5px; outline: none; width: 130px;
  }

  .port-sub { display: flex; align-items: center; gap: 5px; }

  .cmd-pill { font-size: 10px; font-weight: 500; padding: 1px 6px; border-radius: 99px; }
  .cmd-pill.npm { background: rgba(10, 132, 255, 0.18); color: #4fa8ff; }
  .cmd-pill.docker { background: rgba(48, 209, 88, 0.15); color: #32d74b; }

  .pid { font-size: 10px; color: rgba(255, 255, 255, 0.2); font-variant-numeric: tabular-nums; }
  .chevron { color: rgba(255, 255, 255, 0.18); flex-shrink: 0; transition: color 0.12s; }

  .state-center {
    display: flex; align-items: center; justify-content: center;
    gap: 8px; padding: 20px 16px;
  }
  .state-text { font-size: 12px; color: rgba(255, 255, 255, 0.3); }
  .state-text.error { color: #ff453a; }
  .spinner {
    width: 14px; height: 14px; border: 2px solid rgba(255, 255, 255, 0.1);
    border-top-color: #0a84ff; border-radius: 50%; animation: spin 0.7s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .spinning { animation: spin 0.7s linear infinite; }

  footer {
    display: flex; align-items: center; justify-content: space-between;
    padding: 10px 14px 12px;
    border-top: 0.5px solid rgba(255, 255, 255, 0.07);
  }
  .footer-label { font-size: 12px; color: rgba(255, 255, 255, 0.4); }

  .toggle {
    position: relative; width: 36px; height: 22px; border: none;
    border-radius: 11px; background: rgba(255, 255, 255, 0.12);
    cursor: pointer; transition: background 0.2s; padding: 0; flex-shrink: 0;
  }
  .toggle.on { background: #0a84ff; }
  .toggle:disabled { opacity: 0.4; cursor: default; }

  .thumb {
    position: absolute; top: 3px; width: 16px; height: 16px;
    background: white; border-radius: 50%;
    box-shadow: 0 1px 3px rgba(0,0,0,0.4);
    transition: left 0.2s cubic-bezier(0.25, 1, 0.5, 1);
  }
  .toggle:not(.on) .thumb { left: 3px; }
  .toggle.on .thumb { left: 17px; }
</style>
