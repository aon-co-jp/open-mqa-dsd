//! open-mqa-dsd: `open-mqa`のDSD版の相棒。DSF/DSDIFFの読み込み、DSD→PCM変換(DSD非対応ハードウェア向け)、
//! DoP(DSD over PCM)パッキングを提供する。**MQA互換ではない**(MQAの再実装はしない。`open-mqa`のREADME参照)。
pub mod dsd;
pub use dsd::*;
