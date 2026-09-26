//! open-avのマニフェスト(`open-av.json`)。映像ファイルと音声トラック群(DSD・PCM・不透明な素通し)の組み合わせ、
//! チャンネル配置(イマーシブを含む)、同期情報を記述する。MKV内の添付ファイルとして同梱するか、外部ファイルとして置く。
//!
//! **MQA・Auro-CXについて**: どちらも特許・非公開の技術で、本仕様は復号も再実装もしない。すでにその形式でエンコード済みの音声を
//! `kind = "opaque"`・`decode = "none"`として**中身に触れずに運ぶ**ことだけを定める(対応する認定デコーダ/ハードウェアが再生する)。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// 映像を含むパッケージ(MKV)の形式名。
pub const FORMAT_NAME: &str = "open-av";
/// 音声だけのパッケージ(MKA)の形式名(open-avの音声部分をそのまま単独で使える形式)。
pub const FORMAT_AUDIO: &str = "open-mqa-dsd";
pub const VERSION: &str = "0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// DSD(DSF/DSDIFF)。MKV内では添付ファイルとして、外部ファイルなら`file`で参照する。
    Dsd,
    /// 通常のPCM/圧縮音声(コンテナ内の音声ストリームを`stream_index`で指す)。
    Pcm,
    /// 中身を解釈しない素通し(MQA-FLAC、Auro-CXでエンコード済みの音声など)。`decode`は`"none"`必須。
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 主音声。open-av対応プレーヤーが最優先で鳴らす。
    Main,
    /// 互換用。open-av非対応のプレーヤーがコンテナ内の音声として鳴らせるもの(通常のPCM/AAC/FLAC/Opus)。
    Fallback,
    Commentary,
    Alternate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    pub id: String,
    pub kind: TrackKind,
    pub role: Role,
    /// DSDならビットレート(例: 11289600)、PCMならサンプルレート。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<u32>,
    pub channels: u32,
    /// チャンネル配置。プリセット名(`stereo`/`5.1`/`7.1`/`9.1-height`/`11.1-height`)か、スピーカー位置名の配列。
    pub layout: Layout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// MKV内の添付ファイル名、または外部ファイルの相対パス。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// コンテナ内の音声ストリーム番号(`kind = "pcm"`のとき)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_index: Option<u32>,
    /// 素通しの符号化名(例: `mqa-flac`、`auro-cx`)。`kind = "opaque"`のとき。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    /// `kind = "opaque"`のとき`"none"`(open-avは復号しない)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Layout {
    Preset(String),
    Speakers(Vec<String>),
}

impl Layout {
    /// スピーカー位置名の一覧。プリセットは展開する(未知のプリセットはNone)。
    pub fn speakers(&self) -> Option<Vec<String>> {
        match self {
            Layout::Speakers(v) => Some(v.clone()),
            Layout::Preset(p) => preset_speakers(p).map(|s| s.iter().map(|x| x.to_string()).collect()),
        }
    }
}

