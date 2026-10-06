# ドライバ・フィーダ間IPCプロトコル（版1）

本書は `driver_vrdummy`（SteamVR側，vrserver内）と feeder（独立プロセス）の間で用いる
ワイヤ形式を定義する．実装は `driver/src/protocol.*`（C++）と `feeder/crates/protocol`（Rust）の
二箇所にあり，`tests/fixtures/*.bin` のゴールデンデータで相互整合を検証する．

## 1. トランスポート

- Windows 名前付きパイプ `\.\pipe\vrdummy-driver`（名前は `default.vrsettings` の
  `driver_vrdummy.pipe_name` で変更可能）．
- ドライバがサーバ，フィーダがクライアント．同時接続は1本．
- バイトストリームモード．フレーム境界は本書のヘッダで決める（パイプのメッセージモードに依存しない）．
- フィーダは切断を検知したら再接続する．ドライバはフィーダ不在時も動作を継続する
  （HMDは最終ポーズを保持，コントローラ・トラッカーは `RUNNING_OUT_OF_RANGE`）．
- ドライバ側はオーバーラップ I/O で読み書きを並行させる．フィーダ側は同期ハンドルを
  単一スレッドで扱い，`PeekNamedPipe` が報告した到着済みバイトだけを読む．同期ハンドルでは
  複製ハンドル上でブロック中の読み取りが同じファイルオブジェクトへの書き込みを待たせるため，
  受信スレッドと送信を併用するとデッドロックする．

## 2. フレーム

全フィールドはリトルエンディアン．構造体のパディングに依存せず，各フィールドを明示的に
符号化・復号する．

```
offset size  field
0      u32   payload_len   ヘッダを含まないペイロード長（最大 65536）
4      u16   msg_type      3節
6      u16   version       1 固定
8      ...   payload
```

`version` が一致しないフレーム，および未知の `msg_type` は読み飛ばして警告ログを出す．
`payload_len` が上限を超えたら接続を切る．

## 3. メッセージ種別

| 値 | 名前 | 方向 | ペイロード |
|---|---|---|---|
| 1 | HELLO | feeder→driver | Hello |
| 2 | HELLO_ACK | driver→feeder | HelloAck + DeviceInfo[n] |
| 3 | POSE_UPDATE | feeder→driver | Pose |
| 4 | INPUT_UPDATE | feeder→driver | Input |
| 5 | DISPLAY_UPDATE | feeder→driver | Display |
| 6 | LOG_RECORD | driver→feeder | Log |
| 7 | GOODBYE | 双方向 | なし |

接続直後にフィーダは HELLO を送り，ドライバは HELLO_ACK を返す．HELLO_ACK 受信前に
送られた更新メッセージは破棄される．

### 3.1 Hello（8 bytes）

```
0  u32 protocol_version   1
4  u32 client_flags       予約（0）
```

### 3.2 HelloAck（8 + 40×n bytes）

```
0  u32 protocol_version
4  u16 device_count       n
6  u16 reserved
8  DeviceInfo × n
```

DeviceInfo（40 bytes）:

```
0  u16 device_id
2  u8  device_class       1 HMD, 2 CONTROLLER_LEFT, 3 CONTROLLER_RIGHT, 4 TRACKER
3  u8  reserved
4  u8  serial[36]         UTF-8，NUL 埋め
```

`device_id` はドライバが起動時に固定で割り当てる．0 = HMD，1 = 左コントローラ，
2 = 右コントローラ，3 以降 = トラッカー．無効化されたデバイスの id は欠番となり，
HELLO_ACK には存在するデバイスのみ列挙される．

### 3.3 Pose（120 bytes）

```
0    u16 device_id
2    u8  flags             bit0 POSE_VALID, bit1 CONNECTED
3    u8  tracking_result   0 UNINITIALIZED, 1 RUNNING_OK, 2 RUNNING_OUT_OF_RANGE,
                           3 CALIBRATING, 4 FALLBACK_ROTATION_ONLY
4    u32 reserved
8    u64 timestamp_us      フィーダ側単調時刻（ログ・遅延計測用）
16   f64 position[3]       m，SteamVR 座標系（右手系，+Y 上，-Z 前）
40   f64 rotation[4]       単位四元数 (w, x, y, z)
72   f64 linear_velocity[3]   m/s
96   f64 angular_velocity[3]  rad/s
```

ドライバは受信した値を保持し，`pose_rate_hz` の周期で `TrackedDevicePoseUpdated` に渡す．
`poseTimeOffset` は常に 0．

### 3.4 Input（20 bytes）

```
0   u16 device_id
2   u16 component_id      3.4.1
4   u8  kind              0 BOOL, 1 SCALAR
5   u8  reserved[3]
8   f32 value             BOOL は 0.0 / 1.0
12  f64 time_offset_s     IVRDriverInput の fTimeOffset にそのまま渡す（通常 0）
```

#### 3.4.1 component_id

HMD:

| id | パス | 種別 |
|---|---|---|
| 0 | /input/system/click | BOOL |
| 1 | /proximity | BOOL |

コントローラ（左右共通）:

| id | パス | 種別 |
|---|---|---|
| 0 | /input/system/click | BOOL |
| 1 | /input/a/click | BOOL |
| 2 | /input/a/touch | BOOL |
| 3 | /input/b/click | BOOL |
| 4 | /input/b/touch | BOOL |
| 5 | /input/trigger/value | SCALAR（片側正規化 0..1） |
| 6 | /input/trigger/click | BOOL |
| 7 | /input/trigger/touch | BOOL |
| 8 | /input/grip/value | SCALAR（片側正規化 0..1） |
| 9 | /input/grip/click | BOOL |
| 10 | /input/grip/touch | BOOL |
| 11 | /input/joystick/x | SCALAR（両側正規化 -1..1） |
| 12 | /input/joystick/y | SCALAR（両側正規化 -1..1） |
| 13 | /input/joystick/click | BOOL |
| 14 | /input/joystick/touch | BOOL |

トラッカー: 版1では入力コンポーネントを持たない．

### 3.5 Display（44 bytes）

HMD の光学パラメータを実行時に変更する．描画解像度は vrcompositor が起動時に一度しか
読まないため，`default.vrsettings` でのみ設定する．

```
0   u32 flags                bit0 IPD, bit1 PROJECTION, bit2 SWAP_EYES（有効なフィールド）
4   f32 ipd_m
8   f32 projection_left[4]   GetProjectionRaw と同じ (left, right, top, bottom) の正接値
24  f32 projection_right[4]
40  u8  swap_eyes            0 / 1
41  u8  reserved[3]
```

IPD 変更時，ドライバは `Prop_UserIpdMeters_Float` を更新する．投影行列はアプリケーションが
起動時にキャッシュする場合があり，反映にはアプリケーションの再起動が必要になり得る．

### 3.6 Log（12 + 可変）

```
0   u8  level              0 ERROR, 1 WARN, 2 INFO, 3 DEBUG, 4 TRACE
1   u8  reserved[3]
4   u64 timestamp_us       ドライバ側単調時刻
12  u8  text[]             UTF-8，NUL 終端なし，長さは payload_len - 12
```

## 4. 互換性方針

- 既存フィールドの意味・位置は変更しない．追加は末尾に行い，`version` を上げる．
- 両実装は固定レイアウトのサイズを静的に検査する（C++: `static_assert`，Rust: 単体試験）．
- `tests/fixtures/` のゴールデンデータは Rust 側の試験で生成し（`VRDUMMY_UPDATE_FIXTURES=1`），
  C++ 側の試験で復号と再符号化の一致を確認する．
