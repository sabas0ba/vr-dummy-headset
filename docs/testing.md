# 試験方針

## 単体試験

### ドライバ（C++，doctest）

- `protocol`: ゴールデンデータ（`tests/fixtures/`）の復号と再符号化の一致，不正フレームの
  拒否，固定レイアウトサイズの `static_assert`．
- `pose_convert`: プロトコルのポーズから `vr::DriverPose_t` への変換．
- OpenVR の抽象クラス（`IVRServerDriverHost`，`IVRDriverInput`，`IVRProperties`）はフェイク
  実装を用意し，デバイス登録・ポーズ更新・入力更新の呼び出しを検証する（今後追加）．

実行: `cmake --preset msvc-x64 && cmake --build --preset msvc-x64 && ctest --preset msvc-x64`

### フィーダ（Rust）

- `protocol`: レイアウトサイズ，往復符号化，ゴールデンデータ生成と照合．
- `core`: コマンド解釈，四元数演算，姿勢プリセット，OSC 符号化（バイト列），設定の既定値，
  JSON Lines 整形，シナリオ解釈，記録ファイルの再生可能性，制御パイプの往復（Windows）．
- `cli`: 引数解釈．

実行: `cargo test --workspace`（`feeder/` 内）

ゴールデンデータの更新: `VRDUMMY_UPDATE_FIXTURES=1 cargo test -p vrdummy-protocol`

初版のゴールデンデータは仕様書から独立に書いた使い捨ての Python スクリプトで生成し，
Rust と C++ の双方がそれに一致することを確認した．以後の更新は Rust 側で行う．

## 統合試験（SteamVR 実機，手動）

CI では実行しない．`scripts/integration/README.md` の手順に従う．

1. `scripts/install_driver.ps1` でドライバを登録し SteamVR を起動する．
2. `vrdummy-cli scenario <file>` でシナリオを流す．
3. OpenVR クライアントツール（`openvr_api` をリンクした小さな C++ プログラム，今後追加）で
   各デバイスのポーズを読み取り，期待値と比較する．

## CI（GitHub Actions，windows-latest）

- ドライバ: CMake 構成・ビルド・ctest．
- フィーダ: `cargo fmt --check`，`cargo clippy -D warnings`，`cargo test`，`cargo deny check`．
- 成果物: ドライバフォルダとフィーダ実行ファイル．
- Actions はコミット SHA で固定する．
