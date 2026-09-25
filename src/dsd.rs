//! DSD(DSF / DSDIFF)の読み込みと、DSD→PCM変換(DSD非対応のハードウェア向けの自動PCM化)。
//!
//! - DSF(Sony): 1bit・LSBファースト、チャンネルごとのブロック(通常4096バイト)が交互に並ぶ。
//! - DSDIFF/DFF(Philips): ビッグエンディアンのチャンク構造、バイトはMSBファースト・チャンネル交互。
//!   **DST圧縮のDFFは未対応**(明示的なエラーにする)。
//! 内部表現は「チャンネルごとの、時間順・MSBファーストのバイト列」に統一する(DoPパッキングがそのまま使える)。
//!
//! DSD→PCMは窓付きsinc(Kaiser)のFIR間引き。ビット列が±1なので、8タップずつ256通りの部分和を表引きして
//! 高速化する(バイト境界に揃うよう、間引き率Rは8の倍数・タップ数は16の倍数)。

use std::io::Read;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DsdError {
    #[error("DSDファイルを読めません: {0}")]
    Io(#[from] std::io::Error),
    #[error("DSDファイルの形式が不正です: {0}")]
    Format(String),
    #[error("未対応です: {0}")]
    Unsupported(String),
}

/// 時間順・MSBファーストのDSDビットストリーム(チャンネルごと)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsdStream {
    /// DSDのビットレート(Hz)。DSD64=2,822,400、DSD256=11,289,600。
    pub rate_hz: u32,
    /// チャンネルごとのバイト列(全チャンネル同じ長さ)。
    pub channels: Vec<Vec<u8>>,
    /// 1チャンネルあたりの有効ビット数(末尾のパディングを除く)。
    pub sample_bits: u64,
}

impl DsdStream {
    /// DSD64=64、DSD128=128 … の倍率(44.1kHz系のみ。48kHz系は端数の倍率になる)。
    pub fn multiplier(&self) -> u32 {
        self.rate_hz / 44_100
    }
    pub fn duration_secs(&self) -> f64 {
        self.sample_bits as f64 / self.rate_hz as f64
    }
}

fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn u64le(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

/// バイトのビット順を反転する(DSFのLSBファースト→MSBファースト)。
fn reverse_bits(v: &mut [u8]) {
    for b in v.iter_mut() {
        *b = b.reverse_bits();
    }
}

pub fn parse_dsf(bytes: &[u8]) -> Result<DsdStream, DsdError> {
    if bytes.len() < 92 || &bytes[0..4] != b"DSD " || &bytes[28..32] != b"fmt " || &bytes[80..84] != b"data" {
        return Err(DsdError::Format("DSFのヘッダ('DSD '/'fmt '/'data')が見つかりません".into()));
    }
    let channels = u32le(bytes, 52) as usize;
    let rate_hz = u32le(bytes, 56);
    let bits_per_sample = u32le(bytes, 60);
    let sample_bits = u64le(bytes, 64);
    let block = u32le(bytes, 72) as usize;
    let data_size = u64le(bytes, 84).saturating_sub(12) as usize;
    if channels == 0 || channels > 8 || block == 0 || (bits_per_sample != 1 && bits_per_sample != 8) {
        return Err(DsdError::Format(format!("DSFのfmtが不正です(ch={channels}, bits={bits_per_sample}, block={block})")));
    }
    let data = &bytes[92..bytes.len().min(92 + data_size)];
    let mut out: Vec<Vec<u8>> = vec![Vec::with_capacity(data.len() / channels); channels];
    let group = block * channels;
    for chunk in data.chunks(group) {
        for (c, o) in out.iter_mut().enumerate() {
            let start = c * block;
            if start < chunk.len() {
                o.extend_from_slice(&chunk[start..chunk.len().min(start + block)]);
            }
        }
    }
    let valid = (sample_bits.div_ceil(8)) as usize;
    for o in out.iter_mut() {
        o.truncate(valid);
        if bits_per_sample == 1 {
            reverse_bits(o); // LSBファースト → MSBファースト
        }
    }
    Ok(DsdStream { rate_hz, channels: out, sample_bits })
}

pub fn parse_dff(bytes: &[u8]) -> Result<DsdStream, DsdError> {
    if bytes.len() < 16 || &bytes[0..4] != b"FRM8" || &bytes[12..16] != b"DSD " {
        return Err(DsdError::Format("DSDIFFのヘッダ('FRM8'/'DSD ')が見つかりません".into()));
    }
    let be64 = |i: usize| u64::from_be_bytes(bytes[i..i + 8].try_into().unwrap());
    let mut pos = 16;
    let (mut rate, mut channels, mut data): (u32, usize, Option<&[u8]>) = (0, 0, None);
    while pos + 12 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = be64(pos + 4) as usize;
        let body = pos + 12;
        let end = (body + size).min(bytes.len());
        match id {
            b"PROP" if body + 4 <= end && &bytes[body..body + 4] == b"SND " => {
                let mut p = body + 4;
                while p + 12 <= end {
                    let sid = &bytes[p..p + 4];
                    let ssize = be64(p + 4) as usize;
                    let sb = p + 12;
                    match sid {
                        b"FS  " if sb + 4 <= end => rate = u32::from_be_bytes(bytes[sb..sb + 4].try_into().unwrap()),
                        b"CHNL" if sb + 2 <= end => channels = u16::from_be_bytes(bytes[sb..sb + 2].try_into().unwrap()) as usize,
                        b"CMPR" if sb + 4 <= end && &bytes[sb..sb + 4] != b"DSD " => {
                            return Err(DsdError::Unsupported("DST圧縮のDSDIFFは未対応です(非圧縮のDSDIFF/DSFを使ってください)".into()));
                        }
                        _ => {}
                    }
                    p = sb + ssize + (ssize & 1);
                }
            }
            b"DSD " => data = Some(&bytes[body..end]),
            _ => {}
        }
        pos = body + size + (size & 1);
    }
    let data = data.ok_or_else(|| DsdError::Format("DSDIFFにデータチャンクがありません".into()))?;
    if rate == 0 || channels == 0 || channels > 8 {
        return Err(DsdError::Format("DSDIFFのFS/CHNLが見つかりません".into()));
    }
    let per = data.len() / channels;
    let mut out: Vec<Vec<u8>> = vec![Vec::with_capacity(per); channels];
    for frame in data.chunks_exact(channels) {
        for (c, o) in out.iter_mut().enumerate() {
            o.push(frame[c]);
        }
    }
    Ok(DsdStream { rate_hz: rate, channels: out, sample_bits: per as u64 * 8 })
}

