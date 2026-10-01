//! 無音のあいだ ASR への供給を止めるゲート。
//!
//! april-asr は供給された音声に対して常に推論を走らせるため、誰も
//! 喋っていなくても 1 コアの 6 割前後を使い続ける。常駐アプリとしては
//! 重すぎるので、振幅を見て静かな区間は供給しない。
//!
//! 止めたぶん ASR の内部時計は進まなくなる。リングバッファの絶対時間
//! との差は [`ClockMap`] に区切りごとに残し、segment を組むときにトークンの
//! 時刻ごとに足し戻す。

use std::collections::VecDeque;

/// これを超えたら「音がある」とみなす (i16 の振幅)。
/// 小さすぎると環境ノイズで開きっぱなしになり、大きすぎると小声を切る。
pub const DEFAULT_THRESHOLD: u16 = 300;
/// 静かになってから供給を続ける時間。語尾を切らないための余韻。
const HANGOVER_MS: usize = 400;
const LOOKBACK_MS: usize = 128;

pub struct SilenceGate {
    threshold: u16,
    hangover: usize,
    /// 余韻の残りサンプル数。0 なら閉じている。
    remaining: usize,
    /// 開いた瞬間に一緒に流す直前の音声。語頭を切らないため。
    lookback: VecDeque<i16>,
    lookback_samples: usize,
    /// 供給しなかった累積サンプル数。
    skipped: u64,
}

impl SilenceGate {
    pub fn new(threshold: u16, sample_rate: u32) -> Self {
        let lookback_samples = sample_rate as usize * LOOKBACK_MS / 1000;
        Self {
            threshold,
            hangover: sample_rate as usize * HANGOVER_MS / 1000,
            remaining: 0,
            lookback: VecDeque::with_capacity(lookback_samples),
            lookback_samples,
            skipped: 0,
        }
    }

    pub fn skipped_samples(&self) -> u64 {
        self.skipped
    }

    pub fn is_open(&self) -> bool {
        self.remaining > 0
    }

    /// チャンクを受け取り、ASR へ流すべきものを返す。
    ///
    /// `force_closed` は聞き直しの再生中に使う (自分の音を拾い直さない)。
    pub fn admit(&mut self, chunk: &[i16], force_closed: bool) -> Vec<Vec<i16>> {
        if force_closed {
            self.remaining = 0;
            self.lookback.clear();
            self.skipped += chunk.len() as u64;
            return Vec::new();
        }
        let loud = chunk.iter().any(|s| s.unsigned_abs() > self.threshold);
        if loud {
            self.remaining = self.hangover;
            let mut samples = Vec::with_capacity(self.lookback.len() + chunk.len());
            self.skipped -= self.lookback.len() as u64;
            samples.extend(self.lookback.drain(..));
            samples.extend_from_slice(chunk);
            return vec![samples];
        }

        if self.remaining > 0 {
            self.remaining = self.remaining.saturating_sub(chunk.len());
            return vec![chunk.to_vec()];
        }
        let skipped = chunk;
        self.skipped += skipped.len() as u64;
        if skipped.len() >= self.lookback_samples {
            self.lookback.clear();
            self.lookback
                .extend(&skipped[skipped.len() - self.lookback_samples..]);
        } else {
            let remove =
                (self.lookback.len() + skipped.len()).saturating_sub(self.lookback_samples);
            self.lookback.drain(..remove);
            self.lookback.extend(skipped);
        }
        Vec::new()
    }
}

/// ASR の時計 (供給したサンプル数) から絶対時間 (受け取ったサンプル数) への対応。
///
/// 飛ばした量はゲートが閉じるたびに増えるので、segment ごとに一律の差では
/// 戻せない。april は文の確定を次の音声が来てから出すことが多く、その時点の
/// 差を足すと文間の無音ぶん区間が後ろへずれる。ゲートが開いた位置ごとに
/// 「そこから先の飛ばし量」を持ち、トークンの時刻で引く。
#[derive(Debug, Default)]
pub struct ClockMap {
    /// (この位置から先の供給サンプル, それまでに飛ばした累積サンプル)。位置の昇順。
    points: Vec<(u64, u64)>,
}

/// 対応点をこれ以上持たない。古い点はリングバッファの外を指すので捨ててよい。
const MAX_CLOCK_POINTS: usize = 4096;

impl ClockMap {
    /// 供給位置 `fed` から先は累積 `skipped` サンプルを飛ばした時間軸にある。
    pub fn record(&mut self, fed: u64, skipped: u64) {
        match self.points.last_mut() {
            Some(&mut (_, last)) if last == skipped => return,
            Some(last) if last.0 == fed => {
                last.1 = skipped;
                return;
            }
            _ => {}
        }
        self.points.push((fed, skipped));
        if self.points.len() > MAX_CLOCK_POINTS {
            self.points.drain(..MAX_CLOCK_POINTS / 2);
        }
    }

