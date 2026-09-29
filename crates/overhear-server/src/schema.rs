//! Query / Mutation / Subscription。
//!
//! Subscription が本体で、interim → final の差し替えも同じ id の更新として
//! `segmentUpdates` に流れる。フロントは Apollo の正規化キャッシュで受ける。

use std::sync::Arc;

use async_graphql::{Context, ID, Object, Schema, Subscription};
use futures_util::{Stream, StreamExt};
use overhear_core::Overhear;
use tokio_stream::wrappers::BroadcastStream;

use crate::types::{CaptureState, Segment, TranslationEngineInfo};

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

    /// 登録済み翻訳ストラテジーの一覧と利用可否。
    async fn translation_engines(&self, ctx: &Context<'_>) -> Vec<TranslationEngineInfo> {
        let overhear = engine(ctx);
        let default_id = overhear.translators.default_id().to_string();
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
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
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
