//! 16-bit mono PCM helpers. Every voice provider is asked for 24 kHz PCM, so
//! lines can be joined with exact timing and written out as one WAV file.
//! Port of `packages/engine/src/podcast/audio.ts`.

use base64::Engine;

pub const SAMPLE_RATE: u32 = 24_000;

pub fn base64_to_bytes(b64: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(b64.trim()).unwrap_or_default()
}

pub fn bytes_to_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Little-endian 16-bit samples from raw bytes (an odd trailing byte is dropped).
pub fn bytes_to_pcm(bytes: &[u8]) -> Vec<i16> {
    bytes.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect()
}

pub fn pcm_to_bytes(pcm: &[i16]) -> Vec<u8> {
    pcm.iter().flat_map(|s| s.to_le_bytes()).collect()
}

pub fn silence(seconds: f64, rate: u32) -> Vec<i16> {
    vec![0; (seconds * rate as f64).round() as usize]
}

/// Linear resampling, for providers that answer at another rate.
pub fn resample(pcm: Vec<i16>, from: u32, to: u32) -> Vec<i16> {
    if from == to || pcm.is_empty() || from == 0 {
        return pcm;
    }
    let len = (pcm.len() as f64 * to as f64 / from as f64).round() as usize;
    let step = from as f64 / to as f64;
    (0..len)
        .map(|i| {
            let pos = i as f64 * step;
            let a = (pos.floor() as usize).min(pcm.len() - 1);
            let b = (a + 1).min(pcm.len() - 1);
            let (sa, sb) = (pcm[a] as f64, pcm[b] as f64);
            (sa + (sb - sa) * (pos - a as f64)).round() as i16
        })
        .collect()
}

pub(crate) const AUDIBLE: i32 = 120;

/// The audible part, keeping a little air (`pad_seconds`) on each side.
pub fn trim_silence(pcm: &[i16], pad_seconds: f64, rate: u32) -> Vec<i16> {
    let loud = |s: &i16| (*s as i32).abs() > AUDIBLE;
    let Some(start) = pcm.iter().position(loud) else { return Vec::new() };
    let end = pcm.iter().rposition(loud).map_or(start, |i| i + 1);
    let pad = (pad_seconds * rate as f64).round() as usize;
    pcm[start.saturating_sub(pad)..(end + pad).min(pcm.len())].to_vec()
}

/// `trim_silence` with the TypeScript defaults: 80 ms of air at 24 kHz.
pub fn trim(pcm: &[i16]) -> Vec<i16> {
    trim_silence(pcm, 0.08, SAMPLE_RATE)
}

/// Where a piece sits in the timeline, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

/// Appends PCM pieces and reports where each one starts, in seconds.
#[derive(Debug, Default)]
pub struct Timeline {
    samples: Vec<i16>,
    rate: u32,
}

impl Timeline {
    pub fn new(rate: u32) -> Self {
        Timeline { samples: Vec::new(), rate }
    }

    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / self.rate as f64
    }

    pub fn append(&mut self, pcm: &[i16]) -> Span {
        let start = self.seconds();
        self.samples.extend_from_slice(pcm);
        Span { start, end: self.seconds() }
    }

    pub fn pause(&mut self, seconds: f64) {
        let gap = silence(seconds, self.rate);
        self.append(&gap);
    }

    pub fn into_pcm(self) -> Vec<i16> {
        self.samples
    }
}

/// A standard 16-bit mono WAV file.
pub fn encode_wav(pcm: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_files_have_a_correct_header_and_samples() {
        let wav = encode_wav(&[0, 1000, -1000], SAMPLE_RATE);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), SAMPLE_RATE);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(bytes_to_pcm(&wav[44..]), [0, 1000, -1000]);
    }

    #[test]
    fn trim_silence_keeps_the_audible_part_with_a_little_air() {
        let mut pcm = vec![0i16; SAMPLE_RATE as usize];
        pcm[10_000..11_000].fill(500);
        assert_eq!(trim_silence(&pcm, 0.01, SAMPLE_RATE).len(), 1000 + 2 * 240);
        assert!(trim(&[0; 100]).is_empty());
    }

    #[test]
    fn the_timeline_reports_where_each_piece_starts() {
        let mut t = Timeline::new(SAMPLE_RATE);
        assert_eq!(t.append(&vec![0; SAMPLE_RATE as usize]), Span { start: 0.0, end: 1.0 });
        t.pause(0.5);
        assert_eq!(t.append(&vec![0; SAMPLE_RATE as usize / 2]), Span { start: 1.5, end: 2.0 });
    }

    #[test]
    fn resampling_changes_the_length_by_the_rate_ratio() {
        let out = resample(vec![0, 100, 200, 300], 12_000, 24_000);
        assert_eq!(out.len(), 8);
        assert_eq!(&out[..3], &[0, 50, 100]);
        assert_eq!(resample(vec![1, 2], 24_000, 24_000), [1, 2]);
    }
}