/// ファイルの先頭バイトからDSF/DSDIFFを判別して読む。
pub fn read_dsd_file(path: &str) -> Result<DsdStream, DsdError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.starts_with(b"DSD ") {
        parse_dsf(&bytes)
    } else if bytes.starts_with(b"FRM8") {
        parse_dff(&bytes)
    } else {
        Err(DsdError::Format("DSF/DSDIFFではありません".into()))
    }
}

/// ファイルがDSD(DSF/DSDIFF)かを先頭4バイトで調べる。
pub fn is_dsd_file(path: &str) -> bool {
    let mut head = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && (&head == b"DSD " || &head == b"FRM8")
}

// ---- DSD → PCM ----

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        k += 1.0;
    }
    sum
}

/// Kaiser窓付きsincローパス(`taps`個、DC利得1、`fc`は入力レートで正規化したカットオフ)。
fn design_lowpass(taps: usize, fc: f64, beta: f64) -> Vec<f64> {
    let m = (taps - 1) as f64 / 2.0;
    let i0b = bessel_i0(beta);
    let mut h: Vec<f64> = (0..taps)
        .map(|n| {
            let x = n as f64 - m;
            let sinc = if x == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x) };
            let r = x / m;
            sinc * bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0b
        })
        .collect();
    let sum: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= sum);
    h
}

/// DSD→PCM変換の設定。
#[derive(Debug, Clone, Copy)]
pub struct DsdToPcm {
    /// 出力PCMレート(Hz)。DSDレートを割り切れ、間引き率が8の倍数になる値(例: DSD64→176,400)。
    pub out_rate_hz: u32,
    /// ローパスのカットオフ(Hz)。既定40kHz(SACDの一般的な帯域)。
    pub cutoff_hz: f64,
}

impl DsdToPcm {
    /// DSDのレートに対する既定の出力レート(44.1kHz系で176.4kHz、無理なら最も近い割り切れる値)。
    pub fn default_for(dsd_rate_hz: u32) -> Self {
        DsdToPcm { out_rate_hz: dsd_rate_hz / 16, cutoff_hz: 40_000.0 }
    }
}

/// DSD→PCM間引きフィルタ(係数表を一度だけ作り、任意の出力範囲を計算できる。逐次再生・シークに使う)。
pub struct Decimator {
    r: usize,
    half: usize,
    table: Vec<[f32; 256]>,
}