/// プリセットのスピーカー位置(ITU-R BS.2051/ffmpegの一般的な略称。特定社の商標名ではなく位置で表す)。
/// FL/FR=前方左右、FC=前方中央、LFE=低域効果、SL/SR=側方左右、BL/BR=後方左右、TFL/TFR/TSL/TSR/TBL/TBR/TC=高さ方向。
pub fn preset_speakers(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "mono" => &["FC"],
        "stereo" => &["FL", "FR"],
        "5.1" => &["FL", "FR", "FC", "LFE", "SL", "SR"],
        "7.1" => &["FL", "FR", "FC", "LFE", "BL", "BR", "SL", "SR"],
        // 5.1に高さ4本(前方左右・側方左右の上)を足した配置(9.1相当のオープンな表現)
        "9.1-height" => &["FL", "FR", "FC", "LFE", "SL", "SR", "TFL", "TFR", "TSL", "TSR"],
        // 上記にトップ中央を足した配置(11.1相当)
        "11.1-height" => &["FL", "FR", "FC", "LFE", "SL", "SR", "TFL", "TFR", "TSL", "TSR", "TC"],
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Video {
    /// 映像ファイル名(外部ファイルの場合)。MKVに同梱する場合は省略(コンテナ自身の映像ストリームを使う)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sync {
    /// 映像に対する音声の遅れ(ミリ秒、負なら早める)。音声側を時間の基準にして映像を追従させる。
    #[serde(default)]
    pub audio_offset_ms: i64,
}

impl Default for Sync {
    fn default() -> Self {
        Sync { audio_offset_ms: 0 }
    }
}

/// ライセンス種別・購入導線のメタデータ(任意)。
///
/// **これは権利の付与そのものではない。** ここに書いた内容だけで市販音源を配信・販売してよく
/// なるわけではなく、実際にJASRAC/NexTone・レコード会社等と契約したあとで「その契約内容を
/// 機械可読に記録しておく欄」として使う想定(2026-09-26新設、ユーザー依頼)。
/// パブリックドメイン音源(このリポジトリのopen-bar側テストフィクスチャ等)では
/// `license_type = "public-domain"`・`purchase_url = None`でよい(そもそも許諾が不要なため)。
///
/// This is metadata, not a grant of rights. Filling this in does not by itself make it legal
/// to distribute or sell commercial recordings — it's a machine-readable place to record the
/// terms of an *actual* agreement (e.g. with a collecting society or label) once one exists.
/// For public-domain sources, `license_type = "public-domain"` and no `purchase_url` is correct,
/// since no permission is needed in the first place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Licensing {
    /// 例: `"public-domain"`・`"cc0"`・`"cc-by"`・`"promotional-webcast"`・`"licensed-retail"`など。
    /// 自由記述(このcrateは値を検証・強制しない、記録するだけ)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_type: Option<String>,
    /// 許諾の範囲の説明(自由記述)。例: "非インタラクティブ配信のみ、複製・保存不可"。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// 実際に購入できる先(Amazon等の外部ストアURL)。試聴・宣伝から誘導する用途。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchase_url: Option<String>,
    /// 権利者・許諾元(自由記述)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights_holder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<Video>,
    pub audio_tracks: Vec<AudioTrack>,
    #[serde(default)]
    pub sync: Sync,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licensing: Option<Licensing>,
}

#[derive(Debug, Error, PartialEq)]
pub enum ManifestError {
    #[error("formatは\"open-av\"または\"open-mqa-dsd\"である必要があります(実際: {0})")]
    WrongFormat(String),
    #[error("未対応のバージョンです: {0}(対応: {VERSION})")]
    UnsupportedVersion(String),
    #[error("音声トラックがありません")]
    NoAudio,
    #[error("音声だけの形式(open-mqa-dsd)に映像(video)は指定できません")]
    VideoInAudioFormat,
    #[error("トラックIDが重複しています: {0}")]
    DuplicateId(String),
    #[error("主音声(role=main)は1本だけにしてください(実際: {0}本)")]
    MainCount(usize),
    #[error("トラック{id}: {msg}")]
    Track { id: String, msg: String },
}

impl Manifest {
    pub fn from_json(s: &str) -> Result<Manifest, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// 音声だけのopen-mqa-dsd形式か。
    pub fn is_audio_only(&self) -> bool {
        self.format == FORMAT_AUDIO
    }

    /// パッケージに同梱するマニフェストの添付ファイル名(`open-av.json` / `open-mqa-dsd.json`)。
    pub fn attachment_name(&self) -> &'static str {
        if self.is_audio_only() {
            "open-mqa-dsd.json"
        } else {
            "open-av.json"
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Manifest is serializable")
    }

