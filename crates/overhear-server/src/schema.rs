//! Query / Mutation / Subscription。
//!
//! Subscription が本体で、interim → final の差し替えも同じ id の更新として
//! `segmentUpdates` に流れる。フロントは Apollo の正規化キャッシュで受ける。

use std::sync::Arc;

use async_graphql::{Context, ID, Object, Schema, Subscription};
use futures_util::{Stream, StreamExt};
use overhear_core::Overhear;
use tokio_stream::wrappers::BroadcastStream;

use crate::types::{
    AnkiExportResult, AudioDevice, CaptureState, DictEntry, Segment, TranslationEngineInfo,
    VocabItem,
};

pub type OverhearSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

fn engine(ctx: &Context<'_>) -> Arc<Overhear> {
    ctx.data_unchecked::<Arc<Overhear>>().clone()
}

fn capture_state(overhear: &Overhear) -> CaptureState {
    let captured_ms = overhear
        .ring
        .lock()
        .map(|r| r.total_ms() as i32)
        .unwrap_or(0);
    CaptureState {
        running: true,
        sample_rate: overhear.config.sample_rate as i32,
        ring_seconds: overhear.config.ring_seconds as i32,
        captured_ms,
        asr_engine: match overhear.config.engine {
            overhear_core::EngineChoice::April => "april".into(),
            overhear_core::EngineChoice::Mock => "mock".into(),
        },
        target_lang: overhear.config.target_lang.clone(),
        muted: overhear.is_muted(),
        refiner: overhear.whisper.as_ref().map(|_| "whisper.cpp".to_string()),
        capture_target: overhear.capture_target(),
        translation_backlog: overhear.translation_backlog() as i32,
    }
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// 直近の segment を古い順に返す。
    async fn segments(&self, ctx: &Context<'_>, limit: Option<i32>) -> Vec<Segment> {
        let overhear = engine(ctx);
        let limit = limit.unwrap_or(50).clamp(1, 500) as usize;
        overhear
            .recent(limit)
            .into_iter()
            .map(Segment::from)
            .collect()
    }

    async fn segment(&self, ctx: &Context<'_>, id: ID) -> Option<Segment> {
        let overhear = engine(ctx);
        let id = id.parse::<u64>().ok()?;
        overhear.segment(id).map(Segment::from)
    }

    async fn capture_state(&self, ctx: &Context<'_>) -> CaptureState {
        capture_state(&engine(ctx))
    }

    /// 音声の入出力ノード。どこの音を拾うかを選ばせる。
    async fn audio_devices(&self, ctx: &Context<'_>) -> Vec<AudioDevice> {
        engine(ctx)
            .audio_devices()
            .into_iter()
            .map(AudioDevice::from)
            .collect()
    }

    /// 単語を辞書で引く。活用は見出し語へ解かれる。
    async fn lookup(&self, ctx: &Context<'_>, word: String) -> Vec<DictEntry> {
        engine(ctx)
            .lookup(&word)
            .into_iter()
            .map(DictEntry::from)
            .collect()
    }

    /// 保存した語彙を新しい順に返す。
    async fn vocab(
        &self,
        ctx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Vec<VocabItem> {
        let overhear = engine(ctx);
        let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
        let offset = offset.unwrap_or(0).max(0) as usize;
        overhear
            .vocab
            .list(limit, offset)
            .unwrap_or_default()
            .into_iter()
            .map(VocabItem::from)
            .collect()
    }

    /// 登録済み翻訳ストラテジーの一覧と利用可否。
    async fn translation_engines(&self, ctx: &Context<'_>) -> Vec<TranslationEngineInfo> {
        translation_engines(&engine(ctx)).await
    }
}

async fn translation_engines(overhear: &Overhear) -> Vec<TranslationEngineInfo> {
    let default_id = overhear.translators.default_id();
    let mut out = Vec::new();
    for engine in overhear.translators.list() {
        let caps = engine.capabilities();
        let availability = engine.availability().await;
        out.push(TranslationEngineInfo {
            id: ID(engine.id().to_string()),
            display_name: engine.display_name().to_string(),
            available: availability.available,
            unavailable_reason: availability.reason,
            sends_data_externally: caps.sends_data_externally,
            supported_target_langs: caps.supported_target_langs,
            is_default: engine.id() == default_id,
        });
    }
    out
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    /// 拾う先を切り替える。deviceId を省くと既定シンクに戻る。
    async fn set_capture_device(
        &self,
        ctx: &Context<'_>,
        device_id: Option<ID>,
    ) -> async_graphql::Result<CaptureState> {
        let overhear = engine(ctx);
        overhear
            .set_capture_device(device_id.as_ref().map(|d| d.as_str()))
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(capture_state(&overhear))
    }

    /// 自動翻訳に使うエンジンを切り替える。次の起動にも持ち越す。
    async fn set_default_translator(
        &self,
        ctx: &Context<'_>,
        engine_id: ID,
    ) -> async_graphql::Result<Vec<TranslationEngineInfo>> {
        let overhear = engine(ctx);
        overhear
            .set_default_translator(engine_id.as_str())
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(translation_engines(&overhear).await)
    }

    /// segment の 1 語を語彙ストアへ保存する。
    ///
    /// 保存時点の文・訳・語義・音声を焼き付けるので、後から segment が
    /// 消えても残る。
    async fn save_vocab(
        &self,
        ctx: &Context<'_>,
        segment_id: ID,
        token_index: i32,
    ) -> async_graphql::Result<VocabItem> {
        let overhear = engine(ctx);
        let id = segment_id
            .parse::<u64>()
            .map_err(|_| async_graphql::Error::new("segmentId が数値でない"))?;
        let item = overhear
            .save_vocab(id, token_index.max(0) as usize)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(VocabItem::from(item))
    }

    async fn remove_vocab(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<bool> {
        let overhear = engine(ctx);
        let id = id
            .parse::<i64>()
            .map_err(|_| async_graphql::Error::new("id が数値でない"))?;
        overhear
            .vocab
            .remove(id)
            .map_err(|e| async_graphql::Error::new(e.to_string()))
    }

    /// 語彙を Anki へ送る。1 件ずつ失敗理由を返す。
    async fn export_to_anki(
        &self,
        ctx: &Context<'_>,
        vocab_ids: Vec<ID>,
        deck: Option<String>,
    ) -> AnkiExportResult {
        let overhear = engine(ctx);
        let ids: Vec<i64> = vocab_ids
            .iter()
            .filter_map(|i| i.parse::<i64>().ok())
            .collect();
        overhear.export_to_anki(&ids, deck.as_deref()).await.into()
    }

    /// 指定時間だけ入力を無音として扱う。
    ///
    /// 聞き直しの再生音は既定シンクの monitor に戻ってくるため、
    /// フロントは再生の前後をこれで挟む。
    async fn mute_capture(&self, ctx: &Context<'_>, ms: i32) -> CaptureState {
        let overhear = engine(ctx);
        overhear.mute_for(ms.clamp(0, 60_000) as u64);
        capture_state(&overhear)
    }

    /// 任意のエンジンで翻訳を引き直す。結果は translations に追加される。
    async fn retranslate(
        &self,
        ctx: &Context<'_>,
        segment_id: ID,
        engine_id: Option<ID>,
    ) -> Option<Segment> {
        let overhear = engine(ctx);
        let id = segment_id.parse::<u64>().ok()?;
        overhear
            .translate_segment(id, engine_id.as_ref().map(|e| e.as_str()))
            .await
            .map(Segment::from)
    }
}

pub struct SubscriptionRoot;

#[Subscription]
impl SubscriptionRoot {
    /// 新規 segment と差し替えの両方がここに流れる。
    async fn segment_updates(&self, ctx: &Context<'_>) -> impl Stream<Item = Segment> {
        let overhear = engine(ctx);
        BroadcastStream::new(overhear.updates.subscribe())
            .filter_map(|res| async move { res.ok().map(Segment::from) })
    }
}
