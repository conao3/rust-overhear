//! overhear の GraphQL サーバ。
//!
//! Tauri の IPC ではなく 127.0.0.1 の HTTP / WebSocket に口を開ける。
//! 字幕は subscription が本質で、graphql-ws をそのまま話せるほうが
//! Apollo Client と素直に噛み合うため。

mod cli;
mod schema;
mod types;

use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context as _;
use async_graphql::http::{ALL_WEBSOCKET_PROTOCOLS, GraphiQLSource};
use async_graphql::{Data, Schema};
use async_graphql_axum::{GraphQLProtocol, GraphQLRequest, GraphQLResponse, GraphQLWebSocket};
use axum::Router;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use overhear_cli::{CliError, GlobalArgs};
use overhear_core::anki::AnkiConnect;
use overhear_core::dict::DictionaryRegistry;
use overhear_core::pipeline::Services;
use overhear_core::ring::encode_wav;
use overhear_core::secrets::{ApiKeys, KeyringBackend};
use overhear_core::settings::SettingsStore;
use overhear_core::translate::TranslatorRegistry;
use overhear_core::vocab::VocabStore;
use overhear_core::{EngineChoice, Overhear, RuntimeConfig};
use rand::RngExt;

use cli::{Cli, Command, ServerCommand, StartServerArgs, StartServerOutput};
use schema::{MutationRoot, OverhearSchema, QueryRoot, SubscriptionRoot};
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
    // 認証は /graphql と同じくヘッダのみ。<audio src> は直接ここを叩けないため、
    // フロントは fetch でトークンを付けて取得し blob にして再生する。
    // URL にトークンを載せない方針。
    if !header_token_ok(&state, &headers) {
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

fn other(err: anyhow::Error) -> CliError {
    CliError::Other(format!("{err:#}"))
}

async fn start_server(global: &GlobalArgs, args: StartServerArgs) -> Result<(), CliError> {
    let data_dir = overhear_core::data_dir().map_err(other)?;
    let settings = Arc::new(
        SettingsStore::open(&data_dir)
            .context("設定を読めない")
            .map_err(other)?,
    );

    // Secret Service が無くても字幕は使えるよう、読めなければキー無しで続ける。
    // キーの保存はその時点でエラーになる。
    let api_keys = match ApiKeys::load(Box::new(KeyringBackend), &["deepl", "google"]) {
        Ok(keys) => keys,
        Err(err) => {
            tracing::warn!(%err, "API キーを読めないので、キー無しで続ける");
            ApiKeys::load(Box::new(KeyringBackend), &[]).map_err(other)?
        }
    };
    let api_keys = Arc::new(api_keys);

    let registry = TranslatorRegistry::with_defaults(Arc::clone(&api_keys))
        .context("翻訳エンジンの設定")
        .map_err(other)?;
    match &args.translator {
        Some(id) => registry
            .set_default(id)
            .map_err(|err| CliError::Target(format!("--translator: {err}")))?,
        None => {
            if let Some(id) = settings.get().translator {
                registry.set_default(&id).map_err(other)?;
            }
        }
    }
    if let Some(model) = settings.get().ollama_model {
        registry.ollama().set_model(&model).map_err(other)?;
    }

    let config = RuntimeConfig {
        silence_threshold: args
            .silence_threshold
            .unwrap_or(overhear_core::gate::DEFAULT_THRESHOLD),
        ring_seconds: args.ring_seconds.unwrap_or(600),
        target_lang: args
            .target_lang
            .clone()
            .or_else(|| settings.get().target_lang)
            .unwrap_or_else(|| "ja".to_string()),
        engine: if args.mock {
            EngineChoice::Mock
        } else {
            EngineChoice::April
        },
        ..RuntimeConfig::default()
    };

    let dictionaries = DictionaryRegistry::from_env();
    if dictionaries.is_empty() {
        tracing::warn!("辞書が 1 つも読み込めなかった。単語の語義は出ない");
    }
    let vocab = VocabStore::open(&data_dir)
        .context("語彙ストアを開けない")
        .map_err(other)?;
    tracing::info!(saved = vocab.count().unwrap_or(0), "語彙ストアを読み込んだ");

    let translators = Arc::new(registry);
    // 最初の発話を待たせないよう、裏でモデルを載せておく。
    tokio::spawn({
        let translators = Arc::clone(&translators);
        async move { translators.warm_up_default().await }
    });

    let refinement_enabled = settings.get().refinement_enabled && !args.no_whisper;
    let services = Services {
        translators,
        whisper_threads: (!args.no_whisper).then_some(args.whisper_threads.unwrap_or(2)),
        dictionaries: Arc::new(dictionaries),
        vocab: Arc::new(vocab),
        anki: Arc::new(AnkiConnect::from_env()),
        settings,
        api_keys,
    };

    let overhear = Overhear::start(config, services)
        .context("パイプラインの起動")
        .map_err(other)?;
    if refinement_enabled {
        overhear.set_refinement_enabled(true).await.map_err(other)?;
    }

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
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port.unwrap_or(0)));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| CliError::Target(format!("{addr} を bind できない: {err}")))?;
    let local = listener
        .local_addr()
        .map_err(|err| CliError::Other(err.to_string()))?;

    // Tauri / フロントが読む接続情報。
    let announce = StartServerOutput {
        base_url: format!("http://{local}"),
        graphql_url: format!("http://{local}/graphql"),
        websocket_url: format!("ws://{local}/graphql"),
        token,
    };
    let value = serde_json::to_value(announce).map_err(|err| CliError::Other(err.to_string()))?;
    overhear_cli::emit(global, &value)?;
    let _ = std::io::stdout().flush();

    tracing::info!(%local, "overhear-server を起動した");
    axum::serve(listener, app)
        .await
        .map_err(|err| CliError::Other(err.to_string()))
}

fn execute(cli: Cli) -> Result<(), CliError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| CliError::Other(format!("tokio の起動: {err}")))?;
    match cli.command {
        Command::Server(ServerCommand::StartServer(args)) => {
            runtime.block_on(start_server(&cli.global, args))
        }
    }
}

fn main() {
    // stdout は接続情報 (JSON) 専用にするため、ログは stderr へ出す。
    // Tauri はこの stdout の先頭の JSON を読んで WebView に流し込む。
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,overhear=debug".into()),
        )
        .init();
    std::process::exit(overhear_cli::run::<Cli>(
        std::env::args_os().collect(),
        execute,
    ));
}
