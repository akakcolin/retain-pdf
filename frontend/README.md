# RetainPDF 前端（主站）

**生产入口：本目录 `frontend/`**（三页静态 SPA：`index` / `detail` / `reader`）。

| 目录 | 说明 |
|------|------|
| `src/pages/home` | 主页（书架、上传、任务） |
| `src/pages/reader` | 阅读器（默认 react-pdf；见该目录 README） |
| `src/pages/detail` | 任务详情 |
| `src/js` | 共享 API / 命令式领域 / 旧阅读引擎 / mock |
| `src/styles` | 全局与按页 CSS |

**文件夹逻辑与双 features 树**：见 [`src/FEATURES.md`](src/FEATURES.md)  
（`js/features` = 领域；`pages/home/features` = React UI；阅读器另见 `pages/reader` + `js/reader`。）

## 常用命令

```bash
npm run build        # css + js + stamp
npm run build:js
npm run build:css
npm test
python3 scripts/serve_static.py --host 127.0.0.1 --port 40001 --root .
```

| 文档 | 内容 |
|------|------|
| `src/FEATURES.md` | 全站目录 / 双 features / 阅读器三层 / detail external |
| `src/pages/reader/README.md` | 阅读器 react-pdf vs shared ports vs legacy |
| `src/js/reader/README.md` | 旧 pdf.js 引擎边界 |
| `src/pages/detail/README.md` | 详情页 external 规则 |
| `src/pages/home/composition/README.md` | 主页装配规则 |
| `src/pages/home/features/README.md` | 主页 React 域索引 |
| `src/styles/README.md` | CSS 按页拆包（home/detail/reader） |
