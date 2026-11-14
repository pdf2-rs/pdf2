#!/bin/sh
# Point git at the committed hooks. Run once per clone.
set -e
git config core.hooksPath .githooks
echo "hooks installed: $(git config core.hooksPath)"
