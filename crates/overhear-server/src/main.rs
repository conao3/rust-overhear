//! overhear の GraphQL サーバ。
//!
//! Tauri の IPC ではなく 127.0.0.1 の HTTP / WebSocket に口を開ける。
//! 字幕は subscription が本質で、graphql-ws をそのまま話せるほうが
//! Apollo Client と素直に噛み合うため。

mod schema;
mod types;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_graphql::http::{ALL_WEBSOCKET_PROTOCOLS, GraphiQLSource};
use async_graphql::{Data, Schema};
use async_graphql_axum::{GraphQLProtocol, GraphQLRequest, GraphQLResponse, GraphQLWebSocket};
use axum::Router;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use clap::Parser;
use overhear_core::ring::encode_wav;
use overhear_core::translate::TranslatorRegistry;
use overhear_core::{EngineChoice, Overhear, RuntimeConfig};
use rand::Rng;

use schema::{MutationRoot, OverhearSchema, QueryRoot, SubscriptionRoot};

#[derive(Parser, Debug)]
#[command(
    name = "overhear-server",
    about = "システム音声の字幕化 GraphQL サーバ"
)]
struct Args {
    /// 待ち受けポート。0 なら OS に任せる。
    #[arg(long, default_value_t = 0)]
    port: u16,

    /// 認証トークンを要求しない (開発用)。
    #[arg(long, default_value_t = false)]
    no_auth: bool,

    /// GraphiQL を / に出す (開発用)。
    #[arg(long, default_value_t = false)]
    graphiql: bool,

    /// 擬似 ASR で動かす。april のモデルが無くても起動できる。
    #[arg(long, default_value_t = false)]
    mock: bool,

    /// リングバッファの長さ (秒)。
    #[arg(long, default_value_t = 600)]
    ring_seconds: usize,

    /// 翻訳先の言語。
    #[arg(long, default_value = "ja")]
    target_lang: String,

    /// 既定の翻訳エンジン。
    #[arg(long, default_value = "ollama")]
    translator: String,
}

#[derive(Clone)]
struct AppState {
    schema: OverhearSchema,
    overhear: Arc<Overhear>,
    token: Option<String>,
}

fn random_token() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect()
}

/// Authorization ヘッダのトークンを検証する。
fn header_token_ok(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = &state.token else {
        return true; // --no-auth
    };
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim_start_matches("Bearer ").trim() == expected)
        .unwrap_or(false)
}

async fn graphql_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> Response {
    if !header_token_ok(&state, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    let resp: GraphQLResponse = state.schema.execute(req.into_inner()).await.into();
    resp.into_response()
}

/// graphql-ws。Apollo の GraphQLWsLink がそのまま繋がる。
/// トークンは connectionParams で受け取る。
async fn graphql_ws_handler(
    State(state): State<AppState>,
    protocol: GraphQLProtocol,
    websocket: WebSocketUpgrade,
) -> Response {
    let schema = state.schema.clone();
    let expected = state.token.clone();
    websocket
        .protocols(ALL_WEBSOCKET_PROTOCOLS)
        .on_upgrade(move |socket| {
            GraphQLWebSocket::new(socket, schema, protocol)
                .on_connection_init(move |value| {
                    let expected = expected.clone();
                    async move {
                        let Some(expected) = expected else {
                            return Ok(Data::default());
                        };
                        let provided = value.get("token").and_then(|v| v.as_str());
                        if provided == Some(expected.as_str()) {
                            Ok(Data::default())
                        } else {
                            Err(async_graphql::Error::new("invalid token"))
                        }
                    }
                })
                .serve()
        })
}

/// segment の音声。GraphQL にバイナリは載せず、ここから取らせる。
async fn audio_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(file): Path<String>,
) -> Response {
    if !header_token_ok(&state, &headers) {
        // <audio src> はヘッダを付けられないため、クエリ経由も許す。
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    let Some(id) = file
        .strip_suffix(".wav")
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return (StatusCode::BAD_REQUEST, "bad segment id").into_response();
    };
    let Some(samples) = state.overhear.segment_audio(id) else {
        return (StatusCode::NOT_FOUND, "audio is no longer buffered").into_response();
    };
    let wav = encode_wav(&samples, state.overhear.config.sample_rate);
    ([(header::CONTENT_TYPE, "audio/wav")], wav).into_response()
}

async fn graphiql() -> impl IntoResponse {
    Html(
        GraphiQLSource::build()
            .endpoint("/graphql")
            .subscription_endpoint("/graphql")
            .finish(),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    // stdout は接続情報 (JSON 1 行) 専用にするため、ログは stderr へ出す。
    // Tauri はこの stdout の 1 行目を読んで WebView に流し込む。
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,overhear=debug".into()),
        )
        .init();

    let args = Args::parse();

    let mut registry = TranslatorRegistry::with_defaults();
    registry.set_default(&args.translator);

    let config = RuntimeConfig {
        ring_seconds: args.ring_seconds,
        target_lang: args.target_lang.clone(),
        engine: if args.mock {
            EngineChoice::Mock
        } else {
            EngineChoice::April
        },
        ..RuntimeConfig::default()
    };

    let overhear = Overhear::start(config, Arc::new(registry)).context("パイプラインの起動")?;

    let schema = Schema::build(QueryRoot, MutationRoot, SubscriptionRoot)
        .data(Arc::clone(&overhear))
        .finish();

    let token = if args.no_auth {
        None
    } else {
        Some(random_token())
    };

    let state = AppState {
        schema,
        overhear,
        token: token.clone(),
    };

    let mut app = Router::new()
        .route("/graphql", axum::routing::post(graphql_handler))
        .route("/graphql", get(graphql_ws_handler))
        .route("/audio/{file}", get(audio_handler));
    if args.graphiql {
        app = app.route("/", get(graphiql));
    }
    let app = app
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    // 127.0.0.1 のみにバインドする。ポートは既定で OS 任せ。
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("{addr} を bind できない"))?;
    let local = listener.local_addr()?;

    // Tauri / フロントが読む接続情報。
    let announce = serde_json::json!({
        "endpoint": format!("http://{local}"),
        "graphql": format!("http://{local}/graphql"),
        "websocket": format!("ws://{local}/graphql"),
        "token": token,
    });
    println!("{announce}");

    tracing::info!(%local, "overhear-server を起動した");
    axum::serve(listener, app).await?;
    Ok(())
}
