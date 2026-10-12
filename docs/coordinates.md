# 座標系

本プロジェクトで扱う空間と規約を定める．数値はすべてメートルとラジアン（CLI とシナリオ
ファイルの `euler` のみ度）．

## 1. 空間

| 空間 | 定義 |
|---|---|
| ドライバ空間 | ドライバが `DriverPose_t` で報告する空間．`qWorldFromDriverRotation` は単位四元数，`vecWorldFromDriverTranslation` は 0 とする．したがって **ドライバ空間 = SteamVR raw 空間**（`TrackingUniverseRawAndUncalibrated`）． |
| standing 空間 | シャペロン設定（ルームセットアップ）で定まる床原点の空間．VRChat をはじめ多くのアプリはこれを使う．`vrdummy_probe chaperone [size_m]` が `IVRChaperoneSetup` で **standing 原点 = raw 原点**，正方形のプレイエリア（既定 3 m × 3 m）を書き込みコミットする．`vrdummy_e2e` と `vrdummy_viewer` も起動時に同じ設定をコミットする． |
| seated 空間 | 同様に seated 原点 = raw 原点に設定する． |

ドライバは HMD に固定のトラッキングユニバース ID（`Prop_CurrentUniverseId_Uint64`，既定
0x5652444D，`default.vrsettings` の `universe_id`）を報告する．SteamVR はシャペロン設定をこの ID
をキーに `config/chaperone_info.vrchap` へ保存するため，一度コミットすれば SteamVR を再起動しても
standing 空間とプレイエリアが保持され，「ルームセットアップを実行してください」の通知も出ない．
ID を変えると別のユニバースとして扱われ，再度コミットが必要になる．

### 実機 HMD との共存

実機（PICO 4 など）のストリーミングドライバは独自のユニバース ID を報告するため，設定は
`chaperone_info.vrchap` 内で別エントリになり，通常は互いに干渉しない．ただしシャペロンを
書き込むツール（`vrdummy_probe chaperone`，`vrdummy_e2e`，`vrdummy_viewer`）は「アクティブな
HMD のユニバース」に書き込むので，実機がアクティブな状態で実行すると実機のルームセットアップを
上書きする．これを防ぐため，各ツールはアクティブな HMD の `Prop_CurrentUniverseId_Uint64` が
ダミーの ID と一致しない限り書き込みを拒否する（`vrdummy_probe chaperone --force` で強制）．
`steamvr_e2e.ps1` は実行前に `Steam/config/chaperone_info.vrchap` を
`.tmp/chaperone-backup/` に複製するので，必要ならそこから手で戻せる．

実機と併用してトラッカーだけを使う場合（`hmd_enable: false`）はダミーの HMD が存在せず
ユニバース ID も設定されないため，standing 原点は実機のルームセットアップに従う．フィーダの
`pos` は raw 空間の値なので，実機の standing 原点が raw からずれているぶん，アプリから見た位置は
ずれる．`vrdummy_probe chaperone-state` で standing 原点の raw からのオフセットを確認できる．

フィーダのコマンド（`pos`，`rot`，`euler`，プリセット）はすべてドライバ空間の値である．

## 2. 軸と向き

SteamVR の規約に従う．右手系．

- +X: 右，+Y: 上，-Z: 前（利用者の正面）
- 床は y = 0．立位の HMD は y ≈ 身長 × 0.93（両眼中心の高さ）
- 回転は単位四元数 (w, x, y, z)．`rot` コマンドはこの順
- `euler <yaw> <pitch> <roll>`（度）は Y → X → Z の順に適用する固有回転
  - yaw: +Y 回り．正で左を向く（上から見て反時計回り）．yaw = +90 で -X を向く
  - pitch: +X 回り．正で上を向く
  - roll: +Z 回り．正で頭が左に傾く（時計回りに見える）

## 3. デバイスの原点

| デバイス | 報告する点 | 備考 |
|---|---|---|
| HMD | 両眼の中点 | `shouldApplyHeadModel = false`．vrserver の頸部モデルによる位置補正を受けない．左右の眼は `Prop_UserIpdMeters_Float` から ±IPD/2 を X にずらした位置 |
| コントローラ | コントローラの基準点（`/pose/raw`） | 単位回転のとき +X 右，-Z 前．グリップ・エイムの派生ポーズは入力プロファイルに定義しない（版 1） |
| トラッカー | トラッカーの基準点 | 役割（腰・足など）は SteamVR 側またはアプリ側（VRChat はキャリブレーション時の位置）で決まる |

## 4. プリセットの配置（`preset <name> [height_m]`，既定 1.7 m）

| 名前 | HMD | 両手 | トラッカー 0 / 1 / 2 |
|---|---|---|---|
| stand | (0, 0.93h, 0) | (∓0.2, 0.53h + 0.15, -0.3) | 腰 (0, 0.53h, 0)，左足 (-0.1, 0.05, 0)，右足 (0.1, 0.05, 0) |
| tpose | (0, 0.93h, 0) | (∓0.41h, 0.82h, 0)，手は ±90° ロール | 同上 |
| sit | (0, 0.26h + 0.40h, 0) | (∓0.2, 0.26h + 0.1, -0.3) | 腰 (0, 0.26h, 0)，足 (∓0.1, 0.05, -0.4) |

## 5. 投影と表示

- `proj lL lR lT lB rL rR rT rB` は `IVRDisplayComponent::GetProjectionRaw` と同じ正接値
  （左・右・上・下）．対称投影は `-1 1 -1 1`
- `swap 1` は左右の眼の出力ビューポートと投影を入れ替える
- 描画解像度は `default.vrsettings` の `vrdummy_display` でのみ設定する

## 6. VRChat との対応

- VRChat は standing 空間を使う．床 y = 0，初期の正面は -Z
- 全身トラッキングのキャリブレーションはトラッカーの相対位置で役割を割り当てる．
  `preset stand` または `tpose` の配置（腰 1 点，足 2 点）が 3 点トラッキングに対応する
- アバターの身長合わせは HMD の高さを基準にするため，`preset` の身長引数で調整する
