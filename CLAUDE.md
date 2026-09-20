# open-mqa-dsd 開発メモ

- 開発方針・ルールの正本は [`open-raid-z/CLAUDE.md`](https://github.com/aon-co-jp/open-raid-z)。
- MQAは互換も再実装もしない(理由はREADME)。DSDは純Rustの読み込み/変換/DoPを担当し、再生は`open-bar`、変換・書き込みは`make-disk`が担当する。
- 2026-09-20: 初版。`src/dsd.rs`(DSF/DFF・間引き・DoP)。`cargo test`は6件。DSD→PCMは実DSD256(make-disk製)で確認。次: ΔΣ変調器の移植、DSF/DFF書き出し、open-cpuでの間引き高速化。
