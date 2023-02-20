use crate::jwt::*;
use crate::CreationLimiter;
use actix_web::web::Data;
use actix_web::{get, HttpResponse};
use core_types::UserId;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use xerror::api::*;

#[get("/admin/disable_create")]
pub async fn disable_create(
    creation_limiter: Data<Arc<Mutex<CreationLimiter>>>,
    admin_uids: Data<HashSet<UserId>>,
    auth_data: AuthData,
) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;
    if !admin_uids.contains(&uid) {
        return Err(ApiError::Admin(AdminError::NoPermission));
    }
    creation_limiter.into_inner().lock().await.disable_creation();
    Ok(HttpResponse::Ok().json(json!({"success": true})))
}

#[get("/admin/enable_create")]
pub async fn enable_create(
    creation_limiter: Data<Arc<Mutex<CreationLimiter>>>,
    admin_uids: Data<HashSet<UserId>>,
    auth_data: AuthData,
) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;
    if !admin_uids.contains(&uid) {
        return Err(ApiError::Admin(AdminError::NoPermission));
    }
    creation_limiter.into_inner().lock().await.enable_creation();
    Ok(HttpResponse::Ok().json(json!({"success": true})))
}
