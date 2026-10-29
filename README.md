<p align="center"><img src="apps/m-geo/public/brand/m-logo.png" width="104" alt="M-GEO Logo"></p>

# M-GEO

**本地优先的电商商品 GEO 桌面工作台。**

当前源码版本为 **0.21.0**。[0.21.0 预发布版](https://github.com/startopenapi-glitch/m-geo/releases/tag/v0.21.0)提供 Apple Silicon Mac 安装包；该安装包未使用 Apple Developer ID 签名，也未通过 Apple 公证。

M-GEO 帮助团队整理有来源的商品事实、制作并审核内容、导出本地交付物，并记录 AI 助手对商品的实际回答和引用。它不会自动发布商品内容，也不保证任何 AI 首页推荐、收录、排名、流量或销量。

## 工作流

商品资料 → 核验事实与来源 → 准备问题和实验 → 生成或编写草稿 → 人工审核 → 导出并自行发布 → 记录实际回答和引用。

- 单机管理多个隔离品牌、商品、问题、稿件、发布记录和观察。
- 为公开事实记录依据、可见范围和有效期；事实变更后需重新确认。
- 可离线写作，也可配置 OpenAI 兼容文本模型；每次模型测试或生成由用户主动触发，可能产生费用。
- 将审核后的内容导出为 Markdown、HTML 或本地站点/目录草稿；发布操作由用户在目标渠道完成。
- 记录豆包、元宝、千问和百度的人工观察，区分消费者页面与 API、测试基线与复测，并保留不确定和缺测状态。
- 提供本地归因、批量运营、证据包复核和逐商品推荐变化分析；模拟数据不会被当成真实业务成效。

## 下载与版本

- [0.21.0 预发布版与安装包](https://github.com/startopenapi-glitch/m-geo/releases/tag/v0.21.0)：提供 Apple Silicon Mac 安装包和 SHA-256 校验文件。
- [主分支源码 ZIP](https://github.com/startopenapi-glitch/m-geo/archive/refs/heads/main.zip)：需要按下文自行安装依赖并构建。该源码快照不含数据库、模型密钥或试点商品资料。
- [0.21.0 功能与已知限制](docs/release-0.21.0.md)。

## 开发与验证

技术栈：Tauri 2、React 19、TypeScript、Vite、Rust 和 SQLite。请使用仓库锁文件安装依赖：

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

**0.21.0** 新增 P0 接入演练：请求模式隔离、模拟预算、独立发布批准、模拟 CMS 回执／对账／回滚和静态页面检查，见 [0.21.0 本地候选说明](docs/release-0.21.0.md)。本版本已提供未公证的 macOS Apple Silicon 预发布安装包。

0.20.0 新增持久本地自动化与模拟证据包，范围和操作说明见 [0.20.0 本地候选说明](docs/release-0.20.0.md)。

完整构建前置条件、平台限制和数据说明见 [应用开发与使用说明](apps/m-geo/README.md)。本地自动化或虚构资料验收不代表远程 CI、平台账号、真实线上发布或消费者推荐效果已经通过。

## 数据、模型与隐私

- 业务资料保存在本机 SQLite；数据库本身不宣称加密。单机多品牌不等于多人账号或权限系统。
- API Key 使用操作系统凭据库保存，不进入源码归档或业务备份。模型配置保存时不发起请求；主动调用会把所选内容发送到用户配置的模型服务。
- 加密备份覆盖整个本地工作区，但不含模型设置或 API Key；恢复会替换工作区。请先阅读应用说明并保留可用备份。
- 软件不会因安装或启动而上传商品资料或公开发布稿件。

## 版权与许可

软件按 **AGPL-3.0-only** 提供。业务实现包含从 [GEOFlow](https://github.com/yaojingang/GEOFlow) 移植并修改的代码，保留原作者 Yao Jingang 的来源与版权说明。依赖许可见 [第三方说明](THIRD_PARTY_NOTICES.md)。

M-GEO 名称和标识不因源码许可而自动授予商标使用权。品牌素材的保留仅用于构建与本源码对应的应用，不代表原品牌对衍生项目背书。

更多信息：[隐私说明](PRIVACY.md) · [贡献指南](CONTRIBUTING.md) · [安全问题报告](SECURITY.md) · [许可证](LICENSE)
