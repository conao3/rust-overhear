//! 直近 N 秒の PCM を保持するリングバッファ。
//!
//! 「字幕行をクリックして聞き直す」「Anki へ音声つきで書き出す」の両方が
//! ここに乗る。絶対サンプル数で位置を管理し、ASR が返す time_ms と同じ
//! 時間軸を共有する。

pub struct RingBuffer {
    buf: Vec<i16>,
    /// 次に書き込む位置 (buf 上の index)。
    write_pos: usize,
    /// これまでに push された総サンプル数。巻き戻らない。
    total: u64,
    sample_rate: u32,
}

impl RingBuffer {
    pub fn new(sample_rate: u32, seconds: usize) -> Self {
        let capacity = sample_rate as usize * seconds;
        Self {
            buf: vec![0; capacity],
            write_pos: 0,
            total: 0,
            sample_rate,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub fn total_samples(&self) -> u64 {
        self.total
    }

    pub fn total_ms(&self) -> u64 {
        self.total * 1000 / self.sample_rate as u64
    }

    pub fn push(&mut self, data: &[i16]) {
        let cap = self.buf.len();
        if cap == 0 {
            return;
        }
        // 1 回の push が容量を超える場合、保持するのは末尾だけだが、
        // total (絶対時間軸) は捨てた分も進める。ここを取り違えると
        // ASR が返す time_ms とリングバッファの位置がずれる。
        let full_len = data.len() as u64;
        let kept = if data.len() > cap {
            &data[data.len() - cap..]
        } else {
            data
        };

        // kept の先頭が置かれる絶対位置。
        let start_abs = self.total + full_len - kept.len() as u64;
        let begin = (start_abs % cap as u64) as usize;
        let first = (cap - begin).min(kept.len());
        self.buf[begin..begin + first].copy_from_slice(&kept[..first]);
        if first < kept.len() {
            let rest = kept.len() - first;
            self.buf[..rest].copy_from_slice(&kept[first..]);
        }

        self.total += full_len;
        // 不変条件: write_pos == total % cap
        self.write_pos = (self.total % cap as u64) as usize;
    }

    /// 指定区間をコピーして返す。既にリングから溢れていれば None。
    pub fn slice_ms(&self, start_ms: u64, end_ms: u64) -> Option<Vec<i16>> {
        if end_ms <= start_ms {
            return None;
        }
        let rate = self.sample_rate as u64;
        let start = start_ms * rate / 1000;
        let end = (end_ms * rate / 1000).min(self.total);
        if start >= end {
            return None;
        }
        let cap = self.buf.len() as u64;
        let oldest = self.total.saturating_sub(cap);
        if start < oldest {
            return None; // 溢れて消えている
        }

        let len = (end - start) as usize;
        let mut out = Vec::with_capacity(len);
        // 絶対位置 -> buf 上の index
        let begin = (start % cap) as usize;
        let first = (self.buf.len() - begin).min(len);
        out.extend_from_slice(&self.buf[begin..begin + first]);
        if first < len {
            out.extend_from_slice(&self.buf[..len - first]);
        }
        Some(out)
    }
}

/// PCM16 mono を WAV バイト列にする。`/audio/{id}.wav` がこれを返す。
pub fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_returns_pushed_range() {
        let mut ring = RingBuffer::new(1000, 2); // 2000 サンプル
        let data: Vec<i16> = (0..1500).map(|i| i as i16).collect();
        ring.push(&data);
        assert_eq!(ring.total_ms(), 1500);
        let got = ring.slice_ms(500, 1000).unwrap();
        assert_eq!(got.len(), 500);
        assert_eq!(got[0], 500);
        assert_eq!(got[499], 999);
    }

    #[test]
    fn slice_returns_none_when_evicted() {
        let mut ring = RingBuffer::new(1000, 1); // 1000 サンプル
        let data: Vec<i16> = (0..2500).map(|i| i as i16).collect();
        ring.push(&data);
        // 0..1500 は既に溢れている
        assert!(ring.slice_ms(0, 500).is_none());
        let got = ring.slice_ms(2000, 2500).unwrap();
        assert_eq!(got.len(), 500);
        assert_eq!(got[0], 2000);
    }

    #[test]
    fn wav_header_is_44_bytes() {
        let wav = encode_wav(&[0, 1, 2, 3], 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 8);
    }
}
