# 制御コマンド

フィーダの CLI，GUI，制御パイプ，シナリオファイルはすべて同じテキストコマンドで動作する．
1 行 1 コマンド，空白区切り，`#` 以降はコメント．

## デバイス名

| 名前 | device_id |
|---|---|
| `hmd` | 0 |
| `ctrl-l`（`left`） | 1 |
| `ctrl-r`（`right`） | 2 |
| `trk<n>`（n は 0 始まり） | 3 + n |
| 数値 | そのまま |

## コマンド

| 構文 | 動作 |
|---|---|
| `pos <dev> <x> <y> <z>` | 位置（m，SteamVR 座標系）を設定し，ポーズを有効にする |
| `move <dev> <dx> <dy> <dz>` | 現在位置からの相対移動 |
| `rot <dev> <w> <x> <y> <z>` | 回転を四元数で設定（正規化される） |
| `euler <dev> <yaw> <pitch> <roll>` | 回転をオイラー角（度）で設定．Y（yaw）→ X（pitch）→ Z（roll）の順 |
| `valid <dev> <0\|1>` | ポーズ有効 / 追跡外 |
| `input <dev> <component> <value>` | 入力コンポーネントを更新．`value` は数値または `on`/`off` |
| `ipd <meters>` | IPD |
| `proj <lL> <lR> <lT> <lB> <rL> <rR> <rT> <rB>` | 片目ごとの投影（正接値，`GetProjectionRaw` と同じ順） |
| `swap <0\|1>` | 左右の映像を入れ替え |
| `preset <stand\|tpose\|sit> [height_m]` | 体格に応じた姿勢プリセット（HMD，両手，トラッカー 0..2） |
| `osc <address> [args...]` | OSC 送信．引数は `i:<int>`，`f:<float>`，`s:<text>`，`T`，`F` |
| `sync` | 全ポーズ・入力・表示パラメータを再送 |
| `status` | 状態を 1 行で返す |
| `status --json` | 全デバイス・入力・表示パラメータを JSON で返す |
| `quit` | 終了 |

### 入力コンポーネント名

コントローラ: `system/click`，`a/click`，`a/touch`，`b/click`，`b/touch`，`trigger/value`，
`trigger/click`，`trigger/touch`，`grip/value`，`grip/click`，`grip/touch`，`joystick/x`，
`joystick/y`，`joystick/click`，`joystick/touch`．

HMD: `system/click`，`proximity`．

数値 id（`docs/protocol.md` 3.4.1）も使える．

## 応答

制御パイプでは 1 コマンドにつき 1 行返る．

- `ok` または `ok <text>`
- `err <message>`
- `quit`

## GUI と AI Agent からの併用

GUI と `vrdummy-cli run` は同じ制御パイプ API を提供する．GUI が起動している場合は，
別の常駐フィーダを起動せず `vrdummy-cli status` / `vrdummy-cli set` でその GUI を操作する．
GUI の編集欄と 3D 表示にも変更が反映される．独自の設定を使う場合は双方に同じ
`--config` を渡す．同じ制御パイプ名で GUI と `run` を同時起動しない．

```powershell
# GUI を起動しておき，別の端末または Agent から実行する
vrdummy-gui --config feeder/vrdummy.example.toml
vrdummy-cli --config feeder/vrdummy.example.toml status --json
vrdummy-cli --config feeder/vrdummy.example.toml set --json pos hmd 0 1.7 0
vrdummy-cli --config feeder/vrdummy.example.toml status --json --timeout-ms 3000
```

`--json` は `status` と `set` に対応する．標準出力は改行で終わる JSON オブジェクト 1 個，
診断ログは標準エラーに出力する．`schema_version` は 1．成功は
`{"schema_version":1,"ok":true,"result":...}`，失敗は
`{"schema_version":1,"ok":false,"error":{"kind":"...","message":"..."}}`．
状態変更の成功時の `result` は `null`，`quit` は `{"quit":true}`．

`status --json` の `result` は次の状態を持つ．これはフィーダが保持する指令値であり，
SteamVR の実測値やドライバ側の適用完了通知ではない．GUI と CLI フィーダのどちらでも同じ形式を返す．

| フィールド | 内容 |
|---|---|
| `schema_version` | 状態形式の版（1） |
| `connected` | フィーダとドライバの接続状態 |
| `devices` | 存在する全デバイスの配列，ID 順 |
| `devices[].id` | 数値 ID．そのまま `set pos <id> ...` 等に使用できる |
| `devices[].serial`, `class` | ハンドシェイクの情報．未取得時は `null`．class は `docs/protocol.md` の列挙値 |
| `devices[].pose_valid`, `connected`, `tracking_result` | 保持しているポーズのフラグと追跡状態の列挙値 |
| `devices[].position_m`, `rotation_wxyz` | 位置と四元数 |
| `devices[].linear_velocity_m_s`, `angular_velocity_rad_s` | 速度と角速度 |
| `devices[].inputs` | `component_id` と `value` の配列 |
| `display` | `ipd_m`, `projection_left`, `projection_right`, `swap_eyes` |

