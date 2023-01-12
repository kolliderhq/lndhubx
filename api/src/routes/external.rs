use reqwest;

use actix_web::{get, HttpResponse, web};

use serde::{Deserialize, Serialize};
use rust_decimal::prelude::*;
use xerror::api::*;

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::PriceCache;

#[derive(Deserialize, Debug, Serialize)]
pub struct FtxSpotPrice {
    symbol: String,
    #[serde(alias = "lastPrice")]
    price: Option<Decimal>,
    #[serde(alias = "priceChangePercent")]
    change_24h: Option<Decimal>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct FtxSpotResponse {
    success: bool,
    result: Vec<FtxSpotPrice>,
}

#[get("/get_spot_prices")]
pub async fn get_spot_prices(price_cache: web::Data<Arc<RwLock<PriceCache>>>) -> Result<HttpResponse, ApiError> {
    let available_pairs = vec!["BTCUSDT", "BTCEUR", "EURUSDT"];
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
