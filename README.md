# vr-dummy-headset

SteamVR から認識される仮想デバイス一式（HMD，左右コントローラ，汎用トラッカー）と，
それらを操作するフィーダアプリケーション．VR 環境（主に VRChat）のデバッグ用．

- 設計: [docs/design.md](docs/design.md)
- 座標系: [docs/coordinates.md](docs/coordinates.md)
- IPC プロトコル: [docs/protocol.md](docs/protocol.md)
- 制御コマンド: [docs/control.md](docs/control.md)
- 試験方針: [docs/testing.md](docs/testing.md)
- 統合試験と E2E: [scripts/integration/README.md](scripts/integration/README.md)

## 実機 HMD（PICO 4 など）と併用するときの注意

ダミードライバは常時「HMD あり」を宣言するため，登録したまま実機の HMD を使うと HMD が
二重になり，どちらが採用されるかは起動順に依存する．

- **実機を使う日は登録を解除する**: `.\scripts\uninstall_driver.ps1`，または SteamVR の
  設定 → スタートアップ/シャットダウン → アドオンの管理 → vrdummy をオフ．
- **実機と併用してトラッカーだけ試す場合**: `driver/resources/settings/default.vrsettings` で
  `hmd_enable: false`，`controllers_enable: false` にする．この構成ではユニバース ID を
  設定しないため，シャペロン（ルームセットアップ）にも影響しない．
- **シャペロンの書き込み**: `vrdummy_probe chaperone`，`vrdummy_e2e`，`vrdummy_viewer` は
  standing 原点 = raw 原点・3 m × 3 m をアクティブな HMD のユニバースに書き込む．実機の
  ルームセットアップを上書きしないよう，アクティブな HMD がダミー（ユニバース ID
  0x5652444D）でなければ拒否する（`--force` で強制）．`steamvr_e2e.ps1` は実行前に
  `Steam/config/chaperone_info.vrchap` を `.tmp/chaperone-backup/` に複製する．
- VRChat 向け既定バインディングは `vrdummy_controller` にのみ適用され，実機のコントローラの
  バインディングには干渉しない．

詳細は [docs/coordinates.md](docs/coordinates.md) を参照．

## 構成

```
driver/                 OpenVR ドライバ driver_vrdummy（C++ / CMake）
driver/tests/host/      ドライバ DLL をフェイク OpenVR でロードする結合試験（SteamVR 不要）
driver/tools/           OpenVR クライアント: vrdummy_probe，vrdummy_e2e（E2E シナリオ），
                        vrdummy_viewer（HMD 内とコンソールにデバイスと入力を表示）
feeder/                 フィーダ（Rust workspace: protocol, core, cli, gui）
third_party/openvr/     OpenVR SDK（サブモジュール）
third_party/doctest/    doctest 単一ヘッダ
tests/fixtures/         プロトコルのゴールデンデータ
scripts/                ドライバ登録・解除，ビルド補助，統合試験手順
docs/                   設計書類
```

## ビルド

### 前提

- Windows 11，Visual Studio 2022（C++ ツールセット，CMake コンポーネント）
- Rust（版は `rust-toolchain.toml` で固定）
- SteamVR（統合試験時のみ）

### 取得

```powershell
git clone --recurse-submodules https://github.com/sabas0ba/vr-dummy-headset.git
```

### ドライバ

```powershell
.\scripts\build_driver.ps1
```

成果物は `driver/build/msvc-x64/vrdummy/` に SteamVR のドライバフォルダ形式で出力される．

### フィーダ

```powershell
cd feeder
cargo build --release
cargo test --workspace
```

## SteamVR への登録

```powershell
.\scripts\install_driver.ps1     # 登録
.\scripts\uninstall_driver.ps1   # 解除
```

手順の詳細は [scripts/integration/README.md](scripts/integration/README.md) を参照．

## GUI と CLI の使い方

リポジトリのルートから GUI を起動する．以下は release ビルドの実行例．

```powershell
.\feeder\target\release\vrdummy-gui.exe --config feeder/vrdummy.example.toml
```

GUI を起動したまま，別の端末や AI Agent から同じ設定を指定して操作・状態取得できる．
同じ制御パイプで `vrdummy-cli run` を追加起動する必要はない．

```powershell
# HMD の位置を設定する（単位: m）
.\feeder\target\release\vrdummy-cli.exe --config feeder/vrdummy.example.toml set --json pos hmd 0 1.7 0

# 状態を JSON で取得する（接続・送受信の期限: 3000 ms）
.\feeder\target\release\vrdummy-cli.exe --config feeder/vrdummy.example.toml status --json --timeout-ms 3000
```

`--json` は状態・操作結果を標準出力に返す．状態はフィーダが保持する指令値であり，
SteamVR の実測値ではない．コマンド一覧と終了コードは [docs/control.md](docs/control.md) を参照．

## ライセンス

Apache-2.0．third_party 配下は各ライセンス（OpenVR SDK: BSD-3-Clause，doctest: MIT）に従う．
