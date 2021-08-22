use actix_web::{http::StatusCode, HttpResponseBuilder, ResponseError};
use serde_json::json;
use thiserror::Error;
use log::error;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("{}", .0.msg())]
    DB(#[from] sqlx::Error),
    #[error("{}", .0)]
    Str(&'static str),
    #[error("{}", .0)]
    String(String),
}

impl ResponseError for ServerError {
    fn error_response(&self) -> actix_web::HttpResponse {
        error!("{}", self.to_string());
        HttpResponseBuilder::new(self.status_code()).json(json!({"err": self.to_string()}))
    }
    fn status_code(&self) -> actix_web::http::StatusCode {
        match *self {
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub trait ErrMsg {
    fn msg(&self) -> String;
}

impl ErrMsg for sqlx::Error {
    fn msg(&self) -> String {
        match self {
            sqlx::Error::Database(i) => i.message().to_string(),
            sqlx::Error::RowNotFound => "数据不存在".to_string(),
            _ => self.to_string(),
        }
    }
}
