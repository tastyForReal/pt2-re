curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain=stable --profile=minimal
. "$HOME/.cargo/env" && rustup component add rustfmt clippy
rm -rf "$HOME/.cargo/registry/*"
. "$HOME/.cargo/env" && cargo build