    /// ASR の時刻 (ms) を絶対時間 (ms) にする。
    pub fn to_absolute_ms(&self, asr_ms: u64, sample_rate: u32) -> u64 {
        let rate = sample_rate.max(1) as u64;
        let fed = asr_ms * rate / 1000;
        let index = self.points.partition_point(|&(at, _)| at <= fed);
        let skipped = if index == 0 {
            0
        } else {
            self.points[index - 1].1
        };
        asr_ms + skipped * 1000 / rate
    }
}

impl Default for SilenceGate {
    fn default() -> Self {
        Self::new(DEFAULT_THRESHOLD, 16_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> Vec<i16> {
        vec![10; 100]
    }
    fn loud() -> Vec<i16> {
        vec![5000; 100]
    }

    #[test]
    fn closed_while_quiet() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 1_000);
        assert!(gate.admit(&quiet(), false).is_empty());
        assert!(gate.admit(&quiet(), false).is_empty());
        // 供給しなかったぶんは数えられている
        assert_eq!(gate.skipped_samples(), 200);
        assert!(!gate.is_open());
    }

    #[test]
    fn opens_with_lookback() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 1_000);
        gate.admit(&quiet(), false); // 直前として覚えられる
        let out = gate.admit(&loud(), false);
        // 語頭を切らないよう、直前の音声も一緒に流す
        assert_eq!(out, vec![[quiet(), loud()].concat()]);
        // 流した以上、飛ばした扱いは取り消す
        assert_eq!(gate.skipped_samples(), 0);
        assert!(gate.is_open());
    }

    #[test]
    fn hangover_keeps_feeding_after_speech() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 1_000);
        gate.admit(&loud(), false);
        // 余韻のあいだは静かでも流す
        for _ in 0..4 {
            assert_eq!(gate.admit(&quiet(), false).len(), 1);
        }
        // 余韻が切れたら閉じる
        assert!(gate.admit(&quiet(), false).is_empty());
    }

    #[test]
    fn clock_map_offsets_each_token_by_its_own_gap() {
        let mut clock = ClockMap::default();
        // 1 秒飛ばしてから 0 サンプル目を供給、2 秒ぶん話して、さらに 3 秒飛ばした。
        clock.record(0, 16_000);
        clock.record(32_000, 64_000);
        // 最初の文は 1 秒の差だけ戻す。確定が後から来ても 4 秒にはならない。
        assert_eq!(clock.to_absolute_ms(500, 16_000), 1_500);
        assert_eq!(clock.to_absolute_ms(1_999, 16_000), 2_999);
        // 次の文は 4 秒の差。
        assert_eq!(clock.to_absolute_ms(2_000, 16_000), 6_000);
    }

    #[test]
    fn clock_map_merges_points() {
        let mut clock = ClockMap::default();
        assert_eq!(clock.to_absolute_ms(100, 16_000), 100);
        clock.record(0, 1_600);
        clock.record(0, 3_200); // 同じ位置は上書き
        clock.record(800, 3_200); // 差が変わらなければ足さない
        assert_eq!(clock.points, vec![(0, 3_200)]);
    }

    #[test]
    fn force_closed_ignores_loud_audio() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 1_000);
        // 聞き直しの再生中は、音が大きくても拾わない
        assert!(gate.admit(&loud(), true).is_empty());
        assert_eq!(gate.skipped_samples(), 100);
    }
    #[test]
    fn gate_timing_is_independent_of_capture_chunk_size() {
        for chunk_size in [1, 5, 10, 20, 40, 100] {
            let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 1_000);
            let mut input = vec![0; 1_000];
            input.extend(vec![5000; 200]);
            input.extend(vec![0; 1_000]);
            let admitted: Vec<_> = input
                .chunks(chunk_size)
                .flat_map(|chunk| gate.admit(chunk, false))
                .flatten()
                .collect();
            assert_eq!(admitted.len(), 128 + 200 + 400, "chunk_size={chunk_size}");
            assert_eq!(
                gate.skipped_samples() as usize + admitted.len(),
                input.len()
            );
            assert_eq!(&admitted[128..328], &[5000; 200]);
        }
    }

    #[test]
    fn muting_discards_hangover_and_lookback() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 1_000);
        gate.admit(&loud(), false);
        assert!(gate.admit(&loud(), true).is_empty());
        assert!(!gate.is_open());
        assert_eq!(gate.admit(&loud(), false), vec![loud()]);
        assert_eq!(gate.skipped_samples(), 100);
    }
}
