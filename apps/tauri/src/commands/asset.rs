use std::sync::Arc;

use crate::context::ServiceContext;
use rust_decimal::Decimal;
use tauri::State;
use wealthfolio_core::{
    assets::{Asset, NewAsset, UpdateAssetProfile},
    fx::normalize_amount,
};

#[tauri::command]
pub async fn get_asset_profile(
    asset_id: String,
    state: State<'_, Arc<ServiceContext>>,
) -> Result<AssetProfileResponse, String> {
    let asset = state
        .asset_service()
        .get_asset_by_id(&asset_id)
        .map_err(|e| e.to_string())?;

    let quote = state.quote_service().get_latest_quote(&asset_id).ok();
    Ok(AssetProfileResponse::new(asset, quote))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetProfileResponse {
    #[serde(flatten)]
    asset: Asset,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_market_price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_market_currency: Option<String>,
}

impl AssetProfileResponse {
    fn new(asset: Asset, quote: Option<wealthfolio_core::quotes::Quote>) -> Self {
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

#[tauri::command]
pub async fn get_assets(state: State<'_, Arc<ServiceContext>>) -> Result<Vec<Asset>, String> {
    state
        .asset_service()
        .get_assets()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_asset_profile(
    id: String,
    payload: UpdateAssetProfile,
    state: State<'_, Arc<ServiceContext>>,
) -> Result<Asset, String> {
    state
        .asset_service()
        .update_asset_profile(&id, payload)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_quote_mode(
    id: String,
    quote_mode: String,
    state: State<'_, Arc<ServiceContext>>,
) -> Result<Asset, String> {
    state
        .asset_service()
        .update_quote_mode(&id, &quote_mode)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_asset(
    payload: NewAsset,
    state: State<'_, Arc<ServiceContext>>,
) -> Result<Asset, String> {
    state
        .asset_service()
        .create_asset(payload)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_asset(id: String, state: State<'_, Arc<ServiceContext>>) -> Result<(), String> {
    // Domain events handle quote sync state cleanup automatically
    state
        .asset_service()
        .delete_asset(&id)
        .await
        .map_err(|e| e.to_string())
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
        let asset = Asset {
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
