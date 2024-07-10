#!/bin/sh

set -e

glslc -fshader-stage=fragment -o ./shader.fragment.spv ./shader.fragment.glsl
glslc -fshader-stage=vertex -o ./shader.vertex.spv ./shader.vertex.glsl

