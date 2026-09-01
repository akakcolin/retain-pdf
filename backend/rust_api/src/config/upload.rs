use super::env_vars::{env_u32, env_u64};

/// 单文件默认上限 200 MiB。
///
/// 前端另有一道更严格的 50 MiB 门禁（`upload-constants.ts`）；这里放宽是
/// 为了不误伤 README 承诺的 API 自动化场景——脚本可以直传大文件。
pub const DEFAULT_UPLOAD_MAX_BYTES: u64 = 200 * 1024 * 1024;

/// 默认页数上限。前端门禁是 999 页，这里给到 2000 留出直传余量。
pub const DEFAULT_UPLOAD_MAX_PAGES: u32 = 2_000;

/// 默认复杂度上限（页数 × 对象数）。
///
/// PDF 是典型的恶意输入载体：一个几十 KB 的文件可以通过嵌套对象展开成
/// 数十 GB 的内存占用。单看字节数挡不住 PDF 炸弹，必须同时看
/// 页数 × 对象数。10M 对正常学术 PDF（数百页 × 每页数百对象）有充足余量。
pub const DEFAULT_UPLOAD_MAX_COMPLEXITY: u64 = 10_000_000;

#[derive(Clone, Debug)]
pub struct UploadRuntimeConfig {
    pub upload_max_bytes: u64,
    pub upload_max_pages: u32,
    /// 页面数 × 对象数 复杂度上限,防止 PDF 炸弹拖死渲染;0 = 关闭。
    pub upload_max_complexity: u64,
}

impl UploadRuntimeConfig {
    /// 从环境变量读取上传预算。
    ///
    /// 三个上限默认**全部启用**（见各 `DEFAULT_*` 常量）。早前默认值为 0，
    /// 而 0 在本配置中的语义是"关闭",等于整套防护从未生效过。
    ///
    /// 注意 `env_u64` / `env_u32` 会过滤掉非正值并回退到默认值——即
    /// `RUST_API_UPLOAD_MAX_BYTES=0` 不会关闭预算，而是被忽略。关闭预算
    /// 只能显式调用 [`Self::unlimited`]，这是刻意的 fail-safe 设计。
    pub fn from_env() -> Self {
        let config = Self {
            upload_max_bytes: env_u64("RUST_API_UPLOAD_MAX_BYTES", DEFAULT_UPLOAD_MAX_BYTES),
            upload_max_pages: env_u32("RUST_API_UPLOAD_MAX_PAGES", DEFAULT_UPLOAD_MAX_PAGES),
            upload_max_complexity: env_u64(
                "RUST_API_UPLOAD_MAX_COMPLEXITY",
                DEFAULT_UPLOAD_MAX_COMPLEXITY,
            ),
        };

        // 让运维在启动日志里直接看到防护是否开着，而不是靠读代码猜。
        if config.any_enabled() {
            tracing::info!(
                max_bytes = config.upload_max_bytes,
                max_pages = config.upload_max_pages,
                max_complexity = config.upload_max_complexity,
                "upload budget enabled"
            );
        } else {
            tracing::warn!("upload budget DISABLED: PDF bomb protection is off");
        }

        config
    }

    /// 显式关闭全部上传预算。
    ///
    /// 仅供测试夹具使用。生产路径（服务端与桌面端）一律走 [`Self::from_env`]。
    /// 用 `#[cfg(test)]` 把这条约定固化成编译期约束——生产代码想调用它，
    /// 编译都过不去。
    #[cfg(test)]
    pub fn unlimited() -> Self {
        Self {
            upload_max_bytes: 0,
            upload_max_pages: 0,
            upload_max_complexity: 0,
        }
    }

    /// 是否至少启用了一项预算。
    pub fn any_enabled(&self) -> bool {
        self.upload_max_bytes > 0 || self.upload_max_pages > 0 || self.upload_max_complexity > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_enabled() {
        assert!(UploadRuntimeConfig::from_env().any_enabled());
    }

    #[test]
    fn unlimited_disables_every_budget() {
        assert!(!UploadRuntimeConfig::unlimited().any_enabled());
    }

    #[test]
    fn defaults_have_expected_magnitude() {
        assert_eq!(DEFAULT_UPLOAD_MAX_BYTES, 209_715_200);
        assert_eq!(DEFAULT_UPLOAD_MAX_PAGES, 2_000);
        assert_eq!(DEFAULT_UPLOAD_MAX_COMPLEXITY, 10_000_000);
    }
}
