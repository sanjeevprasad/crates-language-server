# crates-language-server
Crates Language Server For Helix-Editor

# Steps to build and install
```sh
# build
cargo build --release
# move binary to any dir included in $PATH
sudo cp -v ./target/release/crate-lsp /usr/local/bin
# append content of language.toml to your own languages.toml
cat ./languages.toml >> ~/.config/helix/languages.toml
```

![Screenshot](screenshots/screenshot.png)

This was just a quick way to learn about LSP, and to try out Helix Editor, (which I am kind of liking.)
This is just an early version, and lots of features are missing like autocomplete, and semver compatible latest versioning.
I may implement them if I got free time. Right now I am happy with this this showing the latest version.
