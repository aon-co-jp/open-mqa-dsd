//! open-mqa-dsdの音声専用コンテナプロファイル(旧称「open-audio」、2026-09-26に`open-av`から
//! 移設した際に廃止・このクレート名へ統一)。
//! WAV/FLAC等の互換音声+DSD(DSF/DSDIFF)添付+マニフェスト(チャンネル配置・ライセンス・
//! プレビュー情報)を1つのMatroska(`.mka`)にまとめる。映像を持つ`open-av`プロファイル
//! (`.mkv`)も同じ実装(`pack`/`inspect`/`extract`)を共有しており、`open-av`クレートは
//! これに依存して映像プロファイル専用のCLI・ドキュメントを提供する側になった。
//!
//! **MQA・Auro-CXについて**: どちらも特許・非公開の技術で、本仕様は復号も再実装もしない。
//! すでにその形式でエンコード済みの音声を`kind = "opaque"`・`decode = "none"`として
//! **中身に触れずに運ぶ**ことだけを定める(対応する認定デコーダ/ハードウェアが再生する)。

mod manifest;
mod pack;
mod preview;

pub use manifest::{preset_speakers, AudioTrack, Layout, Licensing, Manifest, ManifestError, Role, Sync, TrackKind, Video, FORMAT_AUDIO, FORMAT_NAME, VERSION};
pub use pack::{extract, inspect, pack, Attachment, PackError, PackageInfo};
pub use preview::{make_preview, PreviewError, DEFAULT_FADE_SECS, DEFAULT_PREVIEW_SECS};
