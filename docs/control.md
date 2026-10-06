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
