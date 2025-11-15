#!/bin/sh -x
WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_DMABUF_RENDERER
exec unstoppableswap-gui-rs "$@"
