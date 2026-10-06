# 統合試験手順（SteamVR 実機）

CI では実行しない．SteamVR がインストールされた Windows 機で手動実行する．

## 前提

- SteamVR が終了していること．
- `scripts/build_driver.ps1` でドライバをビルド済みであること．
- `cargo build --release`（`feeder/` 内）でフィーダをビルド済みであること．

## 手順

1. ドライバを登録する．

   ```powershell
   .\scripts\install_driver.ps1
   ```

2. SteamVR を起動する．仮想 HMD により SteamVR が「待機中」ではなく起動完了になること，
   コントローラ 2 台とトラッカー 3 台（既定）がアイコンに現れることを確認する．
   フィーダ未接続のため，HMD 以外は追跡外（点滅）表示になる．

3. フィーダを起動する．

   ```powershell
   .\feeder\target\release\vrdummy-cli.exe run
   ```

   SteamVR の Web コンソール（開発者設定）で `vrdummy:` を検索し，
   `feeder handshake complete` が出ていることを確認する．

4. ポーズと入力を送る．

   ```powershell
   .\feeder\target\release\vrdummy-cli.exe set hmd pos 0 1.7 0
   .\feeder\target\release\vrdummy-cli.exe set ctrl-r button a 1
   ```

   SteamVR の VR ビュー（メニューから「VR ビューを表示」）で視点が変わること，
   コントローラのバインディング画面で入力が反応することを確認する．

5. 終了後，登録を解除する．

   ```powershell
   .\scripts\uninstall_driver.ps1
   ```

## 期待値の自動比較（今後）

`openvr_api` をリンクした小さなクライアントツールで
`IVRSystem::GetDeviceToAbsoluteTrackingPose` を読み取り，シナリオの期待値と比較する．
