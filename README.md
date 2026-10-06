# vr-dummy-headset

SteamVR から認識される仮想デバイス一式（HMD，左右コントローラ，汎用トラッカー）と，
それらを操作するフィーダアプリケーション．VR 環境（主に VRChat）のデバッグ用．

- 設計: [docs/design.md](docs/design.md)
- 座標系: [docs/coordinates.md](docs/coordinates.md)
- IPC プロトコル: [docs/protocol.md](docs/protocol.md)
- 制御コマンド: [docs/control.md](docs/control.md)
- 試験方針: [docs/testing.md](docs/testing.md)
- 統合試験と E2E: [scripts/integration/README.md](scripts/integration/README.md)

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

## ライセンス

Apache-2.0．third_party 配下は各ライセンス（OpenVR SDK: BSD-3-Clause，doctest: MIT）に従う．
