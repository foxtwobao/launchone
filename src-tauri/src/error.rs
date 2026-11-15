use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Login,
    Portal,
    Retry,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("配置无法处理，请检查文件格式；原文件未被覆盖")]
    Config(String),
    #[error("{0}")]
    Message(String),
    #[error("{message}")]
    Actionable {
        message: String,
        action: RecoveryAction,
    },
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
        if let Self::Actionable { message, action } = self {
            #[derive(Serialize)]
            struct Recovery<'a> {
                message: &'a str,
                action: RecoveryAction,
            }
            Recovery {
                message,
                action: *action,
            }
            .serialize(serializer)
        } else {
            serializer.serialize_str(&self.to_string())
        }
    }
}

impl AppError {
    pub fn recovery_action(&self) -> Option<RecoveryAction> {
        match self {
            Self::Actionable { action, .. } => Some(*action),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

pub fn message(text: impl Into<String>) -> AppError {
    AppError::Message(text.into())
}

pub fn actionable(text: impl Into<String>, action: RecoveryAction) -> AppError {
    AppError::Actionable {
        message: text.into(),
        action,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn actionable_errors_have_a_safe_message_and_action_while_legacy_errors_stay_strings() {
        let error = actionable("请重新登录 IDONE", RecoveryAction::Login);
        assert_eq!(error.to_string(), "请重新登录 IDONE");
        assert_eq!(error.recovery_action(), Some(RecoveryAction::Login));
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({"message":"请重新登录 IDONE", "action":"login"})
        );
        let plain = message("无法连接，请重试");
        assert_eq!(plain.recovery_action(), None);
        assert_eq!(
            serde_json::to_value(plain).unwrap(),
            json!("无法连接，请重试")
        );
    }
}
