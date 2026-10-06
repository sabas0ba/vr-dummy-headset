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

## 2. SteamVR 実機（手動）

前提: SteamVR がインストールされ，Steam クライアントが起動していること．
ドライバ（`scripts/build_driver.ps1`）とフィーダ（`cargo build --release`）をビルド済みであること．

```powershell
.\scripts\integration\steamvr_check.ps1
```

スクリプトは次を行い，期待値の一致を PASS / FAIL で表示する．

1. `vrpathreg adddriver` でドライバを登録
2. SteamVR を起動し，`vrdummy_probe wait VRDUMMY-HMD-0` で仮想 HMD の出現を待つ
3. `vrdummy_probe list` でコントローラ 2 台とトラッカーを確認
4. `vrdummy-cli run --no-stdin` を常駐させ，制御パイプ経由で位置を設定
5. `vrdummy_probe expect <serial> x y z` で OpenVR クライアントから見た位置を照合
6. フィーダ終了後にトラッカーが追跡外になることを確認
7. SteamVR を終了し，登録を解除（`-KeepRunning` で省略）

`vrdummy_probe` は `VRApplication_Background` で初期化するため，SteamVR が起動していない
状態では失敗する．

### 手動確認の補助

```powershell
.\driver\build\msvc-x64\Release\vrdummy_probe.exe list
.\driver\build\msvc-x64\Release\vrdummy_probe.exe pose VRDUMMY-TRK-3
.\feeder\target\release\vrdummy-cli.exe set euler hmd 30 0 0
```

SteamVR の VR ビュー（メニュー →「VR ビューを表示」）で視点の変化と左右の映像を確認できる．
