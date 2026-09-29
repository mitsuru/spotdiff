# README用の画像

`before.png`と`after.png`は比較用のサンプルです。円と長方形の色を変更し、黄色いバーを追加しています。

表示例は実装の`Renderer::draw`をratatuiの`TestBackend`で描画し、`rasterize_viewport`の画像出力を合成して生成します。文字と枠はSVGに書き出し、PNGとGIFに変換します。端末、Wayland、X11は不要です。これは表示内容の例であり、端末のKitty Protocol対応を検証するものではありません。

## 再生成

RustとCargoのほか、`rsvg-convert`（librsvg）、ImageMagickの`magick`、DejaVu Sans Monoフォントが必要です。リポジトリのルートで実行してください。

```sh
cargo run --locked --example readme_frames

rsvg-convert target/readme-frames/side-by-side.svg -o docs/images/side-by-side.png
rsvg-convert target/readme-frames/highlight.svg -o docs/images/highlight.png
rsvg-convert target/readme-frames/blink-before.svg -o target/readme-frames/blink-before.png
rsvg-convert target/readme-frames/blink-after.svg -o target/readme-frames/blink-after.png

magick -delay 50 target/readme-frames/blink-before.png target/readme-frames/blink-after.png \
  -loop 0 -layers Optimize docs/images/blink.gif
```

Blinkのフレームは`App`の自動再生を開始し、500ms後の状態へ進めて生成します。GIFの各フレームも500ms表示します。表示例の色・フォント・セル寸法は生成例の中で固定しています。