impl Decimator {
    pub fn new(dsd_rate_hz: u32, cfg: DsdToPcm) -> Result<Decimator, DsdError> {
        if cfg.out_rate_hz == 0 || dsd_rate_hz % cfg.out_rate_hz != 0 {
            return Err(DsdError::Unsupported(format!("出力{}HzはDSD{}Hzを割り切れません", cfg.out_rate_hz, dsd_rate_hz)));
        }
        let r = (dsd_rate_hz / cfg.out_rate_hz) as usize;
        if r % 8 != 0 {
            return Err(DsdError::Unsupported(format!("間引き率{r}は8の倍数である必要があります")));
        }
        let taps = ((r * 12).max(256) + 15) / 16 * 16;
        let fc = (cfg.cutoff_hz.min(cfg.out_rate_hz as f64 * 0.45)) / dsd_rate_hz as f64;
        let h = design_lowpass(taps, fc, 9.0);
        let groups = taps / 8;
        // table[g][byte] = Σ h[8g+i] * (bit_i ? +1 : -1)、bit_iはMSBから数えてi番目
        let mut table = vec![[0f32; 256]; groups];
        for (g, t) in table.iter_mut().enumerate() {
            for (byte, slot) in t.iter_mut().enumerate() {
                let mut s = 0.0;
                for i in 0..8 {
                    s += if (byte >> (7 - i)) & 1 == 1 { h[8 * g + i] } else { -h[8 * g + i] };
                }
                *slot = s as f32;
            }
        }
        Ok(Decimator { r, half: taps / 2, table })
    }

    /// `bits`(1チャンネル分)から出力フレーム`[first, first+count)`を計算して`out`へ追加する(範囲外の入力は0扱い)。
    pub fn process_range(&self, bits: &[u8], first: usize, count: usize, out: &mut Vec<f32>) {
        for n in first..first + count {
            // 窓の先頭ビット位置(=n*r - half)は常にバイト境界
            let start = (n * self.r) as isize - self.half as isize;
            let mut acc = 0f32;
            for (g, t) in self.table.iter().enumerate() {
                let bit = start + (g * 8) as isize;
                if bit < 0 || (bit as usize) / 8 >= bits.len() {
                    continue;
                }
                acc += t[bits[(bit / 8) as usize] as usize];
            }
            out.push(acc);
        }
    }

    /// 入力ビット列全体から得られる出力フレーム数。
    pub fn output_len(&self, bits: &[u8]) -> usize {
        bits.len() * 8 / self.r
    }
}

/// 1チャンネルのDSDをPCM(f32、±1.0が±100%変調)へ変換する。
pub fn decimate_channel(bits: &[u8], dsd_rate_hz: u32, cfg: DsdToPcm) -> Result<Vec<f32>, DsdError> {
    let d = Decimator::new(dsd_rate_hz, cfg)?;
    let n = d.output_len(bits);
    let mut out = Vec::with_capacity(n);
    d.process_range(bits, 0, n, &mut out);
    Ok(out)
}

/// 全チャンネルを並列にPCMへ変換し、インターリーブしたf32を返す。
pub fn dsd_to_pcm(stream: &DsdStream, cfg: DsdToPcm) -> Result<(Vec<f32>, u32), DsdError> {
    let chans: Vec<Result<Vec<f32>, DsdError>> = std::thread::scope(|s| {
        let hs: Vec<_> = stream.channels.iter().map(|c| s.spawn(move || decimate_channel(c, stream.rate_hz, cfg))).collect();
        hs.into_iter().map(|h| h.join().expect("decimator thread panicked")).collect()
    });
    let chans: Vec<Vec<f32>> = chans.into_iter().collect::<Result<_, _>>()?;
    let n = chans.iter().map(|c| c.len()).min().unwrap_or(0);
    let mut out = Vec::with_capacity(n * chans.len());
    for i in 0..n {
        for c in &chans {
            out.push(c[i]);
        }
    }
    Ok((out, cfg.out_rate_hz))
}

