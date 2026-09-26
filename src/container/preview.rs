//! 試聴用の短縮版(プレビュー)自動生成。
//!
//! 全体を配信・販売する前提の音源から、冒頭N秒(既定30秒)を切り出し、末尾に短いフェードアウトを
//! 付けた試聴用ファイルを作る。これは業界で広く使われる「30秒プレビュー」的な慣行に沿ったもので、
//! **ライセンス許諾が不要になるわけではない**(元音源の配信・販売そのものには別途契約が必要——
//! `Licensing`〈manifest.rs〉参照)。ffmpeg実行が前提(open-av::pack等と同じ`OPEN_AV_FFMPEG`で差し替え可能)。

use std::path::Path;
use std::process::Command;
use thiserror::Error;

pub const DEFAULT_PREVIEW_SECS: f64 = 30.0;
pub const DEFAULT_FADE_SECS: f64 = 2.0;

#[derive(Debug, Error)]
pub enum PreviewError {
    #[error("ffmpegを実行できません: {0}")]
    Spawn(String),
    #[error("ffmpegが失敗しました: {0}")]
    Ffmpeg(String),
}

fn tool(name: &str, env: &str) -> Command {
    Command::new(std::env::var(env).unwrap_or_else(|_| name.to_string()))
}

/// `source`の冒頭`preview_secs`秒を切り出し、末尾`fade_secs`秒をフェードアウトして`out`へ書き出す。
/// 音声コーデック・コンテナは`out`の拡張子からffmpegが判断する(例: `.flac`/`.wav`/`.mp3`)。
pub fn make_preview(source: &Path, out: &Path, preview_secs: f64, fade_secs: f64) -> Result<(), PreviewError> {
    let fade_start = (preview_secs - fade_secs).max(0.0);
    let mut cmd = tool("ffmpeg", "OPEN_AV_FFMPEG");
    cmd.args(["-y", "-v", "error", "-i"])
        .arg(source)
        .args(["-t", &preview_secs.to_string(), "-af", &format!("afade=t=out:st={fade_start}:d={fade_secs}")])
        .arg(out);
    let result = cmd.output().map_err(|e| PreviewError::Spawn(e.to_string()))?;
    if !result.status.success() {
        return Err(PreviewError::Ffmpeg(String::from_utf8_lossy(&result.stderr).to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ffmpeg_ok() -> bool {
        Command::new(std::env::var("OPEN_AV_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string())).arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
    }
    fn ffprobe_duration(path: &Path) -> f64 {
        let out = Command::new(std::env::var("OPEN_AV_FFPROBE").unwrap_or_else(|_| "ffprobe".to_string()))
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    #[test]
    fn preview_is_trimmed_to_the_requested_length() {
        if !ffmpeg_ok() {
            eprintln!("ffmpegが無いためスキップ");
            return;
        }
        let dir = std::env::temp_dir().join(format!("open_av_preview_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.wav");
        let out = dir.join("preview.flac");
        // 10秒の1kHzトーンを用意(実ffmpegで生成)
        Command::new(std::env::var("OPEN_AV_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string()))
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=44100:duration=10"])
            .arg(&src)
            .output()
            .unwrap();

        make_preview(&src, &out, 3.0, 1.0).unwrap();
        let dur = ffprobe_duration(&out);
        assert!((dur - 3.0).abs() < 0.2, "プレビューは指定秒数に切り詰められるはず: {dur}");

        let _ = PathBuf::from(&dir); // clippy対策(未使用警告回避、実際は下でcleanup)
        let _ = std::fs::remove_dir_all(&dir);
    }
}
