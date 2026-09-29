//! 無音のあいだ ASR への供給を止めるゲート。
//!
//! april-asr は供給された音声に対して常に推論を走らせるため、誰も
//! 喋っていなくても 1 コアの 6 割前後を使い続ける。常駐アプリとしては
//! 重すぎるので、振幅を見て静かな区間は供給しない。
//!
//! 止めたぶん ASR の内部時計は進まなくなる。リングバッファの絶対時間
//! との差は `skipped_samples` で持ち、segment を組むときに足し戻す。

/// これを超えたら「音がある」とみなす (i16 の振幅)。
/// 小さすぎると環境ノイズで開きっぱなしになり、大きすぎると小声を切る。
pub const DEFAULT_THRESHOLD: u16 = 300;
/// 静かになってから供給を続けるチャンク数。語尾を切らないための余韻。
pub const DEFAULT_HANGOVER: u32 = 8;

pub struct SilenceGate {
    threshold: u16,
    hangover: u32,
    /// 余韻の残り。0 なら閉じている。
    remaining: u32,
    /// 開いた瞬間に一緒に流す直前のチャンク。語頭を切らないため。
    lookback: Option<Vec<i16>>,
    /// 供給しなかった累積サンプル数。
    skipped: u64,
}

impl SilenceGate {
    pub fn new(threshold: u16, hangover: u32) -> Self {
        Self {
            threshold,
            hangover,
            remaining: 0,
            lookback: None,
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
        let peak = chunk.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let loud = !force_closed && peak > self.threshold;

        if loud {
            let was_closed = self.remaining == 0;
            self.remaining = self.hangover;
            let mut out = Vec::with_capacity(2);
            // 閉じていたところから開いたときだけ、直前のチャンクを添える。
            if was_closed {
                if let Some(prev) = self.lookback.take() {
                    self.skipped = self.skipped.saturating_sub(prev.len() as u64);
                    out.push(prev);
                }
            }
            self.lookback = None;
            out.push(chunk.to_vec());
            return out;
        }

        if self.remaining > 0 {
            self.remaining -= 1;
            self.lookback = None;
            return vec![chunk.to_vec()];
        }

        // 閉じている。次に開いたとき用に直前だけ覚えておく。
        self.skipped += chunk.len() as u64;
        self.lookback = Some(chunk.to_vec());
        Vec::new()
    }
}

impl Default for SilenceGate {
    fn default() -> Self {
        Self::new(DEFAULT_THRESHOLD, DEFAULT_HANGOVER)
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
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 2);
        assert!(gate.admit(&quiet(), false).is_empty());
        assert!(gate.admit(&quiet(), false).is_empty());
        // 供給しなかったぶんは数えられている
        assert_eq!(gate.skipped_samples(), 200);
        assert!(!gate.is_open());
    }

    #[test]
    fn opens_with_lookback() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 2);
        gate.admit(&quiet(), false); // 直前として覚えられる
        let out = gate.admit(&loud(), false);
        // 語頭を切らないよう、直前の 1 チャンクも一緒に流す
        assert_eq!(out.len(), 2);
        // 流した以上、飛ばした扱いは取り消す
        assert_eq!(gate.skipped_samples(), 0);
        assert!(gate.is_open());
    }

    #[test]
    fn hangover_keeps_feeding_after_speech() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 2);
        gate.admit(&loud(), false);
        // 余韻のあいだは静かでも流す
        assert_eq!(gate.admit(&quiet(), false).len(), 1);
        assert_eq!(gate.admit(&quiet(), false).len(), 1);
        // 余韻が切れたら閉じる
        assert!(gate.admit(&quiet(), false).is_empty());
    }

    #[test]
    fn force_closed_ignores_loud_audio() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD, 2);
        // 聞き直しの再生中は、音が大きくても拾わない
        assert!(gate.admit(&loud(), true).is_empty());
        assert_eq!(gate.skipped_samples(), 100);
    }
}
