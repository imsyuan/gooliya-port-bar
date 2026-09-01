# Gooliya Port Bar

<img src="./assets/logo.png" alt="Gooliya Port Bar logo" width="120" />

一個輕量的 macOS 選單列(menu bar)小工具,自動掃描本機正在監聽的 port —— npm/Vite/Next/Astro 開發伺服器以及 Docker container —— 集中顯示在一個小視窗裡,支援釘選、改名、移除、一鍵在瀏覽器開啟。

English: [README.md](./README.md) | 日本語: [README.ja.md](./README.ja.md)

官網: [port-bar.gooliya.com](https://port-bar.gooliya.com/)

## 功能

- 列出本機所有正在監聽的 port,包含 Node 開發伺服器(`vite`、`astro`、`next`、`tsx`/`ts-node` 等)與 Docker container
- 點擊 port 項目直接用預設瀏覽器開啟
- 釘選常用項目,固定在清單最上方
- 幫任一項目自訂顯示名稱
- 直接在清單裡移除服務 —— 終止對應的行程(npm/node)或停止 Docker container,執行前有二段式內嵌確認
- 視窗重新取得焦點時自動重新掃描,也可手動點擊重新整理
- 可選擇開機自動啟動
- 完全常駐選單列,不佔用 Dock
- 單一實例執行 —— 重複啟動 app 只會把現有視窗喚醒並取得焦點,不會開出重複的實例

## 安裝

1. **下載 Gooliya Port Bar** —— 到 [Releases](https://github.com/imsyuan/gooliya-port-bar/releases/latest) 下載最新的 `.dmg`,同一個檔案同時支援 **Apple Silicon** 與 **Intel** Mac。
2. **安裝** —— 把 `Gooliya Port Bar.app` 拖進 Applications 資料夾,雙擊打開。第一次執行時系統會提示確認,到「**系統設定 → 隱私權與安全性**」點一下「**強制打開**」,輸入密碼或用 Touch ID 確認即可,之後開啟都不會再詢問。

   <details>
   <summary>查看安裝步驟圖解</summary>
   <br>

   <img src="./assets/install-1.png" width="200" alt="把 Gooliya Port Bar.app 拖進 Applications 資料夾" />
   <img src="./assets/install-2.png" width="200" alt="第一次打開時系統顯示的確認提示" />
   <img src="./assets/install-3.png" width="200" alt="在系統設定的隱私權與安全性頁面點一下強制打開" />
   <img src="./assets/install-4.png" width="200" alt="再次確認要打開這個 App" />
   <img src="./assets/install-5.png" width="200" alt="輸入密碼或使用 Touch ID 完成授權" />

   </details>

3. **開始使用** —— 打開後 app 會常駐在選單列,本機正在跑的開發伺服器與 Docker container 會自動被掃描出來,點擊就能在瀏覽器打開。

## 技術架構

- [Tauri v2](https://tauri.app/)(Rust)負責原生視窗、tray icon 與視窗管理
- [SvelteKit](https://svelte.dev/) + [Svelte 5](https://svelte.dev/)(runes)負責介面,以靜態 SPA 方式建置
- Port 掃描透過 `lsof`,行程資訊透過 `ps` / `docker ps`(皆在 Rust 端執行)

## 開發

需要 Node.js,以及你的平台對應的 [Tauri 前置需求](https://v2.tauri.app/start/prerequisites/)(Rust 工具鏈等)。

這個 repo 附了一個 `pre-push` hook,用 [gitleaks](https://github.com/gitleaks/gitleaks)(`brew install gitleaks`)在 push 之前掃描有沒有外洩的密鑰。clone 完之後跑一次即可啟用:

```bash
git config core.hooksPath .githooks
```

安裝相依套件:

```bash
npm install
```

完整 app(Rust + 原生視窗)—— 要測試 Tauri command 一定要用這個:

```bash
npm run tauri dev
```

只跑前端,在一般瀏覽器分頁開(沒有 Tauri 後端,`invoke()` 呼叫會失敗):

```bash
npm run dev
```

前端型別檢查:

```bash
npm run check
```

## 建置

```bash
npm run tauri build
```

Rust 單元測試放在 `src-tauri/src/lib.rs`:

```bash
cd src-tauri
cargo test
```

## 授權

[MIT](./LICENSE)
