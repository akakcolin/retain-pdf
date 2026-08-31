# 前端与桌面端贡献指南

## 目录边界

- `frontend/`：当前生产使用的静态前端源码，也是桌面端 bundle 的输入。
- `desktop/`：Electron 桌面端打包。
- `desktop/app/frontend/**`：桌面端实际读取的前端 bundle，不应该作为主要编辑入口。

## 本地启动

```bash
cd frontend
python3 -m http.server 40001 --bind 0.0.0.0
```

默认端口：

- Web 前端：`40001`
- Rust API：`41000`
- multipart 异步提交 API：`42000`

前端 API base 规则见 [本地启动与配置](../api/local-dev.md)。

## 桌面端同步

修改 `frontend/src/**`、`frontend/*.html`、`frontend/src/styles/**` 或其他会进入桌面端 bundle 的前端资源后，必须同步桌面端：

```bash
npm --prefix desktop run verify-frontend-sync
```

这个命令会重新构建静态前端、同步到桌面端 bundle，并跑桌面端前端 smoke。

## 改动规则

- 不要只改 `desktop/app/frontend/**`，应改 `frontend/**` 源文件后同步。
- UI 逻辑优先放到现有 feature/controller/view 模块，不要把新流程塞回一个大入口文件。
- 新增下载、reader、状态卡、术语表能力时，确认桌面端 bundle 也能通过 `npm --prefix desktop run verify-frontend-sync`。
- 前端需要新增 API 字段时，先确认后端是否有稳定 view/projection，不要让前端从内部 payload、raw artifact 或数据库字段里猜。

## 常用检查

```bash
npm --prefix frontend run build
npm --prefix desktop run verify-frontend-sync
```

前端端到端状态 smoke 会真实提交任务，通常需要本地 Rust API、OCR token、模型 key 和样本 PDF；具备这些条件时再跑：

```bash
cd frontend
npm run smoke:status -- --file ../data/temPDF/test1.pdf
```
