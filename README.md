<p align="center"><img src="apps/m-geo/public/brand/m-logo.png" width="104" alt="M-GEO Logo"></p>

# M-GEO

**本地优先的电商商品 GEO 工作台。**

帮助电商团队把商品资料整理成有依据、可审核的内容，并记录豆包、元宝、千问和百度的实际回答与引用情况。独立桌面应用，无需运行 CNVP、Docker 或服务器。

> 当前版本 **0.1.1，公开测试版**。Mac Apple 芯片已完成本地原生备份恢复验收；Windows x64 已构建并完成静态验包，实机验收待完成。安装包未完成正式签名／公证。软件不保证任何平台收录、排名、流量或销量。

## 下载

前往 [Releases](https://github.com/startopenapi-glitch/m-geo/releases) 下载 Mac ARM64 或 Windows x64 安装包。每个测试版附对应源码与 SHA-256 校验文件。Intel Mac 暂无经过实机验证的安装包。

## 核心工作流

商品资料 → 确认事实与依据 → 确认客户问题 → 本地／AI 草稿 → 人工审核 → 导出与发布登记 → AI 表现记录。

- **单机多品牌**：商品、问题、稿件、发布与观察按品牌隔离。
- **可信商品资料**：记录依据、有效期与公开范围；修改事实后重新确认。
- **内容工作室**：本地模板可离线使用；可自行配置 OpenAI 兼容文本接口生成候选草稿。
- **人工审核与导出**：审核后复制或导出 Markdown／HTML；不自动发布。
- **四平台观察**：手动记录豆包、元宝、千问、百度的回答与引用；区分消费者端和 API、基线与复测。
- **本地安全备份**：加密备份、整体恢复、恢复前安全快照；错误口令不替换当前资料。

## 资料与模型

业务数据保存在本机 SQLite，**数据库本身不宣称加密**。单机多品牌不等于多人权限系统。模型 Key 保存在操作系统凭据库；备份不包含模型配置、Key 或临时 AI 任务，恢复后需要重新配置模型。

保存配置不会调用模型。用户主动测试或生成时，所选内容会发送到自行配置的模型服务，可能产生费用。真实内容发布、平台登录与模型服务使用须由用户自行确认。

## 开发与验证

技术栈：Tauri 2、React 19、TypeScript、Vite、Rust、SQLite。锁文件固定实际依赖版本。

```sh
cd apps/m-geo
pnpm install --frozen-lockfile
pnpm test
pnpm typecheck
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --features desktop --all-targets -- -D warnings
pnpm build
pnpm tauri dev
```

Node、pnpm、Rust 和平台构建依赖见 [开发与使用说明](apps/m-geo/README.md)。Windows 原生构建脚本为 `apps/m-geo/scripts/build-windows.ps1`。GitHub Actions 提供手动构建矩阵；仓库上传不代表这些远程构建已执行。

浏览器验收使用隔离 SQLite 和真实 Rust 命令，不替代原生安装测试；使用方法见 [测试说明](docs/testing.md)。

## 文档

- [使用和构建](apps/m-geo/README.md)
- [0.1.1 发布说明与已知限制](docs/release-0.1.1.md)
- [Windows 实机验收](apps/m-geo/verification/windows-test-guide.md)
- [JSON 导入格式](docs/import-format.md)
- [模块接口契约](docs/m-geo-contract.md)
- [隐私说明](PRIVACY.md)
- [安全问题报告](SECURITY.md)
- [贡献指南](CONTRIBUTING.md)

## 版权与许可

软件代码沿用 **AGPL-3.0-only**。商品 GEO 业务逻辑基于 [GEOFlow](https://github.com/yaojingang/GEOFlow) 移植与修改，保留原作者 **Yao Jingang** 的版权与来源说明。完整许可见 [LICENSE](LICENSE)，第三方说明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

M-GEO 名称、蓝白机器人 Logo 与品牌标识不因代码开源而自动授予商标使用权。保留原品牌素材是为了让接收者可以构建与所获安装包对应的版本；衍生产品不得暗示获得原品牌认可。
