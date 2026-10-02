#!/bin/sh
# Tauri's generated RPM owns /usr/lib/LiveBlock but not every intermediate
# resource directory. Remove only empty package directories after RPM has
# removed their contents; never delete residual files from an operator.
rmdir \
  /usr/lib/LiveBlock/resources/onnxruntime \
  /usr/lib/LiveBlock/resources \
  /usr/lib/LiveBlock \
  2>/dev/null || :
