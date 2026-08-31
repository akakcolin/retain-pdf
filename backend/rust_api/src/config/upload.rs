use super::env_vars::{env_u32, env_u64};

#[derive(Clone, Debug)]
pub struct UploadRuntimeConfig {
    pub upload_max_bytes: u64,
    pub upload_max_pages: u32,
    /// 页面数 × 对象数 复杂度上限,防止 PDF 炸弹拖死渲染;0 = 关闭。
    pub upload_max_complexity: u64,
}

impl UploadRuntimeConfig {
    pub fn from_env() -> Self {
        Self {
            upload_max_bytes: env_u64("RUST_API_UPLOAD_MAX_BYTES", 0),
            upload_max_pages: env_u32("RUST_API_UPLOAD_MAX_PAGES", 0),
            upload_max_complexity: env_u64("RUST_API_UPLOAD_MAX_COMPLEXITY", 0),
        }
    }

    pub fn unlimited() -> Self {
        Self {
            upload_max_bytes: 0,
            upload_max_pages: 0,
            upload_max_complexity: 0,
        }
    }
}
