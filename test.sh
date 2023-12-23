#!/usr/bin/env bash

VER=debug
#VER=release

target/${VER}/server&
sleep 1
target/${VER}/client&
target/${VER}/client&
target/${VER}/client&
target/${VER}/client&
target/${VER}/client&
