use reqwest;

use actix_web::{get, HttpResponse, web};

use serde::{Deserialize, Serialize};
use rust_decimal::prelude::*;
use xerror::api::*;

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{PriceCache, FtxSpotPrice};

#[derive(Deserialize, Debug, Serialize)]
pub struct FtxSpotResponse {
    success: bool,
    result: Vec<FtxSpotPrice>,
}

#[get("/get_spot_prices")]
pub async fn get_spot_prices(price_cache: web::Data<Arc<RwLock<PriceCache>>>) -> Result<HttpResponse, ApiError> {
    let available_pairs = vec!["BTCUSDT", "BTCEUR", "EURUSDT"];
    let last_updated = &(**price_cache).read().await.last_updated.clone();

    let spot_prices = if last_updated.elapsed().as_secs() > 5 {
        let res = reqwest::get("https://api.binance.com/api/v3/ticker/24hr");
        let mut response = match res {
            Ok(r) => r,
            Err(_) => return Err(ApiError::External(ExternalError::FailedToFetchExternalData)),
        };

        let body = match response.text() {
            Ok(b) => b,
            Err(_) => return Err(ApiError::External(ExternalError::FailedToFetchExternalData)),
        };

        let spot_prices: Vec<FtxSpotPrice>= match serde_json::from_str(&body) {
            Ok(sp) => sp,
            Err(err) => {dbg!(&err); return Err(ApiError::External(ExternalError::FailedToFetchExternalData))},
        };

        let mut price_cache = price_cache.write().await;
        price_cache.last_updated = std::time::Instant::now();
        price_cache.spot_prices = spot_prices.clone();
        spot_prices
    } else {
        let spot_prices = &(**price_cache).read().await.spot_prices.clone();
        spot_prices.to_vec()
    };

    let mut filtered_spot = FtxSpotResponse {
        success: true,
        result: spot_prices
            .into_iter()
            .filter(|s| available_pairs.iter().any(|ss| *ss == s.symbol))
            .collect::<Vec<FtxSpotPrice>>(),
    };
    filtered_spot.result.iter_mut().for_each(|i| {
        i.symbol = i.symbol[..6].to_string();
    });

    Ok(HttpResponse::Ok().json(filtered_spot))
}
