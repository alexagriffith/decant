# decant developer tasks

build:
    cargo build --release

test:
    cargo test

# Sync your real sessions into the default DB
sync:
    cargo run -p decant-cli --release -- sync

# List recent sessions
ls *ARGS:
    cargo run -p decant-cli --release -- session ls {{ARGS}}