    /// 仕様どおりか検証する。
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.format != FORMAT_NAME && self.format != FORMAT_AUDIO {
            return Err(ManifestError::WrongFormat(self.format.clone()));
        }
        if self.format == FORMAT_AUDIO && self.video.is_some() {
            return Err(ManifestError::VideoInAudioFormat);
        }
        if self.version.split('.').next() != VERSION.split('.').next() {
            return Err(ManifestError::UnsupportedVersion(self.version.clone()));
        }
        if self.audio_tracks.is_empty() {
            return Err(ManifestError::NoAudio);
        }
        let mut ids = HashSet::new();
        for t in &self.audio_tracks {
            if !ids.insert(t.id.clone()) {
                return Err(ManifestError::DuplicateId(t.id.clone()));
            }
            let bad = |msg: &str| ManifestError::Track { id: t.id.clone(), msg: msg.to_string() };
            if t.channels == 0 {
                return Err(bad("channelsは1以上"));
            }
            match t.layout.speakers() {
                None => return Err(bad("未知のlayoutプリセットです")),
                Some(sp) if sp.len() as u32 != t.channels => return Err(bad(&format!("layoutのスピーカー数({})がchannels({})と一致しません", sp.len(), t.channels))),
                Some(sp) => {
                    let mut seen = HashSet::new();
                    if sp.iter().any(|s| !seen.insert(s.clone())) {
                        return Err(bad("layout内でスピーカー位置が重複しています"));
                    }
                }
            }
            match t.kind {
                TrackKind::Dsd => {
                    let rate = t.rate_hz.ok_or_else(|| bad("DSDにはrate_hzが必要"))?;
                    // DSD64=2,822,400の整数倍(44.1k系)または3,072,000の整数倍(48k系)
                    if rate % 2_822_400 != 0 && rate % 3_072_000 != 0 {
                        return Err(bad(&format!("rate_hz {rate} はDSDのレートではありません")));
                    }
                    if t.file.is_none() {
                        return Err(bad("DSDにはfile(添付名または相対パス)が必要"));
                    }
                }
                TrackKind::Pcm => {
                    if t.stream_index.is_none() && t.file.is_none() {
                        return Err(bad("PCMにはstream_indexかfileが必要"));
                    }
                }
                TrackKind::Opaque => {
                    if t.decode.as_deref() != Some("none") {
                        return Err(bad("opaqueはdecode=\"none\"が必須(open-avはMQA/Auro-CX等を復号しません)"));
                    }
                    if t.codec.is_none() {
                        return Err(bad("opaqueにはcodec(例: mqa-flac)が必要"));
                    }
                    if t.file.is_none() && t.stream_index.is_none() {
                        return Err(bad("opaqueにはfileかstream_indexが必要"));
                    }
                }
            }
        }
        let mains = self.audio_tracks.iter().filter(|t| t.role == Role::Main).count();
        if mains != 1 {
            return Err(ManifestError::MainCount(mains));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsd(id: &str) -> AudioTrack {
        AudioTrack { id: id.into(), kind: TrackKind::Dsd, role: Role::Main, rate_hz: Some(11_289_600), channels: 2, layout: Layout::Preset("stereo".into()), language: Some("jpn".into()), title: None, file: Some("track.dsf".into()), stream_index: None, codec: None, decode: None, note: None }
    }

    fn fallback() -> AudioTrack {
        AudioTrack { id: "fb".into(), kind: TrackKind::Pcm, role: Role::Fallback, rate_hz: Some(48_000), channels: 2, layout: Layout::Preset("stereo".into()), language: None, title: None, file: None, stream_index: Some(0), codec: None, decode: None, note: None }
    }

    fn manifest(tracks: Vec<AudioTrack>) -> Manifest {
        Manifest { format: "open-av".into(), version: "0.1".into(), title: Some("t".into()), video: None, audio_tracks: tracks, sync: Sync::default(), licensing: None }
    }

    #[test]
    fn a_dsd_main_track_with_a_pcm_fallback_is_valid_and_round_trips_json() {
        let m = manifest(vec![dsd("dsd"), fallback()]);
        assert_eq!(m.validate(), Ok(()));
        assert_eq!(Manifest::from_json(&m.to_json()).unwrap(), m);
    }

    #[test]
    fn layouts_are_checked_against_the_channel_count() {
        let mut t = dsd("d");
        t.channels = 10;
        t.layout = Layout::Preset("9.1-height".into());
        assert_eq!(manifest(vec![t.clone()]).validate(), Ok(()), "9.1-heightは10ch(5.1+高さ4)");
        t.channels = 6;
        assert!(matches!(manifest(vec![t]).validate(), Err(ManifestError::Track { .. })));
        let mut u = dsd("d");
        u.channels = 3;
        u.layout = Layout::Speakers(vec!["FL".into(), "FR".into(), "FL".into()]);
        assert!(matches!(manifest(vec![u]).validate(), Err(ManifestError::Track { .. })), "位置の重複");
        assert_eq!(preset_speakers("11.1-height").unwrap().len(), 11);
        assert!(preset_speakers("nope").is_none());
    }

    #[test]
    fn opaque_tracks_must_declare_no_decoding() {
        let mqa = AudioTrack { id: "mqa".into(), kind: TrackKind::Opaque, role: Role::Alternate, rate_hz: Some(44_100), channels: 2, layout: Layout::Preset("stereo".into()), language: None, title: None, file: Some("song.mqa.flac".into()), stream_index: None, codec: Some("mqa-flac".into()), decode: Some("none".into()), note: Some("MQA対応DACへ素通し".into()) };
        assert_eq!(manifest(vec![dsd("d"), mqa.clone()]).validate(), Ok(()));
        let mut bad = mqa;
        bad.decode = None;
        assert!(matches!(manifest(vec![dsd("d"), bad]).validate(), Err(ManifestError::Track { .. })), "復号を許さない宣言が無いopaqueは不正");
    }

    #[test]
    fn structural_rules_are_enforced() {
        let mut m = manifest(vec![dsd("a"), dsd("a")]);
        assert_eq!(m.validate(), Err(ManifestError::DuplicateId("a".into())));
        m = manifest(vec![dsd("a"), { let mut t = dsd("b"); t.role = Role::Main; t }]);
        assert_eq!(m.validate(), Err(ManifestError::MainCount(2)));
        m = manifest(vec![]);
        assert_eq!(m.validate(), Err(ManifestError::NoAudio));
        m = manifest(vec![dsd("a")]);
        m.format = "other".into();
        assert!(matches!(m.validate(), Err(ManifestError::WrongFormat(_))));
        m.format = "open-av".into();
        m.version = "9.0".into();
        assert!(matches!(m.validate(), Err(ManifestError::UnsupportedVersion(_))));
        let mut t = dsd("a");
        t.rate_hz = Some(44_100);
        assert!(matches!(manifest(vec![t]).validate(), Err(ManifestError::Track { .. })), "PCMのレートはDSDとして不正");
    }

    #[test]
    fn open_mqa_dsd_format_is_the_audio_only_profile_and_rejects_video() {
        let mut m = manifest(vec![dsd("d"), fallback()]);
        m.format = "open-mqa-dsd".into();
        assert_eq!(m.validate(), Ok(()));
        assert!(m.is_audio_only());
        assert_eq!(m.attachment_name(), "open-mqa-dsd.json");
        m.video = Some(Video { file: Some("v.mp4".into()), stream_index: None });
        assert_eq!(m.validate(), Err(ManifestError::VideoInAudioFormat));
        m.format = "open-av".into();
        assert_eq!(m.validate(), Ok(()), "open-avは映像を持てる");
        assert_eq!(m.attachment_name(), "open-av.json");
    }

    #[test]
    fn the_shipped_open_mqa_dsd_example_validates() {
        let m = Manifest::from_json(include_str!("../../examples/example.open-mqa-dsd.json")).unwrap();
        assert_eq!(m.validate(), Ok(()));
        assert!(m.is_audio_only());
    }

    #[test]
    fn the_shipped_example_manifest_validates() {
        let m = Manifest::from_json(include_str!("../../examples/example.open-av.json")).unwrap();
        assert_eq!(m.validate(), Ok(()));
        assert!(m.audio_tracks.iter().any(|t| t.kind == TrackKind::Dsd));
        assert!(m.audio_tracks.iter().any(|t| t.kind == TrackKind::Opaque));
    }
}
