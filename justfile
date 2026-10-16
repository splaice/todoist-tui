root := home_directory() / ".local"

build:
    cargo build --release

install:
    cargo install --path . --root "{{root}}" --force

uninstall:
    cargo uninstall --root "{{root}}" todoist-tui
