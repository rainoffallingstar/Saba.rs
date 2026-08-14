# 打包与发布调研（Saba.rs）

> 状态：调研记录，尚未实现。实际打包脚本在 Beta 门槛临近时落地。

## 目标平台

- macOS（Apple Silicon 与 Intel）
- Windows 10/11
- Linux（AppImage 主目标；Flatpak 需单独验证，参考原 Sabaki 的 Flatpak CI）

## 渲染后端与打包的关系

- macOS：`gpui` 使用 `macos-blade` feature（blade/WGSL 后端，无需完整 Xcode）。
  打包为 `.app` bundle 时不依赖 Metal 编译；运行时 blade 走 Metal API。
- Windows/Linux：gpui 默认后端（x11/wayland/windows）。Linux 需要
  `libxkbcommon`/`libxcb`/`fontconfig` 等运行时库（CI 已装编译依赖；
  发布包需携带或声明依赖）。

## 可行的打包路线

| 平台 | 方案 | 备注 |
|---|---|---|
| macOS | `cargo build --release` + 手工 .app bundle（Info.plist、icon、`Contents/MacOS`）→ `dmg`（`create-dmg` 或 `hdiutil`） | 无 tauri 的 bundler；GPUI 生态无官方打包器，用脚本化 bundle 最简单 |
| Windows | `cargo build --release` → NSIS/Inno Setup installer 或便携 zip | 无签名时 SmartScreen 警告；签名需证书 |
| Linux | AppImage（`appimage-builder` 或手工 AppDir） | 需打包 fontconfig/xkbcommon 依赖；Flatpak 需 manifest + 单独验证 |

## 签名 / 公证 / 更新

- macOS：`codesign` + `notarytool`（需 Developer ID 证书）；GPUI 二进制是
  普通 Mach-O，无额外签名复杂度。
- Windows：Authenticode 证书（EV 或 OV）。
- 更新：自建更新服务器（`updater` 协议）或 GitHub Releases 手工更新；
  无现成自动更新库被验证（tauri-updater 不可复用，因无 tauri 运行时）。

## 待办（Beta 前）

1. CI 增加 release 构建 job（三个平台 `cargo build --release` + 产物上传）。
2. macOS bundle 脚本 + dmg。
3. Linux AppImage 构建（含依赖收集）；Flatpak manifest 实验。
4. Windows installer 实验。
5. 性能基准与启动时间对比（对照 Electron 参考版）。

## 参考

- 原 Sabaki 的 Flatpak/AppImage CI（SabakiHQ/Sabaki `.github/workflows`）。
- gpui 0.2.2 平台后端要求（`crates.io` gpui 包 features）。
