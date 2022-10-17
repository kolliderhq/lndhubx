use reqwest;

use actix_web::{
    get,
    HttpResponse,
};

use serde::{Deserialize, Serialize};
use xerror::api::*;

#[derive(Deserialize, Debug, Serialize)]
pub struct FtxSpotPrice {
	#[serde(alias="name")]
	symbol: String,
	price: Option<f64>,
	#[serde(alias="change24h")]
	change_24h: Option<f64>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct FtxSpotResponse {
	success: bool,
	result: Vec<FtxSpotPrice>
}

#[get("/get_spot_prices")]
pub async fn get_spot_prices(
) -> Result<HttpResponse, ApiError> {
	let available_pairs = vec!["BTC/USD", "BTC/EUR", "EUR/USD"];
	let res = reqwest::get("https://ftx.com/api/markets");
	let mut response = match res {
		Ok(r) => r,
		Err(_) => return Err(ApiError::External(ExternalError::FailedToFetchExternalData))
	};

	let body = match response.text() {
		Ok(b) => b,
		Err(_) => return Err(ApiError::External(ExternalError::FailedToFetchExternalData))
	};

	let spot_prices: FtxSpotResponse = match serde_json::from_str(&body) {
		Ok(sp) => sp,
		Err(_) => return Err(ApiError::External(ExternalError::FailedToFetchExternalData))
	};

	let filtered_spot = FtxSpotResponse {
		success: true,
		result: spot_prices.result.into_iter().filter(|s| available_pairs.iter().any(|ss| ss.to_string() == s.symbol)).collect::<Vec<FtxSpotPrice>>()
	};

    return Ok(HttpResponse::Ok().json(filtered_spot));
}