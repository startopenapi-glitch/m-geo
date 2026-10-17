# 验证范围

日常检查在 `apps/m-geo` 执行 README 中的测试、类型检查、Clippy 和生产构建命令。

## Chromium + Rust 集成测试

单独安装 Playwright 及 Chromium，避免修改应用锁文件：

```sh
npm install --prefix /tmp/mgeo-browser-tools playwright
/tmp/mgeo-browser-tools/node_modules/.bin/playwright install chromium
cd apps/m-geo
cargo build --locked --manifest-path src-tauri/Cargo.toml --example core_bridge
pnpm dev
# 在另一终端，仍在 apps/m-geo：
MGEO_PLAYWRIGHT=/tmp/mgeo-browser-tools/node_modules/playwright node verification/browser-check.mjs
```

测试创建并清理隔离 SQLite，仅使用虚构商品和 example.com 发布记录；不访问发布平台或调用模型。`MGEO_TEST_URL` 可覆盖本地开发地址，`MGEO_CHROMIUM` 可指定测试浏览器。浏览器测试不覆盖 Tauri IPC、系统凭据库、原生文件对话框或安装器。

## Windows 静态验包

```sh
python3 scripts/test-windows-package.py
```

原生 PowerShell 构建与验包需 Python、7-Zip、Windows SDK、MSVC 等依赖；详见构建脚本。静态验包成功不等于 Windows 安装／运行通过，必须另跑实机清单。
