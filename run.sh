#!/usr/bin/dash

RUST_LOG=squatter_swatter=info \
  nohup \
  target/release/squatter-swatter \
  config.json 146170 \
  >run.log &