JSON で表せない非有限数は `null` として返す．状態照会は録画しない．旧版の GUI／フィーダは
JSON 状態取得に対応していないため，CLI と同じ版へ更新して再起動する．

| 終了コード | JSON error.kind | 意味 |
|---|---|---|
| 0 | — | 要求成功 |
| 1 | `command_error` | サーバがコマンドを拒否 |
| 2 | `argument_error` | CLI 引数・設定・ログ初期化エラー |
| 3 | `transport_error` | パイプ接続・送受信エラー |
| 4 | `timeout` | 通信期限超過（サーバ未起動・接続待ちも含む） |
| 5 | `protocol_error` | 未対応または認識できない応答 |

`--timeout-ms` は接続・送信・応答受信を合わせた期限（既定 2000 ms，1〜3600000 ms）．
`status` / `set` の前にも指定できる．パイプの入出力は期限超過時にキャンセルする．
要求と応答はそれぞれ最大 1 MiB，1 要求につき 1 行．

**タイムアウトしても，コマンドが未実行とは限らない．** Agent は状態を再照会し，
`move` やボタン操作などを無条件で再送しない．再試行が必要な処理では `pos` 等の絶対値指定を使う．
ドライバ未接続でもフィーダの状態変更は成功し，再接続時に送信されるため，
ドライバへの接続が必要な処理では `result.connected` も確認する．

## ドライバの登録管理

`vrdummy-cli driver <status|register|unregister> [--path <dir>] [--steamvr-dir <dir>] [--json] [--timeout-ms <ms>]`
は常駐フィーダを必要としない．GUI の Driver registration も同じ処理を使用する．

- `--path` は `driver.vrdrivermanifest` を含むフォルダ．省略時は既存の vrdummy 登録先，
  未登録なら作業ディレクトリからの `driver/build/msvc-x64/vrdummy`．複数登録がある場合は明示指定する．
- SteamVR は OpenVR 登録ファイルの runtime から検出する．`--steamvr-dir` で指定も可能．
- 登録は manifest の名前が `vrdummy` であり，Windows x64 DLL が存在する場合のみ許可する．
  別の場所にある vrdummy の重複登録は拒否する．解除は選択したパスだけを対象とする．
  登録後にフォルダを削除した場合は，元のパスを `--path` に指定して解除できる．
- JSON は既存の `schema_version` / `ok` / `result` 形式．`result` は `driver_dir`，`steamvr_dir`，
  `registered`，`registered_paths`，`package_valid`，`tool_available`，`changed`，`restart_required`．
  `registered` は選択したパスの登録状態であり，ドライバのロード・有効化・接続を保証しない．
- 登録・解除は `vrpathreg.exe` を使用し，操作後に登録ファイルを読み直して確認する．
  登録済みへの再登録，未登録への解除は `changed: false` で成功する．
  `restart_required: true` は今回の変更が次回の SteamVR 起動で反映されることを示す．
- 終了コードは成功 0，引数エラー 2，登録管理エラー 1（`driver_management_error`）．
  このコマンドの `--timeout-ms` は vrpathreg 子プロセスの期限であり，既定 2000 ms．
  期限超過時は状態を再照会する．SteamVR の起動・終了，アドオンのブロック解除は行わない．

GUI ではパスを入力して Register / Unregister を押す．状態は約 3 秒ごとに更新される．
外部操作の処理中もデバイス編集を継続できる．SteamVR の再起動は利用者が行う．

## シナリオファイル

先頭に開始からの秒数を付ける．時刻は単調非減少．

```text
0.0  preset stand
1.0  pos hmd 0 1.7 0
1.5  input ctrl-r a/click 1
1.6  input ctrl-r a/click 0
```

`vrdummy-cli run --record <file>` で実行したコマンドが同じ形式で記録され，
`vrdummy-cli scenario <file>` で再生できる．

## 例

```powershell
# フィーダを常駐させ，標準入力と制御パイプからコマンドを受ける
vrdummy-cli run

# 別の端末から
vrdummy-cli set pos hmd 0 1.7 0
vrdummy-cli set input ctrl-r a/click on
vrdummy-cli set proj -1.2 0.8 -1 1 -0.8 1.2 -1 1
vrdummy-cli set osc /input/Jump i:1

# シナリオを流して終了
vrdummy-cli scenario .\tests\scenarios\walk.txt
```
