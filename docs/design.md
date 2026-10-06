# vr-dummy-headset 設計書

## 1. 目的

VR 環境（主に VRChat）のデバッグ用に，SteamVR から認識される仮想デバイス一式を提供する．

- 仮想 HMD，左右コントローラ，複数の汎用トラッカー（全身トラッキング用）
- キーボード・ゲームパッドによる移動・ジャンプ等の操作
- GUI と CLI による全身姿勢の編集，記録，再生
- 視線・表情データの VRChat への送信（OSC）
- Stereo 描画向けに最適化されていないシェーダの不具合検出を助ける光学パラメータ操作

## 2. 実現経路

| 要求 | 経路 |
|---|---|
| HMD・コントローラ・トラッカーの認識 | 自作 OpenVR ドライバ `driver_vrdummy` |
| コントローラ操作 | ドライバの `IVRDriverInput` にフィーダの入力を写像 |
| 全身トラッキング | `TrackedDeviceClass_GenericTracker` を複数公開 |
| 片目・両目映像 | SteamVR 標準の VR ビュー（ミラー窓）を利用．自前窓は対象外 |
| 視線 | VRChat OSC（`/tracking/eye/...`） |
| 表情 | VRChat OSC のアバターパラメータ（Unified Expressions 互換名） |

SteamVR には視線・表情の標準インターフェースが無いため，これらは OSC で直接 VRChat へ送る．
トラッカーは SteamVR 経由を主とし，VRChat OSC Trackers は補助とする．

## 3. 構成

```
driver/                 C++ / CMake．OpenVR ドライバ本体
feeder/                 Rust workspace（protocol, core, cli, gui）
third_party/openvr/     Valve OpenVR SDK（サブモジュール，SHA 固定）
third_party/doctest/    doctest 単一ヘッダ（MIT，版と SHA256 を同梱記録）
tests/fixtures/         プロトコルのゴールデンデータ（両言語共通）
scripts/                ドライバ登録・解除，ビルド補助，統合試験手順
docs/                   設計書，プロトコル仕様，試験方針
.github/workflows/      CI
```

### 3.1 ドライバ（C++）

OpenVR ドライバは C++ 仮想関数テーブルの ABI を持つため C++ で実装する．vrserver 内で動作
するため極力薄く保ち，状態を持たない．

- `device_provider`: `IServerTrackedDeviceProvider`．設定からデバイス構成を読み，登録する．
  パイプサーバとポーズ送出スレッドを所有する．
- `hmd_device` / `controller_device` / `tracker_device`: `ITrackedDeviceServerDriver`．
  受信した最新状態を保持し，`TrackedDevicePoseUpdated` と `Update*Component` を呼ぶ．
- `display_component`: `IVRDisplayComponent`．拡張モードの仮想ディスプレイ．IPD，片目ごとの
  投影，左右入れ替えを実行時に変更可能．
- `protocol`: ワイヤ形式の符号化・復号．OpenVR 非依存で単体試験の対象．
- `pipe_server`: Win32 名前付きパイプサーバ（受信スレッド）．
- `logging`: `IVRDriverLog` への出力と，接続中のフィーダへの転送．

OpenVR のインターフェースを継承するクラスは委譲のみとし，判断を含むロジックは OpenVR
非依存のクラスに置く．

HMD のポーズは `shouldApplyHeadModel = false` で送る．有効にすると vrserver が向きに依存する
頸部モデルのオフセットを加え，指示した位置が OpenVR 側で再現されなくなるためである．

フィーダ未接続時の振る舞い:

- HMD は最終ポーズを保持し，SteamVR を操作可能に保つ．
- コントローラとトラッカーは `TrackingResult_Running_OutOfRange` にする．

### 3.2 フィーダ（Rust）

- `protocol`: ワイヤ形式．依存クレートなし．
- `core`: 状態モデル，骨格，入力写像，OSC 送信，記録再生，制御 API，ログ．
- `cli`: ヘッドレス実行，シナリオ実行，再生，起動中インスタンスへの単発コマンド．
- `gui`: eframe/egui．`core` を同一プロセスで保持する．

CLI と GUI は同じ制御 API（ローカル名前付きパイプ，行区切りテキストコマンド）を使う．
JSON 用クレートの追加を避けるため，制御コマンドは空白区切りのテキスト形式とする．
コマンドの文法は `docs/control.md` に定める．GUI のウィジェット操作もコマンド文字列に変換して
同じ経路を通るため，GUI・CLI・制御パイプ・シナリオファイルの挙動は一致する．

### 3.3 IPC

`docs/protocol.md` を参照．

### 3.4 ログ

- 形式は JSON Lines（1 行 1 イベント）．フィールドは単調時刻（`ts_us`），レベル，
  モジュール（`target`），メッセージ．標準エラー出力には短い人間向け形式で出す．
- ドライバ側は `IVRDriverLog`（vrserver.txt）に出力しつつ，パイプ経由でフィーダのログに
  合流させる（`target` は `driver`，ドライバ側の時刻を本文に含める）．
- 冗長度は CLI 引数 `--level` と環境変数 `VRDUMMY_LOG` で制御する．
- 記録と再生は JSON ではなくシナリオ形式（`<秒> <コマンド>`，`docs/control.md`）で行う．
  実行したコマンド列をそのまま記録し，同じ解釈器で再生するため，JSON の読み戻しが不要になる．

### 3.5 Stereo シェーダ不具合検出のための操作

フィーダから次を変更できる．

- IPD（`Prop_UserIpdMeters_Float` の更新と `VREvent_IpdChanged`）
- 片目ごとの非対称投影（`GetProjectionRaw`）
- 左右入れ替え
- 描画解像度（設定ファイルのみ．vrcompositor が起動時に一度だけ読む）

画面空間 UV を対称投影前提で扱うシェーダや，instanced stereo で eye index を誤る実装は，
非対称投影と大きな IPD で顕在化する．投影行列はアプリケーションが起動時にキャッシュする
場合があり，反映にはアプリケーションの再起動が必要になり得る．

## 4. コントローラ種別とバインディング

他社識別子（既存機種の controller type 文字列）を名乗らず，独自の入力プロファイル
`vrdummy_controller` を定義する．VRChat 向け既定バインディングはドライバに同梱する方針
だが，アクション名の検証を要するため初期版では空とし，統合試験時に追加する．

## 5. 依存

| 依存 | 用途 | ライセンス | 固定方法 |
|---|---|---|---|
| OpenVR SDK | ドライバ API | BSD-3 | サブモジュール SHA |
| doctest | C++ 単体試験 | MIT | 単一ヘッダ同梱，SHA256 記録 |
| eframe/egui | GUI | MIT / Apache-2.0 | Cargo.lock |
| serde, toml | 設定ファイル | MIT / Apache-2.0 | Cargo.lock |
| log | ログファサード | MIT / Apache-2.0 | Cargo.lock |

OSC，名前付きパイプ，入力デバイスは標準 API と自前実装で扱う．Rust の Win32 呼び出しは
`extern "system"` の手書き宣言で済ませ，バインディングクレートを追加しない．

## 6. 試験と CI

`docs/testing.md` を参照．

## 7. 実装順序

1. HMD と両手コントローラ，キーボードによる移動（a）
2. 光学パラメータ操作と VR ビューでの確認（d）
3. トラッカーと全身トラッキング（b）
4. OSC による視線・表情（c）
