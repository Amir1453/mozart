build:
  cargo build

example EXAMPLE:
  cargo run --release --example {{EXAMPLE}}

expand EXAMPLE:
  cargo expand --example {{EXAMPLE}}
