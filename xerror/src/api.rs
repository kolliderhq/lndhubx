use err_derive::Error;
use serde::Serialize;

use actix_web::{error, http::StatusCode, HttpResponse};
use serde_json::json;

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occurred when authenticating.")]
pub enum AuthError {
    #[error(display = "User already exists.")]
    UserExists,
    #[error(display = "Incorrect password supplied.")]
    IncorrectPassword,
    #[error(display = "New registrations limit exceeded.")]
    RegistrationLimitExceeded,
    #[error(display = "New registrations disabled.")]
    RegistrationsDisabled,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occurred when authenticating.")]
pub enum JWTError {
    #[error(display = "No authorization header supplied.")]
    #[serde(rename = "JwtNotSupplied")]
    NotSupplied,
    #[error(display = "Jwt token that was supplied is invalid.")]
    #[serde(rename = "JwtInvalid")]
    Invalid,
    #[error(display = "Jwt token that was supplied is invalid.")]
    #[serde(rename = "JwtExpired")]
    Expired,
    #[error(display = "Jwt could not be generated.")]
    #[serde(rename = "JwtEncodingFailed")]
    EncodingFailed,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occurred when authenticating.")]
pub enum DbError {
    #[error(display = "Unable to get connection to Db.")]
    DbConnectionError,
    #[error(display = "User already exists.")]
    UserAlreadyExists,
    #[error(display = "User does not exist.")]
    UserDoesNotExist,
    #[error(display = "Couldn't fetch data.")]
    CouldNotFetchData,
    #[error(display = "An unknown error has occurred.")]
    Unknown,
    #[error(display = "Unable to update the database.")]
    UpdateFailed,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occurred using nostr engine.")]
pub enum NostrEngineError {
    #[error(display = "Unable to load profile.")]
    UnableToLoadProfile,
    #[error(display = "Unable to send private message.")]
    UnableToSendPrivateMessage,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occurred for admin request.")]
pub enum AdminError {
    #[error(display = "Only admin can perform the operation.")]
    NoPermission,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occurred when authenticating.")]
pub enum CommsError {
    #[error(display = "Unable to send message.")]
    FailedToSendMessage,
    #[error(display = "Timeout while waiting for a response.")]
    ServerResponseTimeout,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occurred when making this request.")]
pub enum RequestError {
    #[error(display = "Invalid data supplied")]
    InvalidDataSupplied,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has whilst fetching external data.")]
pub enum ExternalError {
    #[error(display = "Error fetching external data.")]
    FailedToFetchExternalData,
}

#[derive(Debug, Error, Serialize)]
#[serde(untagged)]
pub enum ApiError {
    #[error(display = "Auth error.")]
    Auth(AuthError),
    #[error(display = "Db error.")]
    Db(DbError),
    #[error(display = "Comms error.")]
    Comms(CommsError),
    #[error(display = "JWT error.")]
    JWT(JWTError),
    #[error(display = "Request error.")]
    Request(RequestError),
    #[error(display = "External error.")]
    External(ExternalError),
    #[error(display = "Nostr error.")]
    Nostr(NostrEngineError),
    #[error(display = "Admin error.")]
    Admin(AdminError),
}

impl error::ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        let mut response_builder = match self {
            ApiError::Auth(auth) => match auth {
                AuthError::UserExists => HttpResponse::Conflict(),
                AuthError::IncorrectPassword => HttpResponse::Unauthorized(),
                AuthError::RegistrationLimitExceeded => HttpResponse::Unauthorized(),
                AuthError::RegistrationsDisabled => HttpResponse::Unauthorized(),
            },
            ApiError::Db(db) => match db {
                DbError::DbConnectionError => HttpResponse::InternalServerError(),
                DbError::UserAlreadyExists => HttpResponse::Conflict(),
                DbError::UserDoesNotExist => HttpResponse::InternalServerError(),
                DbError::CouldNotFetchData => HttpResponse::InternalServerError(),
                DbError::Unknown => HttpResponse::InternalServerError(),
                DbError::UpdateFailed => HttpResponse::InternalServerError(),
            },
            ApiError::Comms(comms) => match comms {
                CommsError::FailedToSendMessage => HttpResponse::InternalServerError(),
                CommsError::ServerResponseTimeout => HttpResponse::InternalServerError(),
            },
            ApiError::JWT(_) => HttpResponse::Unauthorized(),
            ApiError::Request(request) => match request {
                RequestError::InvalidDataSupplied => HttpResponse::InternalServerError(),
            },
            ApiError::External(external) => match external {
                ExternalError::FailedToFetchExternalData => HttpResponse::InternalServerError(),
            },
            ApiError::Nostr(nostr_engine_error) => match nostr_engine_error {
                NostrEngineError::UnableToLoadProfile => HttpResponse::InternalServerError(),
                NostrEngineError::UnableToSendPrivateMessage => HttpResponse::InternalServerError(),
            },
            ApiError::Admin(AdminError::NoPermission) => HttpResponse::Unauthorized(),
        };
        response_builder.json(json!({ "error": self }))
    }

    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Auth(auth) => match auth {
                AuthError::UserExists => StatusCode::CONFLICT,
                AuthError::IncorrectPassword => StatusCode::UNAUTHORIZED,
                AuthError::RegistrationLimitExceeded => StatusCode::UNAUTHORIZED,
                AuthError::RegistrationsDisabled => StatusCode::UNAUTHORIZED,
            },
            ApiError::Db(db) => match db {
                DbError::DbConnectionError => StatusCode::INTERNAL_SERVER_ERROR,
                DbError::UserAlreadyExists => StatusCode::CONFLICT,
                DbError::UserDoesNotExist => StatusCode::INTERNAL_SERVER_ERROR,
                DbError::CouldNotFetchData => StatusCode::INTERNAL_SERVER_ERROR,
                DbError::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
                DbError::UpdateFailed => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::Comms(comms) => match comms {
                CommsError::FailedToSendMessage => StatusCode::INTERNAL_SERVER_ERROR,
                CommsError::ServerResponseTimeout => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::JWT(_) => StatusCode::UNAUTHORIZED,

            ApiError::Request(request) => match request {
                RequestError::InvalidDataSupplied => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::External(external) => match external {
                ExternalError::FailedToFetchExternalData => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::Nostr(nostr_engine_error) => match nostr_engine_error {
                NostrEngineError::UnableToLoadProfile => StatusCode::INTERNAL_SERVER_ERROR,
                NostrEngineError::UnableToSendPrivateMessage => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::Admin(AdminError::NoPermission) => StatusCode::UNAUTHORIZED,
        }
    }
}
