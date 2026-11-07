use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("配置无法处理，请检查文件格式；原文件未被覆盖")]
    Config(String),
    #[error("{0}")]
    Message(String),
    #[error("无法读写本地文件，请检查目录权限或重试")]
    Io(#[from] std::io::Error),
    #[error("无法解析服务响应或本地记录")]
    Json(#[from] serde_json::Error),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

pub fn message(text: impl Into<String>) -> AppError {
    AppError::Message(text.into())
}
