# spotdiff

[![CI](https://github.com/mitsuru/spotdiff/actions/workflows/ci.yml/badge.svg)](https://github.com/mitsuru/spotdiff/actions/workflows/ci.yml)

Rustで作るターミナル画像diffビューア。Kitty Graphics Protocolで画像を表示し、変更前後の左右比較、変更箇所の強調、1つの枠で交互に表示するBlinkモードを切り替えられます。

lazygitで画像を選択して`I`を押すと全画面ビューアを開きます。`q`で終了し、lazygitの復帰プロンプトでEnterを押すと戻ります。

![spotdiffの左右比較。左が変更前、右が変更後で、円と長方形の色の変更と黄色いバーの追加を確認できる](https://raw.githubusercontent.com/mitsuru/spotdiff/main/docs/images/side-by-side.png)

左が`Before`、右が`After`です。フッターには表示モード・倍率・変更ピクセル数・変更率を表示します。

## インストール

RustとCargo、Kitty Graphics Protocolに対応した端末（Kitty、Ghosttyなど）が必要です。Rust 1.96.0でビルドを検証しています。

```sh
cargo install --path crates/spotdiff --locked
```

インストール先の`~/.cargo/bin`をPATHに追加してください。インストールせず試す場合は`cargo run --release -- before.png after.png`で起動できます。画像処理にはreleaseビルドを使ってください。

## 使い方

```sh
# 2つの画像ファイル
spotdiff before.png after.png

# 未ステージの変更: index → 作業ツリー
spotdiff git -- assets/image.png

# ステージ済みの変更: HEAD → index
spotdiff git --staged -- assets/image.png
```

PNG・JPEG・静止WebPを読み込めます。透過部分は市松模様で表示します。Gitモードでは新規追加、未追跡、`git add -N`、削除、初回コミット前も扱います。ファイル名は呼び出したディレクトリから指定できます。

変更がない場合もビューアを開きます。終了コードは正常終了で0、読み込みや端末のエラーで非ゼロです。Gitのファイル・index・設定は変更しません。

### サンプルで試す

リポジトリのルートで次のコマンドを実行すると、画面例と同じ画像を比較できます。

```sh
cargo run --release --locked -- docs/images/before.png docs/images/after.png
```

起動時は冒頭の左右比較です。`Tab`を押すたびに、左右比較 → 差分強調 → Blink → 左右比較の順に切り替わります。

**差分強調（`Tab`を1回）**：変更部分をマゼンタで強調し、変更のない部分を暗く表示します。円と長方形の色の変更に加え、追加したバーの位置も確認できます。

![差分強調モード。色を変更した円と長方形、追加したバーがマゼンタで強調されている](https://raw.githubusercontent.com/mitsuru/spotdiff/main/docs/images/highlight.png)

**Blink（`Tab`を2回）**：1つの枠で変更前後を切り替え、同じ位置を見ながら比較できます。`Space`で手動切り替え、`a`でGIFのように500ms間隔の自動切り替えを開始します。もう一度`a`を押すと停止します。

![Blinkモード。BeforeとAfterを同じ枠で交互に表示し、色の変更とバーの追加が分かる](https://raw.githubusercontent.com/mitsuru/spotdiff/main/docs/images/blink.gif)

### キー操作

| キー | 操作 |
| --- | --- |
| `Tab` | 左右比較 → 差分強調 → Blink |
| `Space` | Blinkで変更前後を手動切り替え（自動再生中は停止） |
| `a` | Blinkの自動切り替えを開始 / 停止（500ms間隔） |
| `+` / `-` | ズームイン / ズームアウト |
| 矢印 / `h j k l` | 表示位置を移動 |
| `f` | 画面にフィット |
| `1` | 1倍（画像の1pxを端末の1pxに合わせる） |
| `q` / `Esc` / `Ctrl-C` | 終了 |

左右は同じ画像座標・倍率・位置で表示します。リサイズ時もフィット中は倍率を再計算し、手動ズーム中は倍率を維持します。ピクセルの境界を確認しやすいよう、表示のリサイズにはnearest neighborを使います。

Blinkに入ると、1つの枠に`Before`を表示して手動モードで開始します。枠のタイトルで現在の画像、フッターで`Manual` / `Auto`を確認できます。変更前後で倍率・位置を共有し、ズームや移動も使えます。両画像がある場合はキャッシュを保持し、切り替えだけでは画像データを再転送しません。追加・削除で画像がない側は説明を表示します。`herdr --remote`では画像なし側の表示後、存在する側に戻ると中継側で画像を再転送する場合があります。

画像の比較キャンバス内だけを画像として転送し、その外側は端末の背景を表示します。画像内の透過部分とサイズ違いで片側に画像がない部分には、市松模様を表示します。

## lazygitとの連携

[examples/lazygit.yml](https://github.com/mitsuru/spotdiff/blob/main/examples/lazygit.yml)を既存のlazygit設定の`customCommands`へマージしてください。設定ファイルはlazygitのStatusパネルで`e`を押して開けます。すでに`customCommands`がある場合はキーを重複させず、配列の項目を追加してください。

- `I`：未ステージ変更があればindexと作業ツリーを比較し、それ以外はHEADとindexを比較。
- `Ctrl-S`：ステージ済みの変更を明示的に比較。
- 両方に変更があるファイルでは、`I`は未ステージを優先します。

既存キーバインドと重複する場合は設定例の`key`を変更してください。画像以外のファイルでは説明付きエラーを返します。`output: terminal`でlazygitを一時停止して起動します。lazygit 0.65.0では終了後に`Press enter to return to lazygit`が出るのでEnterを押してください。

lazygitのdiffパネル内表示は将来の拡張です。初期版の連携は全画面ビューアで行います。

## 差分の定義と制限

- デコードした8ビットRGBAを、拡大縮小する前に比較します。RGBA値が1つでも異なれば変更です。ただし両側ともalpha=0ならRGBの違いを無視します。
- サイズ違いは左上を揃えて比較します。片側だけに存在するピクセルも変更です。変更率の分母は少なくとも片側にピクセルが存在する領域です。
- 差分強調では変更箇所をマゼンタで表示し、変更のない領域を暗くします。削除領域には変更前の画像を使います。
- 各入力のデコード後のRGBAと比較キャンバスは、それぞれ256 MiB以下です。合計メモリ使用量はこの値より大きくなります。
- 表示画像のメモリ使用量を抑えるため、各画像の表示領域は最大297列×297行です。大きな端末でも、この領域にフィットして表示し、ズーム時は画像全体を移動して確認できます。
- FIFOやデバイスなど通常ファイル以外の入力は拒否します。
- SVG、アニメーション、知覚的な差分、ICCによる色管理、EXIFによる自動回転、Git LFSの展開、リネーム追跡、merge conflict、シンボリックリンクには対応していません。
- tmux/screen内では起動を拒否します。対応端末で直接実行してください。非対応端末では文字表示へのフォールバックを行いません。
- 対話用TTYが必要です。画像制御列をファイルへリダイレクトして利用する形式ではありません。

## 開発と検証

ルートの`Cargo.toml`はvirtual workspaceです。`crates/spotdiff`にバイナリ・ライブラリ・Rustテスト・Rust exampleをまとめ、依存関係とパッケージ情報はルートで管理します。`Cargo.lock`と`target/`はworkspace全体で共有し、PythonのPTYテストとlazygit設定例はルートに置いています。Cargoコマンドはリポジトリのルートから実行してください。

GitHub Actionsはmainへのpushとmainを対象とするPRで、Ubuntu 24.04・Rust 1.96.0を使ってworkspace全体のfmt、Rustテスト、Clippy、releaseビルド、端末のPTYテストを実行します。配布用crateの作成・ビルド検証と、lazygit 0.65.0での起動・復帰の連携テストも実行します。

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo build --workspace --locked --release --bins --examples
cargo package -p spotdiff --locked
python3 tests/terminal_pty.py target/release/spotdiff target/release/examples/terminal-failure
python3 tests/lazygit_pty.py target/release/spotdiff
```

PTYテストは端末応答を模擬し、端末モード・カーソル・画面の復元を検証します。lazygitのPTYテストはインストール済みのlazygitと一時設定を使用します。画像の見え方と実画面での復帰は[実機検証手順](https://github.com/mitsuru/spotdiff/blob/main/docs/manual-testing.md)で確認してください。

## 設計

[設計仕様](https://github.com/mitsuru/spotdiff/blob/main/docs/superpowers/specs/2026-09-28-spotdiff-design.md)と[実装計画](https://github.com/mitsuru/spotdiff/blob/main/docs/superpowers/plans/2026-09-28-spotdiff.md)を参照してください。画像取得・差分生成はターミナル描画から分離しています。

[実装・検証記録](https://github.com/mitsuru/spotdiff/blob/main/docs/implementation-notes.md)に独立レビューの修正と実装中の判断をまとめています。

Ghosttyでの更新遅延に対する画像転送の改善と計測条件は[性能確認](https://github.com/mitsuru/spotdiff/blob/main/docs/performance.md)を参照してください。
