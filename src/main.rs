use axum::{
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use once_cell::sync::Lazy;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

const API_URL: &str = "https://plausible.canine.tools/api/stats/artistgrid.cx/custom-prop-values/name/?period=all&date=2025-11-07&filters=%5B%5B%22is%22%2C%22event%3Agoal%22%2C%5B%22Artist%20Click%22%5D%5D%5D&with_imported=true&detailed=true&order_by=%5B%5B%22visitors%22%2C%22desc%22%5D%5D&limit=100&page=1";
const CACHE_DURATION: Duration = Duration::from_secs(600); // 10 minutes

#[derive(Clone)]
struct CacheEntry {
    data: String,
    timestamp: Instant,
}

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    cache: Arc<RwLock<Option<CacheEntry>>>,
    bearer_token: String,
}

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
});

#[tokio::main]
async fn main() {
    // Load .env file
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Get bearer token from environment
    let bearer_token = std::env::var("BEARER_TOKEN")
        .expect("BEARER_TOKEN must be set in environment or .env file");

    let state = AppState {
        client: HTTP_CLIENT.clone(),
        cache: Arc::new(RwLock::new(None)),
        bearer_token,
    };

    let app = Router::new()
        .route("/", get(handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    
    tracing::info!("Server running on http://0.0.0.0:3000");
    
    axum::serve(listener, app).await.unwrap();
}

async fn handler(State(state): State<AppState>) -> Response {
    // Check cache first
    {
        let cache = state.cache.read().await;
        if let Some(entry) = cache.as_ref() {
            if entry.timestamp.elapsed() < CACHE_DURATION {
                tracing::info!("Returning cached response");
                return entry.data.clone().into_response();
            }
        }
    }

    // Cache miss or expired, fetch new data
    tracing::info!("Fetching fresh data from API");
    
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", state.bearer_token)
            .parse()
            .expect("Invalid bearer token"),
    );

    match state.client.get(API_URL).headers(headers).send().await {
        Ok(response) => {
            match response.text().await {
                Ok(body) => {
                    // Update cache
                    let entry = CacheEntry {
                        data: body.clone(),
                        timestamp: Instant::now(),
                    };
                    
                    let mut cache = state.cache.write().await;
                    *cache = Some(entry);
                    
                    body.into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to read response body: {}", e);
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error reading response: {}", e),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to fetch data: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error fetching data: {}", e),
            )
                .into_response()
        }
    }
}