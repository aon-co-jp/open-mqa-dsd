//! MKV単一ファイルへのパッケージング(ffmpeg/ffprobe使用)。
//!
//! 映像・コンテナ内の音声(互換用)はそのままコピーし、DSDなどの追加音声とマニフェスト(`open-av.json`)を
//! **Matroskaの添付ファイル**として同梱する。open-av非対応のプレーヤーは映像+互換音声として普通に再生でき、
//! 対応プレーヤーは添付のDSDを主音声として鳴らす。ffmpegは環境変数`OPEN_AV_FFMPEG`/`OPEN_AV_FFPROBE`で差し替えられる。

use super::manifest::{Manifest, ManifestError};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// 映像ありパッケージのマニフェスト添付名(音声だけの`open-mqa-dsd`形式は`open-mqa-dsd.json`、`Manifest::attachment_name`)。
pub const MANIFEST_NAME: &str = "open-av.json";
pub const MANIFEST_NAME_AUDIO: &str = "open-mqa-dsd.json";

#[derive(Debug, Error)]
pub enum PackError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("マニフェストのJSONが不正です: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("ffmpeg/ffprobeを実行できません: {0}")]
    Spawn(String),
    #[error("ffmpegが失敗しました: {0}")]
    Ffmpeg(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn tool(name: &str, env: &str) -> Command {
    Command::new(std::env::var(env).unwrap_or_else(|_| name.to_string()))
}

/// プロセス内で一意な一時名(並列実行のテストや複数呼び出しで衝突しないように、通し番号を付ける)。
fn unique(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!("open_av_{tag}_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

fn mime_for(name: &str) -> &'static str {
    match Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "dsf" => "audio/x-dsf",
        "dff" => "audio/x-dsdiff",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn base_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// 基になるファイル(open-avなら映像、open-mqa-dsd形式なら互換用の音声)とアセット(DSD等)を、マニフェスト付きの1つのMatroskaにまとめる。
/// open-av → `.mkv`、open-mqa-dsd形式(音声だけ) → `.mka`(または`.mkv`)。マニフェストの`file`が指す各ファイルは、`assets`の中に同じ名前で存在しなければならない。
pub fn pack(base: &Path, manifest: &Manifest, assets: &[PathBuf], output: &Path) -> Result<(), PackError> {
    manifest.validate()?;
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let ok_ext = if manifest.is_audio_only() { ext == "mka" || ext == "mkv" } else { ext == "mkv" };
    if !ok_ext {
        let want = if manifest.is_audio_only() { ".mka(または.mkv)" } else { ".mkv" };
        return Err(PackError::Invalid(format!("出力は{want}にしてください(添付ファイルを持てるコンテナ)")));
    }
    let video = base;
    for t in &manifest.audio_tracks {
        if let Some(f) = &t.file {
            if !assets.iter().any(|a| base_name(a) == *f) {
                return Err(PackError::Invalid(format!("トラック{}のファイル{f}がassetsにありません", t.id)));
            }
        }
    }
    let tmp = std::env::temp_dir().join(unique("manifest")).with_extension("json");
    std::fs::write(&tmp, manifest.to_json())?;
    let mut cmd = tool("ffmpeg", "OPEN_AV_FFMPEG");
    cmd.args(["-y", "-v", "error", "-i"]).arg(video).args(["-map", "0", "-c", "copy"]);
    // 添付は「使うものだけ」: マニフェストが参照するアセット + マニフェスト本体
    let mut used: Vec<&PathBuf> = Vec::new();
    for t in &manifest.audio_tracks {
        if let Some(f) = &t.file {
            if let Some(a) = assets.iter().find(|a| base_name(a) == *f) {
                if !used.contains(&a) {
                    used.push(a);
                }
            }
        }
    }
    for (i, a) in used.iter().enumerate() {
        let name = base_name(a);
        cmd.arg("-attach").arg(a).arg(format!("-metadata:s:t:{i}")).arg(format!("mimetype={}", mime_for(&name))).arg(format!("-metadata:s:t:{i}")).arg(format!("filename={name}"));
    }
    let mi = used.len();
    cmd.arg("-attach").arg(&tmp).arg(format!("-metadata:s:t:{mi}")).arg("mimetype=application/json").arg(format!("-metadata:s:t:{mi}")).arg(format!("filename={}", manifest.attachment_name()));
    cmd.arg(output);
    let out = cmd.output().map_err(|e| PackError::Spawn(e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        return Err(PackError::Ffmpeg(String::from_utf8_lossy(&out.stderr).to_string()));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// 添付ストリームの中での番号(`-dump_attachment:t:N`のN)。
    pub index: usize,
    pub filename: String,
    pub mimetype: String,
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub attachments: Vec<Attachment>,
    pub video_streams: usize,
    pub audio_streams: usize,
    /// 同梱のマニフェスト(あれば)。
    pub manifest: Option<Manifest>,
}

#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
}

#[derive(Deserialize)]
struct ProbeStream {
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

/// MKVの構成(映像/音声ストリーム数・添付)を調べ、同梱のマニフェストを読む。
pub fn inspect(path: &Path) -> Result<PackageInfo, PackError> {
    let out = tool("ffprobe", "OPEN_AV_FFPROBE").args(["-v", "error", "-show_streams", "-of", "json"]).arg(path).output().map_err(|e| PackError::Spawn(e.to_string()))?;
    if !out.status.success() {
        return Err(PackError::Ffmpeg(String::from_utf8_lossy(&out.stderr).to_string()));
    }
    let probe: ProbeOut = serde_json::from_slice(&out.stdout)?;
    let mut attachments = Vec::new();
    let (mut video, mut audio) = (0, 0);
    for s in &probe.streams {
        match s.codec_type.as_str() {
            "video" => video += 1,
            "audio" => audio += 1,
            "attachment" => {
                let get = |k: &str| s.tags.iter().find(|(kk, _)| kk.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()).unwrap_or_default();
                attachments.push(Attachment { index: attachments.len(), filename: get("filename"), mimetype: get("mimetype") });
            }
            _ => {}
        }
    }
    let manifest = match attachments.iter().find(|a| a.filename == MANIFEST_NAME || a.filename == MANIFEST_NAME_AUDIO) {
        Some(a) => {
            let dir = std::env::temp_dir().join(unique("inspect"));
            std::fs::create_dir_all(&dir)?;
            let target = dir.join(&a.filename);
            dump(path, a.index, &target)?;
            let text = std::fs::read_to_string(&target)?;
            let _ = std::fs::remove_dir_all(&dir);
            Some(Manifest::from_json(&text)?)
        }
        None => None,
    };
    Ok(PackageInfo { attachments, video_streams: video, audio_streams: audio, manifest })
}

/// N番目の添付ファイルを`target`へ書き出す。
fn dump(path: &Path, index: usize, target: &Path) -> Result<(), PackError> {
    // ffmpegは出力ファイルが無いためエラー終了するが、-dump_attachmentは実行される(公式の使い方)
    let _ = tool("ffmpeg", "OPEN_AV_FFMPEG").args(["-y", "-v", "error"]).arg(format!("-dump_attachment:t:{index}")).arg(target).arg("-i").arg(path).output().map_err(|e| PackError::Spawn(e.to_string()))?;
    if target.exists() {
        Ok(())
    } else {
        Err(PackError::Ffmpeg(format!("添付{index}を取り出せませんでした")))
    }
}

/// すべての添付(DSD・マニフェスト等)を`out_dir`へ取り出し、書き出したパスを返す。
pub fn extract(path: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, PackError> {
    std::fs::create_dir_all(out_dir)?;
    let info = inspect(path)?;
    let mut v = Vec::new();
    for a in &info.attachments {
        if a.filename.is_empty() || a.filename.contains(['/', '\\']) {
            return Err(PackError::Invalid(format!("添付名が不正です: {}", a.filename)));
        }
        let target = out_dir.join(&a.filename);
        dump(path, a.index, &target)?;
        v.push(target);
    }
    Ok(v)
}
