# 試験方針

## 単体試験

### ドライバ（C++，doctest）

- `protocol`: ゴールデンデータ（`tests/fixtures/`）の復号と再符号化の一致，不正フレームの
  拒否，固定レイアウトサイズの `static_assert`．
- `pose_convert`: プロトコルのポーズから `vr::DriverPose_t` への変換．
- `tests/host/`（`vrdummy_host_test`）: ビルド済み DLL を vrserver と同じ手順でロードし，
  OpenVR インターフェースのフェイクを注入して，デバイス登録・プロパティ・入力コンポーネント・
  ポーズ送出スレッド・実パイプ経由のフィーダセッション（ハンドシェイク，ログ転送，ポーズ・
  入力・表示パラメータの反映，切断，再接続）を検証する．SteamVR は不要で CI で実行する．

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

## 統合試験

SteamVR 非依存の結合試験は 2 箇所にある．

- ドライバ側: `vrdummy_host_test`（上記）．実 DLL + フェイク OpenVR + 実パイプ．
- フィーダ側: `feeder/crates/core/tests/driver_pipe.rs`．モックドライバ + 実パイプ．

## SteamVR 実機での E2E（手動，CI 外）

`scripts/integration/steamvr_e2e.ps1` が登録・起動・フィーダ常駐・E2E 実行・後始末を行う．

- `vrdummy_e2e`（`driver/tools/e2e/`）: OpenVR の Scene アプリ．自前のアクションマニフェストと
  `vrdummy_controller` 用バインディングを読み込み，制御パイプでフィーダを操作し，OpenVR 側の
  観測値を doctest のシナリオ S1〜S7 で検証する．フレームを描画・提出するため，コンポジタは
  本アプリを前面のシーンアプリとして扱い，入力が届く（描画しないと dashboard が開き入力が
  奪われる）．起動時に standing / seated 空間を raw 空間に一致させる（`docs/coordinates.md`）．
- `vrdummy_viewer`（`driver/tools/vr/`）: 同じ描画で全デバイスと入力を HMD 内に表示し，
  コンソールにも位置・オイラー角・入力値を出す目視確認用アプリ．
- `vrdummy_probe`: 列挙とポーズ読み取りの小型クライアント．

| シナリオ | 内容 |
|---|---|
| S1 | デバイス列挙，クラス，左右の役割 |
| S2 | 全デバイスの静的な位置とヨー |
| S3 | HMD の連続移動（単調増加と到達） |
| S4 | ボタン，トリガ，グリップ，スティックが SteamVR Input のアクションに到達 |
| S5 | IPD 変更が眼の変換に反映 |
| S6 | トラッカーの追跡喪失と復帰 |
| S7 | 制御パイプ経由の連続コマンド（移動，回転，プリセット） |

加えてスクリプトが `vrdummy-cli scenario` によるシナリオファイル再生と，フィーダ切断時の
追跡喪失を確認する．手順は `scripts/integration/README.md` を参照．

## CI（GitHub Actions，windows-latest）

- ドライバ: CMake 構成・ビルド・ctest．
- フィーダ: `cargo fmt --check`，`cargo clippy -D warnings`，`cargo test`，`cargo deny check`．
- 成果物: ドライバフォルダとフィーダ実行ファイル．
- Actions はコミット SHA で固定する．
