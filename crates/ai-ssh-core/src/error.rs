use thiserror::Error;

/// 核心 crate 统一错误类型。所有 IPC 命令最终转换为结构化 {code, message}。
#[derive(Error, Debug)]
pub enum Error {
    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("网络请求错误: {0}")]
    Http(String),

    #[error("LLM 错误: {0}")]
    Llm(String),

    #[error("加密错误: {0}")]
    Crypto(String),

    #[error("解析错误: {0}")]
    Parse(String),

    #[error("参数错误: {0}")]
    InvalidArgument(String),

    #[error("规则引擎错误: {0}")]
    Safety(String),

    #[error("监控错误: {0}")]
    Monitor(String),

    #[error("未找到: {0}")]
    NotFound(String),
}

impl Error {
    /// 结构化错误码，供前端统一展示（PRD 实现注意事项 #9）。
    pub fn code(&self) -> &'static str {
        match self {
            Error::Db(_) => "DB_ERROR",
            Error::Io(_) => "IO_ERROR",
            Error::Json(_) => "JSON_ERROR",
            Error::Http(_) => "HTTP_ERROR",
            Error::Llm(_) => "LLM_ERROR",
            Error::Crypto(_) => "CRYPTO_ERROR",
            Error::Parse(_) => "PARSE_ERROR",
            Error::InvalidArgument(_) => "INVALID_ARGUMENT",
            Error::Safety(_) => "SAFETY_ERROR",
            Error::Monitor(_) => "MONITOR_ERROR",
            Error::NotFound(_) => "NOT_FOUND",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
