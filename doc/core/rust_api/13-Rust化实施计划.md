# Rust 化实施计划

把 Python 版面分析/布局管线（`backend/scripts/services/rendering`）迁移到 Rust，并用 Tauri v2 替换 Electron 外壳，降低桌面端体积与资源占用。

## 背景依据

- `backend/scripts/devtools/tests/` 共 977 个测试函数 + golden fixtures，业务逻辑已被验证。
- 覆盖盘点结论：第一批可搬运模块约 44 个测试函数，全部无 `fitz` 依赖，断言为数值阈值或枚举。
- 桌面端体积大头：Electron(~150–250MB) / Python 运行时(~150MB+) / CJK 字体(47MB) / typst(20–30MB)。

## 阶段

### Phase 0：Tauri v2 替换 Electron 外壳（先做，风险最低）
- 保留现有后端（rust_api + Python 管线 + typst），复用 `desktop/src` 的前端与后端启动逻辑。
- 收益：去掉 Electron/Chromium 体积，不动 Python/Rust 后端。
- 验收：macOS / Windows / Linux 桌面端可启动、可提交翻译任务。

### Phase 1：第一批纯逻辑模块移植（无风险）
按依赖 DAG 自底向上：

1. `layout/typography/`（10 个文件，纯数学）
2. `layout/font_roles` `font_size_fit` `leading_fit` `title_fit_limits`
3. `layout/font_fit.py`（汇总以上）
4. `layout/chinese_body_fit.py`
5. `layout/fit_decision/`（`curves` `models` `planner`）
6. `layout/payload/`（`capacity` + `shared` `text_common` `formula_cost` `continuation_split`）
7. `analysis/route/`（`layout_route` `background_route` `compose_route` `redaction_route` `builder` `reason`）

- 覆盖：`test_body_font_estimation`(6) `test_body_font_inheritance`(3) `test_body_line_count_and_single_line`(3) `test_dense_body_fit_policy`(6) `test_wide_aspect_body_leading`(4) `test_chinese_body_fit`(6) `test_fit_decision`(6) `test_payload_capacity`(4) `test_page_route`(6)。
- 验收：上述测试翻译为 Rust `#[test]` 全部通过。

### Phase 2：差分测试（等价性 fuzz）
- Python 与 Rust 各实现一套 reference 输入生成器，随机输入比对输出（数值容差 + 枚举精确）。
- 覆盖 Phase 1 全部模块，作为后续移植的回归防线。

### Phase 3：数据形状层 `analysis/profile/*`
- fitz 符号仅 `fitz.Rect` / `fitz.Page`（数据型），先定义 `Rect{x0,y0,x1,y1: f32}` + Page 抽象。
- 覆盖：`test_page_profile`(2) + `test_page_classifier`(2)。

### Phase 4：golden_replay 规格 → Rust 集成测试
- 把 `golden_replay/manifest.json` + `run_golden_flow` 迁移为 Rust 集成测试。
- 前置：Phase 3 的 PDF 读取抽象接入真实 reader（mupdf-rs）。

### Phase 5：PDF-IO / 渲染边界（风险最高，依赖真实样本）
- `layout/page_specs`（`fitz.open`）→ mupdf-rs。
- `payload/first_line_indent` + `prepare`（pixmap 渲染）。
- `analysis/document/builder`、`source_cleanup/pdf/*`、document_schema 适配器。

## 风险与注意

- golden 样本多样性不足（仅 3 个 sample PDF）。
- 翻译 replay 需真实 API Key。
- Python↔Rust 浮点差异需设容差。
