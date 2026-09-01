# 0008 渲染从 pyo3 bridge 转向 native 子进程

## 背景

渲染链路早期由 Python 经 pyo3 bridge 进程内调用 Rust（`rendering_bridge`）。该形态把渲染复杂度拖入 Python 进程：桥接口成为双端绑定，调试要跨 FFI 边界，发布包体积被 pyo3 运行时代价抬升（C4·R 发布包剪除渲染依赖后为 189.9 MiB）。

git 历史完成了三步收口：

- `a25c8536`：C4·R 发布包去 pyo3 bridge 化，剪除渲染依赖。
- `9847e04d`：删除 Python 渲染树与 `rendering_bridge`，render 全量 native。
- `552c1318`：fitz 收编 45 项 native-only，delegate 退役。

## 决策

- **生产路径全量 native**：`render_rs`（`rendering_orchestrator` 装配）以 `--spec` 子进程方式被调用，通过文件/stdout 契约交换规格与结果。
- **bridge 仅作 dev parity reference**：不再进入生产路径，只用于开发期对照 native 输出。
- 渲染调用方（Python/服务端/Docker）只 spawn `render_rs`，不再进程内 FFI。

## 后果

- 进程边界替代 FFI：接口契约显式化（stdin/文件 + stdout），错误通过退出码与 stderr 传递。
- 调试从「双进程共享内存」变为「单进程日志 + 子进程 stderr」，事件流可诊断（`render_rs` 事件流与诊断平价已补齐）。
- Docker 内渲染全量 native（`9a4fd7d2` 出包 `render_rs`），镜像不再承载 pyo3 运行时。
- parity 由 golden / differential 测试守护，防 native 与参考实现漂移。

## 替代方案

- 保留 pyo3 bridge 双轨：性能上进程内调用更省一次 spawn，但发布包体积、跨 FFI 调试、双端绑定维护成本持续存在。
- 由 Python 直接渲染：回归依赖不可控的旧渲染树，与「最小可移植核心」方向冲突。
