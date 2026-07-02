use std::sync::Arc;

use crate::{error::ApiResult, main_lib::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, put},
    Json, Router,
};
use rust_decimal::Decimal;
use wealthfolio_core::{
    assets::{Asset as CoreAsset, NewAsset, UpdateAssetProfile},
    fx::normalize_amount,
};

#[derive(serde::Deserialize)]
struct AssetQuery {
    #[serde(rename = "assetId")]
    asset_id: String,
}

async fn get_asset_profile(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AssetQuery>,
) -> ApiResult<Json<AssetProfileResponse>> {
    let asset = state.asset_service.get_asset_by_id(&q.asset_id)?;
    Ok(Json(AssetProfileResponse::new(
        asset,
        state.quote_service.get_latest_quote(&q.asset_id).ok(),
    )))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AssetProfileResponse {
    #[serde(flatten)]
    asset: CoreAsset,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_market_price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_market_currency: Option<String>,
}

impl AssetProfileResponse {
    fn new(asset: CoreAsset, quote: Option<wealthfolio_core::quotes::Quote>) -> Self {
        let (display_market_price, display_market_currency) = quote
            .map(|quote| {
                let (amount, currency) = normalize_amount(quote.close, &quote.currency);
                (Some(amount), Some(currency.to_string()))
            })
            .unwrap_or((None, None));

        Self {
            asset,
            display_market_price,
            display_market_currency,
        }
    }
}

async fn list_assets(State(state): State<Arc<AppState>>) -> ApiResult<Json<Vec<CoreAsset>>> {
    let assets = state.asset_service.get_assets()?;
    Ok(Json(assets))
}

async fn update_asset_profile(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateAssetProfile>,
) -> ApiResult<Json<CoreAsset>> {
    let asset = state
        .asset_service
        .update_asset_profile(&id, payload)
        .await?;

    Ok(Json(asset))
}

#[derive(serde::Deserialize)]
struct QuoteModeBody {
    #[serde(alias = "pricingMode", alias = "quoteMode")]
    quote_mode: String,
}

async fn update_quote_mode(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<QuoteModeBody>,
) -> ApiResult<Json<CoreAsset>> {
    let asset = state
        .asset_service
        .update_quote_mode(&id, &body.quote_mode)
        .await?;
    Ok(Json(asset))
}

async fn create_asset(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<NewAsset>,
) -> ApiResult<Json<CoreAsset>> {
    let asset = state.asset_service.create_asset(payload).await?;
    Ok(Json(asset))
}

async fn delete_asset(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state.asset_service.delete_asset(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/assets", get(list_assets).post(create_asset))
        .route("/assets/{id}", delete(delete_asset))
        .route("/assets/profile", get(get_asset_profile))
        .route("/assets/profile/{id}", put(update_asset_profile))
        .route("/assets/pricing-mode/{id}", put(update_quote_mode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use wealthfolio_core::{
        assets::{AssetKind, QuoteMode},
        quotes::Quote,
    };

    fn json_decimal(value: &serde_json::Value) -> Decimal {
        if let Some(number) = value.as_f64() {
            return Decimal::from_str(&number.to_string()).unwrap();
        }
        Decimal::from_str(
            value
                .as_str()
                .expect("decimal should serialize as number or string"),
        )
        .unwrap()
    }

    #[test]
    fn asset_profile_response_serializes_backend_normalized_market_quote() {
        let asset = CoreAsset {
            id: "asset-cty-lse".to_string(),
            kind: AssetKind::Investment,
            quote_mode: QuoteMode::Market,
            quote_ccy: "GBp".to_string(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            ..Default::default()
        };
        let quote = Quote {
            id: "quote-cty".to_string(),
            asset_id: asset.id.clone(),
            timestamp: Utc::now(),
            close: Decimal::new(565, 0),
            currency: "GBp".to_string(),
            created_at: Utc::now(),
            ..Default::default()
        };

        let response = AssetProfileResponse::new(asset, Some(quote));
        assert_eq!(response.display_market_price, Some(Decimal::new(565, 2)));
        assert_eq!(response.display_market_currency.as_deref(), Some("GBP"));

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["quoteCcy"], serde_json::json!("GBp"));
        assert_eq!(value["displayMarketCurrency"], serde_json::json!("GBP"));
        assert_eq!(
            json_decimal(&value["displayMarketPrice"]),
            Decimal::new(565, 2)
        );
    }
}
