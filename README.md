# open-mqa-dsd

[English](#english) / 日本語

`open-mqa` の **DSD版の相棒**。DSF / DSDIFF(DFF)の読み込み、DSD→PCM変換(DSD非対応のハードウェア向けの自動PCM化)、DoP(DSD over PCM)パッキングを、純Rustで提供する。

## MQAについて(正直な開示)

**MQA互換ではありません。MQAの再実装もしません。** MQAのエンコード/デコード(折り紙)は特許と営業秘密で保護されたロスレスではない非公開技術です。本プロジェクトは「MQAが目指した、配信帯域に収まる高解像度体験」を、公開規格(WAV/FLAC/DSD/DoP)で実現する独自の道具箱です。MQA対応DACでMQAファイルを鳴らすには、ソフトは**ビットパーフェクトのまま素通し**すればよく(展開はDAC側)、その判断は再生ソフト`open-bar`が担います。

## 機能(2026-09-20、初版)

- `parse_dsf` / `parse_dff` / `read_dsd_file`: DSF(LSBファースト・ブロック交互)とDSDIFF(MSBファースト)を、内部で「チャンネルごとの時間順MSBファーストのバイト列」に統一。**DST圧縮のDSDIFFは未対応**(明示エラー)。
- `dsd_to_pcm` / `decimate_channel`: Kaiser窓sincのFIR間引き(±1ビットのため8タップずつ256通りの表引きで高速化)。既定はDSDレート/16(DSD64→176.4kHz、DSD256→705.6kHz)、カットオフ40kHz。
- `to_dop`: DoPフレーム(`open-mqa`のパッキングを利用)。
- テスト6件(実機E2E含む): DSF/DFFのビット列往復一致、1kHz正弦波の振幅(誤差2%以内)、間引き率の検証、DST拒否、DoPマーカー、`make-disk`が作った実DSD256ファイルの変換(RMS確認)。

## 今後(未実装)

- 高次ΔΣ変調器(PCM→DSD)の移植: 現在は`make-disk`の`engine/dsd.rs`(5次、DSD64で99.6dB SNR実測)にあり、ここへ切り出す予定。
- DSF/DFFの書き出し、SIMD(open-cpu)による間引き高速化、GPU(open-cuda)は直列フィードバックのΔΣには向かないため見送り(FIR間引きの並列化のみ候補)。

## 関連

[open-mqa](https://github.com/aon-co-jp/open-mqa) / [open-bar](https://github.com/aon-co-jp/open-bar)(再生ソフト) / [make-disk](https://github.com/aon-co-jp/make-disk)(変換・書き込み)

<a id="english"></a>
## English

The DSD companion of `open-mqa`: pure-Rust DSF / DSDIFF (DFF) reading, DSD→PCM decimation (automatic PCM fallback for hardware without DSD support), and DoP (DSD over PCM) packing.

**Honest disclosure:** this is **not MQA-compatible and does not reimplement MQA** (patented, trade-secret, not lossless). It provides an open pipeline (WAV/FLAC/DSD/DoP) toward the same goal. Playing MQA files on an MQA-capable DAC only requires a bit-perfect pass-through (unfolding happens in the DAC); that decision lives in the player `open-bar`.

Features (first release, 2026-09-20): DSF/DFF parsing into MSB-first time-ordered bytes per channel (DST-compressed DSDIFF is refused explicitly); Kaiser-windowed-sinc FIR decimation with byte-table acceleration (default DSD rate / 16, 40 kHz cutoff); DoP packing; 6 tests including an end-to-end decode of a real DSD256 file made by make-disk. Not yet: PCM→DSD ΔΣ modulator port (currently in make-disk), DSF/DFF writers, SIMD via open-cpu.
