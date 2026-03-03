#!/bin/sh
prettier -c . '!**/volumes' '!**/dist' '!target' '!**/translations' '!api_tests/pnpm-lock.yaml'
