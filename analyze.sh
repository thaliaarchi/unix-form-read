#!/usr/bin/env bash
set -eEuo pipefail

rg --json -a 'towhom-(?:(?:1|11|26|27)a|2[0-7]|1[0-9]|[1-9])' distr/form.m |
jq -cs '
  [
    .[] |
    select(.type == "match").data |
    .absolute_offset as $offset |
    .submatches[] |
    [$offset + .start, .match.text]
  ] | sort' |
underscore print > manual.json
