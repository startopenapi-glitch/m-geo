# M-GEO 0.15.0：开发与使用说明

M-GEO 是本地优先的电商商品 GEO 桌面工作台，使用 Tauri 2、React 19、TypeScript、Rust 与 SQLite。当前目录提供 0.15.0 源码；[仓库 Releases](https://github.com/startopenapi-glitch/m-geo/releases) 中最后的预编译安装包仍是 v0.1.1。

## 开发环境与运行

本地验证使用 Node 24、pnpm 11.25.0 和 Rust 1.99.1。macOS 需要 Xcode Command Line Tools；Windows 需要 Microsoft C++ Build Tools、Windows SDK 与 WebView2。

```sh
cd apps/m-geo
pnpm install --frozen-lockfile
pnpm test
pnpm typecheck
pnpm build
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --features desktop --all-targets -- -D warnings
pnpm tauri dev
```

构建桌面包：

- macOS Apple Silicon：`pnpm tauri build --bundles app`，之后可运行 `bash scripts/package-macos.sh` 制作本地 DMG。ad-hoc 签名不等同于 Developer ID 签名或 Apple 公证。
- Windows：在 Windows 构建机执行 `./scripts/build-windows.ps1`（需要 Python 和 7-Zip）。跨平台脚本 `scripts/build-windows-cross.sh` 需要额外配置 cargo-xwin、LLVM 和 NSIS。
- Intel Mac：安装 `x86_64-apple-darwin` Rust target，再以该 target 构建。0.15.0 的 Intel Mac 和 Windows 安装包没有随本次源码更新发布。

发布和验证边界见 [0.15.0 源码说明](../../docs/release-0.15.0.md)。本地构建不代表远程 CI 已运行。

## 使用流程

1. 创建品牌并录入商品资料。公开事实需有来源、可见范围与有效期；内部信息应标为私有。
2. 核验商品事实和购买信息，再确认当前版本。
3. 用模板编写本地草稿，或在设置中保存模型服务配置后主动生成候选内容。生成结果须人工核对、编辑和审核。
4. 将审核后的内容复制或导出，在目标渠道自行发布；之后再登记真实发布网址。
5. 在豆包、元宝、千问、百度等消费者入口手动查询，保存回答、引用和核对结论。缺测与未核实状态不会被当成推荐证据。

M-GEO 不自动抓取、登录或发布到外部平台；不会把本地导出误记成已上线，也不承诺首页推荐、收录、排名、流量或销售结果。

## 数据与模型

- macOS 默认工作区位于 `~/Library/Application Support/com.mantou.mgeo/`；Windows 路径由 Tauri 的用户数据目录确定，可在设置中查看。
- SQLite 数据库本身不宣称加密。工作区通常只供当前系统用户使用；单机多品牌不提供多人协作权限。
- API Key 存在操作系统凭据库中。保存配置不会调用模型；主动测试或生成时，所选商品/稿件内容可能发送至用户选择的服务并产生费用。
- 加密备份覆盖工作区数据，不包含模型配置、API Key 或临时 AI 任务。恢复会替换整个工作区，请在升级或恢复前先保留当前备份。
- 数据格式会随版本演进。新版工作区不要再用旧版程序打开；若需回退，应使用适配版本的备份恢复流程。卸载应用不会主动删除本地资料。

## 范围与许可

0.15.0 包含商品资料、内容审核导出、通用商品目录草稿、本地经营归因、批量运营、优化闭环、历史证据复核，以及按商品/问题查看推荐变化等功能。平台适配和真实效果需要在目标平台与真实商品上另行验证。

软件采用 AGPL-3.0-only；保留 GEOFlow 来源和第三方许可说明，见仓库根目录的 `LICENSE` 与 `THIRD_PARTY_NOTICES.md`。M-GEO 名称与商标不随代码许可开放。
