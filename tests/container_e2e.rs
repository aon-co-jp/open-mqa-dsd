//! 実ffmpegでの往復テスト: 映像(MP4)+DSD(DSF)+互換音声を1つのMKVにまとめ、構成の確認・取り出し・一致検証を行う。
//! ffmpeg/ffprobeが無い環境ではスキップする。

use open_mqa_dsd::container::{AudioTrack, Layout, Manifest, Role, Sync, TrackKind};
use open_mqa_dsd::container::{extract, inspect, pack};
use std::path::PathBuf;
use std::process::Command;

fn ffmpeg_ok() -> bool {
    Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false) && Command::new("ffprobe").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("open_mqa_dsd_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 最小のDSF(2ch DSD64、`blocks`ブロック分のパターンデータ)。中身は擬似乱数なので、往復一致の検証に向く。
fn fake_dsf(blocks: usize) -> Vec<u8> {
    let block = 4096usize;
    let data_len = blocks * block * 2;
    let mut v = Vec::new();
    v.extend_from_slice(b"DSD ");
    v.extend_from_slice(&28u64.to_le_bytes());
    v.extend_from_slice(&((92 + data_len) as u64).to_le_bytes());
    v.extend_from_slice(&0u64.to_le_bytes());
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&52u64.to_le_bytes());
    for x in [1u32, 0, 2, 2, 2_822_400, 1] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v.extend_from_slice(&((blocks * block * 8) as u64).to_le_bytes());
    v.extend_from_slice(&(block as u32).to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&((12 + data_len) as u64).to_le_bytes());
    let mut x = 0x9E3779B97F4A7C15u64;
    for _ in 0..data_len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.push((x >> 24) as u8);
    }
    v
}

fn manifest() -> Manifest {
    Manifest {
        format: "open-av".into(),
        version: "0.1".into(),
        title: Some("e2e".into()),
        video: None,
        audio_tracks: vec![
            AudioTrack { id: "dsd".into(), kind: TrackKind::Dsd, role: Role::Main, rate_hz: Some(2_822_400), channels: 2, layout: Layout::Preset("stereo".into()), language: Some("jpn".into()), title: Some("DSD64".into()), file: Some("main.dsf".into()), stream_index: None, codec: None, decode: None, note: None },
            AudioTrack { id: "fb".into(), kind: TrackKind::Pcm, role: Role::Fallback, rate_hz: Some(48_000), channels: 2, layout: Layout::Preset("stereo".into()), language: None, title: None, file: None, stream_index: Some(0), codec: None, decode: None, note: None },
        ],
        sync: Sync::default(),
        licensing: None,
    }
}

#[test]
fn pack_inspect_extract_round_trip_keeps_the_dsd_bytes_exact_and_the_video_playable() {
    if !ffmpeg_ok() {
        return;
    }
    let dir = tmpdir("e2e");
    let video = dir.join("v.mp4");
    let out = Command::new("ffmpeg").args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=duration=2:size=160x120:rate=10", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", video.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let dsf_bytes = fake_dsf(64); // 約512KB
    let dsf = dir.join("main.dsf");
    std::fs::write(&dsf, &dsf_bytes).unwrap();
    let mkv = dir.join("out.mkv");
    pack(&video, &manifest(), &[dsf.clone()], &mkv).expect("pack should succeed");

    let info = inspect(&mkv).unwrap();
    eprintln!("映像{} 音声{} 添付{:?}", info.video_streams, info.audio_streams, info.attachments);
    assert_eq!((info.video_streams, info.audio_streams), (1, 1), "映像と互換音声はそのまま残る(非対応プレーヤーでも再生できる)");
    assert!(info.attachments.iter().any(|a| a.filename == "main.dsf" && a.mimetype == "audio/x-dsf"));
    assert_eq!(info.manifest.as_ref().unwrap(), &manifest(), "同梱のマニフェストが読み戻せる");

    let outdir = dir.join("x");
    let files = extract(&mkv, &outdir).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(std::fs::read(outdir.join("main.dsf")).unwrap(), dsf_bytes, "DSDファイルはビット単位で一致");

    // 通常のプレーヤー(ffmpeg)は、添付を無視して映像+音声を普通にデコードできる
    let dec = Command::new("ffmpeg").args(["-v", "error", "-i", mkv.to_str().unwrap(), "-f", "null", "-"]).output().unwrap();
    assert!(dec.status.success(), "{}", String::from_utf8_lossy(&dec.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pack_rejects_missing_assets_bad_manifests_and_non_mkv_outputs() {
    let dir = tmpdir("reject");
    let video = dir.join("v.mp4");
    std::fs::write(&video, b"x").unwrap();
    let m = manifest();
    // assetsに無い
    assert!(pack(&video, &m, &[], &dir.join("o.mkv")).is_err());
    // .mkv以外
    let dsf = dir.join("main.dsf");
    std::fs::write(&dsf, b"x").unwrap();
    assert!(pack(&video, &m, &[dsf.clone()], &dir.join("o.mp4")).is_err());
    // 不正なマニフェスト(主音声が無い)
    let mut bad = manifest();
    bad.audio_tracks[0].role = Role::Alternate;
    assert!(pack(&video, &bad, &[dsf], &dir.join("o.mkv")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn open_mqa_dsd_audio_only_package_round_trips_dsd_exactly() {
    if !ffmpeg_ok() {
        return;
    }
    let dir = tmpdir("open_mqa_dsd_audio");
    let fallback = dir.join("fb.flac");
    let out = Command::new("ffmpeg").args(["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=2", "-c:a", "flac", fallback.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let dsf_bytes = fake_dsf(32);
    let dsf = dir.join("main.dsf");
    std::fs::write(&dsf, &dsf_bytes).unwrap();
    let mut m = manifest();
    m.format = "open-mqa-dsd".into();
    let mka = dir.join("out.mka");
    pack(&fallback, &m, &[dsf.clone()], &mka).expect("open-mqa-dsd pack should succeed");
    let info = inspect(&mka).unwrap();
    assert_eq!((info.video_streams, info.audio_streams), (0, 1), "音声だけ(映像なし)");
    assert!(info.attachments.iter().any(|a| a.filename == "open-mqa-dsd.json"));
    assert_eq!(info.manifest.as_ref().unwrap().format, "open-mqa-dsd");
    let outdir = dir.join("x");
    extract(&mka, &outdir).unwrap();
    assert_eq!(std::fs::read(outdir.join("main.dsf")).unwrap(), dsf_bytes, "DSDはビット単位で一致");
    // 通常のプレーヤーは、互換のFLACとして普通に再生できる
    let dec = Command::new("ffmpeg").args(["-v", "error", "-i", mka.to_str().unwrap(), "-f", "null", "-"]).output().unwrap();
    assert!(dec.status.success());
    // open-mqa-dsd形式に.mp4は不可
    assert!(pack(&fallback, &m, &[dsf], &dir.join("o.mp4")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
