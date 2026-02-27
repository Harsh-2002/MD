#!/bin/bash
set -e
cargo build --release --features serve
cp target/release/mdx ./mdx
echo "Done: ./mdx"