/// DoP(DSD over PCM)のフレーム(24bit)をチャンネルごとに作る。DoP対応DACへビットパーフェクトで送る用。
pub fn to_dop(stream: &DsdStream) -> Result<(Vec<Vec<open_mqa::dop::PcmFrame24>>, u32), DsdError> {
    let cfg = open_mqa::dop::DopConfig { format: open_mqa::dop::DsdFormat { dsd_bitrate_hz: stream.rate_hz }, container_bits: 24 };
    let rate = cfg.format.dop_pcm_sample_rate_hz();
    let mut out = Vec::new();
    for c in &stream.channels {
        let mut b = c.clone();
        if b.len() % 2 == 1 {
            b.push(0x69); // DSDの無音パターン
        }
        out.push(open_mqa::dop::pack_dop_frames(&b, &cfg).map_err(|e| DsdError::Format(e.to_string()))?);
    }
    Ok((out, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正弦波を高次ΔΣ(2次)で1bit化したDSD64相当のテストビット列を作る(テスト用の簡易変調器)。
    fn modulate_sine(freq: f64, amp: f64, rate: u32, seconds: f64) -> Vec<u8> {
        let n = (rate as f64 * seconds) as usize / 8 * 8;
        let (mut i1, mut i2) = (0.0f64, 0.0f64);
        let mut y = 0.0f64;
        let mut bytes = vec![0u8; n / 8];
        for k in 0..n {
            let u = amp * (2.0 * std::f64::consts::PI * freq * k as f64 / rate as f64).sin();
            i1 += u - y;
            i2 += i1 - y;
            y = if i2 >= 0.0 { 1.0 } else { -1.0 };
            if y > 0.0 {
                bytes[k / 8] |= 0x80 >> (k % 8);
            }
        }
        bytes
    }

    fn tone_amplitude(x: &[f32], rate: u32, freq: f64) -> f64 {
        let (mut s, mut c) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let ph = 2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64;
            s += *v as f64 * ph.sin();
            c += *v as f64 * ph.cos();
        }
        2.0 * (s * s + c * c).sqrt() / x.len() as f64
    }

    #[test]
    fn dsd64_sine_decimates_to_pcm_with_the_right_amplitude() {
        let bits = modulate_sine(1000.0, 0.5, 2_822_400, 0.5);
        let pcm = decimate_channel(&bits, 2_822_400, DsdToPcm::default_for(2_822_400)).unwrap();
        assert_eq!(pcm.len(), bits.len() * 8 / 16);
        // 先頭・末尾(フィルタ過渡)を除いて測る
        let mid = &pcm[2000..pcm.len() - 2000];
        let a = tone_amplitude(mid, 176_400, 1000.0);
        assert!((a - 0.5).abs() < 0.02, "1kHz振幅が0.5のはず: {a}");
    }

    #[test]
    fn range_processing_matches_full_decimation_exactly_for_any_split() {
        let bits = modulate_sine(1500.0, 0.4, 2_822_400, 0.05);
        let cfg = DsdToPcm::default_for(2_822_400);
        let full = decimate_channel(&bits, 2_822_400, cfg).unwrap();
        let d = Decimator::new(2_822_400, cfg).unwrap();
        let mut parts = Vec::new();
        let mut at = 0;
        for size in [1usize, 7, 300, 2048, 5000] {
            let n = size.min(full.len() - at);
            d.process_range(&bits, at, n, &mut parts);
            at += n;
        }
        d.process_range(&bits, at, full.len() - at, &mut parts);
        assert_eq!(parts, full, "任意の分割で全体変換とビット単位で一致(シーク・逐次再生の前提)");
    }

    #[test]
    fn rejects_rates_that_do_not_divide_or_are_not_byte_aligned() {
        let bits = vec![0x55u8; 1024];
        assert!(decimate_channel(&bits, 2_822_400, DsdToPcm { out_rate_hz: 100_000, cutoff_hz: 40_000.0 }).is_err());
        assert!(decimate_channel(&bits, 2_822_400, DsdToPcm { out_rate_hz: 2_822_400 / 4, cutoff_hz: 40_000.0 }).is_err());
    }

    fn write_dsf(channels: &[Vec<u8>], rate: u32, sample_bits: u64) -> Vec<u8> {
        // 入力はMSBファースト。DSFはLSBファーストなので反転して書く。
        let block = 4096usize;
        let ch = channels.len();
        let groups = channels[0].len().div_ceil(block);
        let data_len = groups * block * ch;
        let mut v = Vec::new();
        v.extend_from_slice(b"DSD ");
        v.extend_from_slice(&28u64.to_le_bytes());
        v.extend_from_slice(&((92 + data_len) as u64).to_le_bytes());
        v.extend_from_slice(&0u64.to_le_bytes());
        v.extend_from_slice(b"fmt ");
        v.extend_from_slice(&52u64.to_le_bytes());
        for x in [1u32, 0, 2, ch as u32, rate, 1] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v.extend_from_slice(&sample_bits.to_le_bytes());
        v.extend_from_slice(&(block as u32).to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&((12 + data_len) as u64).to_le_bytes());
        for g in 0..groups {
            for c in channels {
                let mut b = vec![0u8; block];
                let s = g * block;
                let e = c.len().min(s + block);
                if s < e {
                    for (i, x) in c[s..e].iter().enumerate() {
                        b[i] = x.reverse_bits();
                    }
                }
                v.extend_from_slice(&b);
            }
        }
        v
    }

    fn write_dff(channels: &[Vec<u8>], rate: u32) -> Vec<u8> {
        let ch = channels.len();
        let n = channels[0].len();
        let mut prop = Vec::new();
        prop.extend_from_slice(b"SND ");
        prop.extend_from_slice(b"FS  ");
        prop.extend_from_slice(&4u64.to_be_bytes());
        prop.extend_from_slice(&rate.to_be_bytes());
        prop.extend_from_slice(b"CHNL");
        prop.extend_from_slice(&(2 + 4 * ch as u64).to_be_bytes());
        prop.extend_from_slice(&(ch as u16).to_be_bytes());
        for _ in 0..ch {
            prop.extend_from_slice(b"SLFT");
        }
        prop.extend_from_slice(b"CMPR");
        prop.extend_from_slice(&(4 + 1 + 15u64).to_be_bytes());
        prop.extend_from_slice(b"DSD ");
        prop.push(14);
        prop.extend_from_slice(b"not compressed\0");
        let mut body = Vec::new();
        body.extend_from_slice(b"DSD ");
        body.extend_from_slice(b"PROP");
        body.extend_from_slice(&(prop.len() as u64).to_be_bytes());
        body.extend_from_slice(&prop);
        body.extend_from_slice(b"DSD ");
        body.extend_from_slice(&((n * ch) as u64).to_be_bytes());
        for i in 0..n {
            for c in channels {
                body.push(c[i]);
            }
        }
        let mut v = Vec::new();
        v.extend_from_slice(b"FRM8");
        v.extend_from_slice(&(body.len() as u64).to_be_bytes());
        v.extend_from_slice(&body);
        v
    }

    #[test]
    fn dsf_and_dff_round_trip_to_the_same_time_ordered_bits() {
        let l = modulate_sine(1000.0, 0.4, 2_822_400, 0.02);
        let r = modulate_sine(2000.0, 0.3, 2_822_400, 0.02);
        let sbits = l.len() as u64 * 8;
        let dsf = parse_dsf(&write_dsf(&[l.clone(), r.clone()], 2_822_400, sbits)).unwrap();
        assert_eq!((dsf.rate_hz, dsf.channels.len(), dsf.sample_bits), (2_822_400, 2, sbits));
        assert_eq!(dsf.channels, vec![l.clone(), r.clone()], "DSFのLSBファースト→MSBファーストとブロック解除が正しい");
        let dff = parse_dff(&write_dff(&[l.clone(), r.clone()], 2_822_400)).unwrap();
        assert_eq!(dff.channels, vec![l, r]);
        assert_eq!(dff.multiplier(), 64);
    }

    #[test]
    fn dst_compressed_dsdiff_is_refused_explicitly() {
        let mut v = write_dff(&[vec![0x55; 64]], 2_822_400);
        let pos = v.windows(4).position(|w| w == b"CMPR").unwrap() + 12;
        v[pos..pos + 4].copy_from_slice(b"DST ");
        assert!(matches!(parse_dff(&v), Err(DsdError::Unsupported(_))));
    }

    #[test]
    fn dop_frames_carry_the_dsd_bytes_with_alternating_markers() {
        let s = DsdStream { rate_hz: 2_822_400, channels: vec![vec![0xAA, 0x55, 0x12, 0x34]], sample_bits: 32 };
        let (f, rate) = to_dop(&s).unwrap();
        assert_eq!(rate, 176_400);
        assert_eq!(f[0], vec![[0x05, 0xAA, 0x55], [0xFA, 0x12, 0x34]]);
    }

    #[test]
    fn real_dsf_from_make_disk_decodes_to_audible_pcm_when_available() {
        let path = "F:/tmp/cd_dsd/Track04.dsf";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let s = read_dsd_file(path).unwrap();
        assert_eq!((s.rate_hz, s.channels.len()), (11_289_600, 2));
        // 先頭30秒だけ変換して音量を確認
        let cut = DsdStream { rate_hz: s.rate_hz, channels: s.channels.iter().map(|c| c[..c.len().min(11_289_600 / 8 * 30)].to_vec()).collect(), sample_bits: 0 };
        let (pcm, rate) = dsd_to_pcm(&cut, DsdToPcm::default_for(cut.rate_hz)).unwrap();
        let rms = (pcm.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt();
        eprintln!("DSD256→{rate}Hz、30秒、RMS={rms:.4}");
        assert_eq!(rate, 705_600);
        assert!(rms > 0.01 && rms < 0.7, "実音源らしい音量: {rms}");
    }
}
