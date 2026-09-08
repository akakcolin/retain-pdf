// 启动进度浮层:初始可见,进度文案/进度条/隐藏均由命令式 view 层驱动
// (src/pages/reader/legacy/view.js + progress-presenter.js),React 只渲染一次容器。

export function ReaderBootLoading() {
  return (
    <div id="reader-boot-loading" className="reader-boot-loading" aria-live="polite">
      <div className="reader-boot-loading-card">
        <div id="reader-boot-loading-text" className="reader-boot-loading-text">正在准备对照阅读…</div>
        <div className="reader-boot-loading-track">
          <span id="reader-boot-loading-bar" className="reader-boot-loading-bar"></span>
        </div>
      </div>
    </div>
  );
}
