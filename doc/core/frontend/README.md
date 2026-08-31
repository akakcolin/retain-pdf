# 前端文档

这里存放前端联调、状态检查和优化记录，不是业务产品文档。

- [前端状态 Smoke](./status_smoke.md)
- [前端优化记录](./optimization_notes.md)
- [前端状态 Smoke 最新报告](../../ops/reports/frontend-status-smoke-latest.json)

主要代码入口：

- `frontend/src/js/`
- `frontend/src/styles/`
- `frontend/package.json`

桌面端同步：

- 修改 `frontend/src/**` 后，运行 `npm --prefix desktop run sync-frontend`，它会重新构建网页前端并同步到 `desktop/app/frontend`。
- 提交前运行 `npm --prefix desktop run verify-frontend-sync`，它会先同步桌面前端，再跑桌面前端 smoke，避免 Tauri 打包继续使用旧页面。
