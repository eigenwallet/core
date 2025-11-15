#!/bin/sh -x

# Work around https://github.com/eigenwallet/core/issues/665
WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_DMABUF_RENDERER

exec unstoppableswap-gui-rs "$@"
