# GitHub Pages の公開と更新

このリポジトリは，`main` ブランチのルートを GitHub Pages の公開元として使用する．
README をトップページとして表示し，`docs/` と統合試験の README を同じナビゲーションで参照できる．

## 初回の公開設定

1. Pages 用設定を含む PR を `main` にマージする．
2. GitHub リポジトリの **Settings → Pages → Build and deployment** を開く．
3. **Source** を **Deploy from a branch** にする．
4. **Branch** を **main**，フォルダを **/ (root)** にして **Save** を押す．
5. Actions の **pages build and deployment** が成功した後，公開 URL を開く．

公開 URL: <https://sabas0ba.github.io/vr-dummy-headset/>

`/docs` を公開元にすると，ルートの設定・README・テーマが読み込まれないため，必ず `/ (root)` を選ぶ．
公開設定にはリポジトリの管理権限が必要．

## 更新するファイル

| 対象 | ファイル |
|---|---|
| トップページ・GUI の使い方 | `README.md` |
| 技術文書・操作リファレンス | `docs/*.md`，`scripts/integration/README.md` |
| スクリーンショット | `docs/images/` |
| サイト名・公開 URL・ナビゲーション・除外対象 | `_config.yml` |
| 共通 HTML レイアウト | `_layouts/default.html` |
| 配色・余白・狭い画面への対応 | `assets/css/site.css` |

これらを `main` にマージするとサイトが更新される．HTML の手動生成や `gh-pages` ブランチへのコピーは不要．
Markdown 内の文書リンクは引き続き `.md` で記述する．GitHub Pages 標準の
`jekyll-relative-links` が公開先の HTML URL に変換し，`jekyll-readme-index` が README を
各ディレクトリのトップページにする．README とサイト本文を重複管理しない．

テーマはこのリポジトリ内の HTML/CSS で管理する．外部テーマ，JavaScript，外部 Web フォントを
読み込まない．Pages 標準の Jekyll と標準プラグインは GitHub 管理の実行環境を使用するため，
そのバージョンはリポジトリ側では固定しない．アプリ本体の依存とは独立している．

別の所有者・リポジトリ名で公開する場合は，`_config.yml` の `url`，`baseurl`，`repository` を変更する．
`baseurl` はプロジェクトサイトのパス（例: `/vr-dummy-headset`）であり，末尾に `/` を付けない．

## PR での検証

`pages-check` ワークフローが，SHA 固定の GitHub 公式 `actions/jekyll-build-pages` で HTML を生成し，
`scripts/check_pages.py` で内部リンク・ページ内リンク・画像と公開除外対象を検証する．
PC 幅・モバイル幅のスクリーンショットと生成 HTML は Actions の `pages-preview` 成果物から取得できる．
このワークフローは検証専用であり，サイトを公開しない．

実際の公開は **Deploy from a branch** が管理する **pages build and deployment** が担当する．
ローカルへの Ruby や Node.js のインストールは不要．

## 公開後の確認

- トップページの GUI スクリーンショットとサイトのスタイルが表示されること．
- ナビゲーションから制御コマンド，統合試験，本ページを開き，トップへ戻れること．
- README の文書リンクと「GUI と CLI の使い方」へのページ内リンクが機能すること．
- 狭い画面でも画像が本文幅に収まり，表・コードを横スクロールできること．

公開物からドライバ・フィーダのソースツリー，外部 SDK，テスト資材，PowerShell スクリプトを
除外する設定を `_config.yml` に記載している．新しいディレクトリを追加したときは公開対象かを確認する．

## 公式資料

- [公開元ブランチの設定](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site)
- [GitHub Pages と Jekyll・標準プラグイン](https://docs.github.com/en/pages/setting-up-a-github-pages-site-with-jekyll/about-github-pages-and-jekyll)
