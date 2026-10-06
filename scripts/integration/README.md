# 統合試験

## 1. SteamVR 非依存（CI で実行）

`driver/tests/host/` のテストホストは vrserver の代わりに `driver_vrdummy.dll` を `LoadLibrary` し，
`HmdDriverFactory` から得たプロバイダに OpenVR インターフェースのフェイク
（`IVRServerDriverHost`，`IVRProperties`，`IVRDriverInput`，`IVRSettings`，`IVRDriverLog` 等）を
注入する．その上で実際の名前付きパイプにクライアントとして接続し，次を検証する．

- デバイス登録（シリアル，クラス），プロパティ，入力コンポーネントの作成
- フィーダ不在時のポーズ送出（HMD は有効，トラッカーは追跡外）
- ハンドシェイク，ログ転送，ポーズ・入力・表示パラメータの反映
- 切断時の挙動と再接続

実行: `ctest --preset msvc-x64`（`vrdummy_host_test`）．

フィーダ側は `feeder/crates/core/tests/driver_pipe.rs` がモックドライバに対して
同じプロトコルを検証する（`cargo test --workspace`）．

## 2. SteamVR 実機 E2E（手動）

前提: SteamVR がインストールされ，Steam クライアントが起動していること．
ドライバ（`scripts/build_driver.ps1`）とフィーダ（`cargo build --release`）をビルド済みであること．

```powershell
.\scripts\integration\steamvr_e2e.ps1 [-KeepRunning] [-JUnit report.xml]
```

スクリプトの手順:

1. `vrpathreg adddriver` でドライバを登録
2. SteamVR を起動し，`vrdummy_probe wait VRDUMMY-HMD-0` で仮想 HMD の出現を待つ
3. `vrdummy-cli run --no-stdin` を常駐させる（制御パイプ）
4. `vrdummy_e2e` を実行（シナリオ S1〜S7，`docs/testing.md`）
5. `vrdummy-cli scenario scenarios/walk.txt` を再生し，最終位置を `vrdummy_probe expect` で照合
6. フィーダ終了後にトラッカーが追跡外になり HMD が最終ポーズを保つことを確認
7. SteamVR を終了し，登録を解除（`-KeepRunning` で省略）

`vrdummy_e2e` と `vrdummy_probe` は SteamVR が起動していない状態では失敗する．

### 目視確認（vrdummy_viewer）

```powershell
.\scripts\install_driver.ps1
# SteamVR を起動
.\feeder\target\release\vrdummy-cli.exe run          # 別端末でコマンド入力
.\driver\build\msvc-x64\Release\vrdummy_viewer.exe   # HMD 内に床・軸・デバイス・入力を描画
```

SteamVR の VR ビュー（メニュー →「VR ビューを表示」）または拡張モードの Headset Window で
両眼の描画を確認できる．ビューアはコンソールにも全デバイスの位置・オイラー角・入力値を
周期出力する．例:

```text
vrdummy-cli set preset tpose 1.7
vrdummy-cli set euler hmd 20 -15 0
vrdummy-cli set input ctrl-r trigger/value 0.7
vrdummy-cli set proj -1.2 0.8 -1 1 -0.8 1.2 -1 1    # 非対称投影
vrdummy-cli set swap 1                               # 左右入替
```

座標系の定義は `docs/coordinates.md` を参照．
