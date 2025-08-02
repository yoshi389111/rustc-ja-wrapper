# rustc-ja-wrapper

RUST のコンパイルエラーメッセージを日本語化するラッパーです。

とりあえずのお試し版です。

## 使い方

ラッパーをビルドして、適当なフォルダに配置します。

環境変数 `RUSTC_WRAPPER` にラッパーのパスを指定すると、`cargo` 経由でビルドするときに、直接 `rustc` を呼び出すのではなく、ラッパーを呼び出すようになります（詳細は The Cargo Book の [Environment Variables](https://doc.rust-lang.org/cargo/reference/environment-variables.html) を参照）。

あるいは、`PROJECT_ROOT/.cargo/config.toml` 等に以下のように記載することもできます（別のフォルダでも可能。詳細は The Cargo Book の [Configuration](https://doc.rust-lang.org/cargo/reference/config.html) を参照）。

```toml:PROJECT_ROOT/.cargo/config.toml
[build]
rustc-wrapper = "/path/to/rustc-ja-wrapper"
```

実行結果は以下のような感じ

```console
$ cargo build
   Compiling foo v0.1.0 (/project/foo)
warning: 変数が使われていません: `b`
 --> src/main.rs:4:9
  |
4 |     let b = a[10];
  |         ^ help: 意図的ならアンダースコアを前に付けて下さい: `_b`
  |
  = note: `#[warn(unused_variables)]`はデフォルトで有効です

error: この操作は実行時にパニックします
 --> src/main.rs:4:13
  |
4 |     let b = a[10];
  |             ^^^^^ 添え字が範囲外です: 長さは3、添え字は10
  |
  = note: `#[deny(unconditional_panic)]`はデフォルトで有効です

warning: `foo` (bin "foo") generated 1 warning
error: could not compile `foo` (bin "foo") due to 1 previous error; 1 warning emitted
```

## 注意点

- 翻訳している項目はごく一部です。
- 正しく動く保証はありません。
- ソースコード内にコンパイルエラーのメッセージと同じ文字列があると、そちらも置換されてしまいます。

## License

&copy; 2025 SATO Yoshiyuki. MIT Licensed.
