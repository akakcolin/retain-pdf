# 0007 渲染层拆分为五个 Rust crate

## 背景

渲染层曾以单个技术栈自然增长，mupdf 耦合会拖入整个渲染链路。为让「可被读写两侧共享」的部分不背负重依赖，渲染层按依赖隔离拆成五个 crate：`rendering_core` / `rendering_reader` / `rendering_writer` / `rendering_output` / `rendering_orchestrator`。

关键约束：`rendering_core` **故意零 mupdf**（传递依赖仅 20 包，对比 `desktop/src-tauri` 的 495 包），因此可被读、写两侧无痛共享。拆分 commit：`15991635`(core) / `0d57d9db`(reader) / `f1095aca`(writer) / `10988cca`(output) / `092bc885`(orchestrator)。

## 决策

五个 crate 的职责边界：

- `rendering_core`：纯领域逻辑（排版模型、text_flow、TOC），**永不直接依赖 mupdf**。
- `rendering_reader`：mupdf-rs 读取原语（解析不可信输入的第一道关）。
- `rendering_writer`：redaction / 保存，走 `mupdf=0.8` + `mupdf-sys`。
- `rendering_output`：typst emitter / 编译 + overlay。
- `rendering_orchestrator`：`render_rs` 二进制装配（`src/main.rs`）。

「core 永不直接依赖 mupdf」固化为硬约束，由 CI（`cargo` deny / architecture check）守护，防止未来「图省事」把 mupdf 塞进 core。

## 后果

- 五个 crate 的 API 边界需要维护，改接口会跨 crate 传播。
- core 保持可移植、可独立发布，是第十律「最小可移植核心」的载体。
- 引入新依赖时须按 crate 边界决定归属，不能随手加到 core。

## 替代方案

- 单 crate：实现简单，但 mupdf 依赖会被所有消费者（包括纯逻辑路径）拖入。
- 合并 core+reader/writer：隔离减弱，读写两侧共享时仍会牵动 mupdf。
- 不加约束自然增长：依赖边界随时间漂移，无法机器验证。
