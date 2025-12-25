use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::NaiveDateTime;
use license_common::{License, Message};
use rand::rngs::ThreadRng;
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::{RsaPrivateKey, RsaPublicKey};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::log;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn generate_private_key(rng: &mut ThreadRng) -> RsaPrivateKey {
    RsaPrivateKey::new(rng, 2048).expect("failed to generate a key")
}

async fn handler_public_certificate(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pem = state
        .public_key
        .to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
        .unwrap();

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/x-pem-file".parse().unwrap());
    (headers, pem)
}

async fn handler_post_license(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<String, (StatusCode, String)> {
    let server_id = params.get("id").ok_or((
        StatusCode::BAD_REQUEST,
        "Could not find the query parameter 'id' of the server".to_string(),
    ))?;

    let license_id = "134".to_string();
    let license = License::new(
        license_id.clone(),
        server_id.clone(),
        NaiveDateTime::MIN,
        NaiveDateTime::MAX,
    );

    {
        let mut licenses = state.licenses.lock().await;
        licenses.insert(license_id, license.clone());
    }

    let mut rng = rand::thread_rng();
    let message = Message::new(license, &state.public_key, &mut rng);
    let message_raw = message.encrypt().unwrap();

    Ok(hex::encode(&message_raw))
}

async fn handler_get_license(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<License>, (StatusCode, String)> {
    let license_id = params.get("id").ok_or((
        StatusCode::BAD_REQUEST,
        "Could not find the query parameter 'id' of the license".to_string(),
    ))?;

    let licenses = state.licenses.lock().await;

    let license = licenses.get(license_id).ok_or((
        StatusCode::NOT_FOUND,
        format!("Could not find the license by id '{}'", license_id),
    ))?;

    Ok(Json(license.clone()))
}

async fn handler_delete_license(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, (StatusCode, String)> {
    let license_id = params.get("id").ok_or((
        StatusCode::BAD_REQUEST,
        "Could not find the query parameter 'id' of the license".to_string(),
    ))?;

    let mut licenses = state.licenses.lock().await;
    if !licenses.remove(license_id).is_some() {
        log::warn!("Could not find a license with the id '{}'", license_id);
    }

    Ok(StatusCode::ACCEPTED)
}

struct AppState {
    _private_key: RsaPrivateKey,
    public_key: RsaPublicKey,
    licenses: Mutex<HashMap<String, License>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::builder().parse_lossy("debug"))
        .init();

    let mut rng = rand::thread_rng();
    // TODO store the private key in the fs and load if possible
    let priv_key = generate_private_key(&mut rng);

    let router = Router::new()
        .route("/cert.pem", get(handler_public_certificate))
        .route("/license", post(handler_post_license))
        .route("/license", get(handler_get_license))
        .route("/license", delete(handler_delete_license))
        .with_state(Arc::new(AppState {
            public_key: priv_key.to_public_key(),
            _private_key: priv_key,
            licenses: Mutex::new(HashMap::<String, License>::new()),
        }));

    log::info!("Starting server...");
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}
