use err_derive::Error;
use serde::Serialize;


use actix_web::{
    error,
    http::{StatusCode},
    HttpResponse,
};



#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occured when authenticating.")]
pub enum AuthError {
    #[error(display = "User already exists.")]
    UserExists,
    #[error(display = "Incorrect password supplied.")]
    IncorrectPassword,
    #[error(display = "Invalid LnAuth.")]
    InvalidLNAuth,
    #[error(display = "Internal Error.")]
    InternalError,
    #[error(display = "User type not found.")]
    UserTypeNotFound,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occured when authenticating.")]
pub enum JWTError {
    #[error(display = "No authorization header supplied.")]
    NotSupplied,
    #[error(display = "Jwt token that was supplied is invalid.")]
    Invalid,
    #[error(display = "Jwt token that was supplied is invalid.")]
    Expired
}


#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occured when authenticating.")]
pub enum DbError {
    #[error(display = "Unable to get connection to Db.")]
    DbConnectionError,
    #[error(display = "User already exists.")]
    UserAlreadyExists,
    #[error(display = "User does not exist.")]
    UserDoesNotExist,
    #[error(display = "Couldn't fetch data.")]
    CouldNotFetchData,
    #[error(display = "An unknown error has occured.")]
    Unknown,
}

#[derive(Debug, Error, Serialize)]
#[error(display = "An Error has occured when authenticating.")]
pub enum CommsError {
    #[error(display = "Unabel to send message.")]
    FailedToSendMessage,
}

#[derive(Debug, Error, Serialize)]
pub enum ApiError {
    #[error(display = "Auth error.")]
    Auth(AuthError),
    #[error(display = "Db error.")]
    Db(DbError),
    #[error(display = "Comms error.")]
    Comms(CommsError),
    #[error(display = "Comms error.")]
    JWT(JWTError),
}

impl error::ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        match self {
            ApiError::Auth(auth) => match auth {
                AuthError::UserExists => HttpResponse::Conflict().json("There was a conflict with your request."),
                AuthError::IncorrectPassword => {
                    HttpResponse::Unauthorized().json("You have supplied the wrong password.")
                },
                AuthError::InternalError => {
                    HttpResponse::BadRequest().json("Internal server error.")
                },
                AuthError::InvalidLNAuth => {
                    HttpResponse::Unauthorized().json("Invalid LNURL auth.")
                },
                AuthError::UserTypeNotFound => {
                    HttpResponse::Conflict().json("User type not found.")
                }
            },
            ApiError::Db(db) => match db {
                DbError::DbConnectionError => HttpResponse::InternalServerError().json("Couldn't connect to Db."),
                DbError::UserAlreadyExists => HttpResponse::Conflict().json("User already exists."),
                DbError::UserDoesNotExist => HttpResponse::InternalServerError().json("User does not exist."),
                DbError::CouldNotFetchData => HttpResponse::InternalServerError().json("Could not fetch data."),
                DbError::Unknown => HttpResponse::InternalServerError().json("An unknown error has occured."),
            },
            ApiError::Comms(comms) => match comms {
                CommsError::FailedToSendMessage => HttpResponse::InternalServerError().json("Could't send message."),
            },
            ApiError::JWT(jwt) => match jwt {
                JWTError::Invalid => HttpResponse::Unauthorized().json("Jwt token is invalid."),
                JWTError::Expired => HttpResponse::Unauthorized().json("Jwt token is expired."),
                JWTError::NotSupplied => HttpResponse::Unauthorized().json("Jwt token is not supplied."),
            },
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Auth(auth) => match auth {
                AuthError::UserExists => StatusCode::CONFLICT,
                AuthError::IncorrectPassword => StatusCode::UNAUTHORIZED,
                AuthError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
                AuthError::InvalidLNAuth => StatusCode::UNAUTHORIZED,
            },
            ApiError::Db(db) => match db {
                DbError::DbConnectionError => StatusCode::INTERNAL_SERVER_ERROR,
                DbError::UserAlreadyExists => StatusCode::CONFLICT,
                DbError::UserDoesNotExist => StatusCode::INTERNAL_SERVER_ERROR,
                DbError::CouldNotFetchData => StatusCode::INTERNAL_SERVER_ERROR,
                DbError::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::Comms(comms) => match comms {
                CommsError::FailedToSendMessage => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::JWT(jwt) => match jwt {
                JWTError::Invalid => StatusCode::UNAUTHORIZED,
                JWTError::Expired => StatusCode::UNAUTHORIZED,
                JWTError::NotSupplied => StatusCode::UNAUTHORIZED,
            },
        }
    }
}
