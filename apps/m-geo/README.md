# M- GEO

独立的本地商品 GEO 桌面工作台。Tauri 2 / React 19 / TypeScript / Rust / SQLite。
这是内部验收版；各平台签名、安装、真实模型和发布验证状态见 `../../docs/release-0.1.1.md`，不能把构建配置当作实机通过。

## 运行和构建

开发依赖：Node 24、pnpm 11.25.0、Rust 1.99.1。实际解析依赖由 pnpm-lock.yaml / Cargo.lock 固定。
macOS 安装 Xcode Command Line Tools；Windows 安装 Microsoft C++ Build Tools、Windows SDK 和 WebView2。

```sh
cd apps/m-geo
pnpm install --frozen-lockfile
pnpm test
pnpm typecheck
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --features desktop --all-targets -- -D warnings
pnpm tauri dev
```

Mac：`pnpm tauri build --bundles app,dmg`。
Windows：在真实 Windows 构建机执行 `./scripts/build-windows.ps1`（需 Python、7-Zip）；macOS 配好 cargo-xwin/LLVM/NSIS 后可运行 `bash scripts/build-windows-cross.sh`。本次已有 Windows x64 安装包与静态验包证据，实机项目仍按 `verification/windows-test-guide.md` 配合验收。
Intel Mac：安装 `x86_64-apple-darwin` Rust target，使用 `pnpm tauri build --target x86_64-apple-darwin --bundles app,dmg`。
根目录 `.github/workflows/m-geo-desktop.yml` 提供手动内部构建矩阵，未推送或运行远程 CI。正式分发前另行配置签名公证，未签名产物只做内部验收。

## 使用路径

1. 创建品牌，新增商品或导入 CSV。将有依据的参数写入“公开事实”，内部资料留在原始资料或私密字段。
2. 保存并确认当前商品事实。修改事实会撤销确认；过期事实须更新。
3. 内容工作室生成本地草稿，或在设置中自行配置模型后主动生成 AI 草稿。人工核对、编辑和审核后才可复制或导出。
4. 在外部渠道实际发布后登记真实网址。软件不会自动发布或宣称已被 AI 收录。
5. 在豆包、元宝、千问、百度手工提问，记录原始回答、引用和核对结果。消费者端和 API、基线和复测分开统计。

## 数据与备份

- macOS：`~/Library/Application Support/com.mantou.mgeo/m-geo.sqlite3`。
- Windows：由 Tauri app_data_dir 解析到当前用户应用数据目录中的 `com.mantou.mgeo`；可在设置页查看准确位置。
- SQLite 不宣称加密；macOS 工作目录权限 700、数据库 600。单机多品牌不等于多人账号权限。
- 模型 Key 存系统凭据库；保存配置不请求模型。真实调用仅在用户明确点击测试或生成时发生，可能计费。
- 备份使用 Argon2 派生密钥及 AES-256-GCM 认证加密；口令至少 10 字符，无找回功能。
- 备份不含模型配置、Key 引用和临时 AI 任务。恢复后需重新配置模型；取消或等待全部 AI 任务结束才能恢复。
- 恢复替换整个工作区；程序先校验所选备份，在候选副本完成初始化，再留存 `recovery/before-restore-*.mgeobak`（使用本次输入口令）并原子替换。成功提示显示快照位置，可打开快照目录。错误口令/破损/不兼容结构/业务身份错配均拒绝替换。
- 空白安装欢迎页可直接恢复，不需要先创建品牌。恢复期间锁定导航和品牌切换，完成后统一重新加载全部资料。
- 已损坏的业务记录仍会留在恢复前快照中供人工救援，不能将其当作已修复的有效备份。物理数据库损坏或磁盘无法写入时会拒绝替换，需先备份原文件并联系支持。
- 卸载应用不主动删除本地资料。重装使用原独立目录；不要用旧版程序打开未来格式数据库。

## 从 CNVP 迁移

本版接受明确的 JSON 交换格式，不直接读取 CNVP 数据库。字段与限制见 `../../docs/import-format.md`。CNVP 现有文件不一定符合该格式；需先做只读转换导出再导入，未执行真实旧库迁移。
只导入当前品牌的商品、问题、稿件、发布与观察；稳定来源实例 ID 防重复。同 SKU 冲突跳过连同其子记录，绝不覆盖客户数据。
历史版本、快照、审批归属保留在迁移历史，当前确认/审核清空待重新核对，不继承历史审批权限。

## 边界与许可

不包含自动抓取、自动发布、向量检索、多人同步、飞书、支付或账号收费。资料与模型调用均不依赖 CNVP 服务。
业务实现有 GEOFlow 移植来源，按 AGPL-3.0-only 提供对应源码与构建材料。见 `THIRD_PARTY_NOTICES.md`、`src-tauri/resources/LICENSE.txt`。

## 验收

`verification/browser-check.mjs` 是 Chromium →测试专用 stdio桥→真实 Rust/SQLite 流程，数据库临时创建并清理；不代替 Tauri IPC/原生对话框/Windows 实机验收。工具路径可由 `MGEO_PLAYWRIGHT`、`MGEO_CHROMIUM`、`MGEO_TEST_URL` 指定。
`cargo run --example performance --manifest-path src-tauri/Cargo.toml` 测隔离本机数据；`cargo run --example vault_smoke --manifest-path src-tauri/Cargo.toml -- --offline-native-smoke` 仅在系统凭据库创建并删除随机无效哨兵，不调用网络。
