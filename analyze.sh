#!/usr/bin/env bash
set -eEuo pipefail

rg --json -a 'towhom-\d{1,2}' distr/form.m |
jq -cs '
  [
    .[] |
    select(.type == "match").data |
    .absolute_offset as $offset |
    .submatches[] |
    [$offset + .start, .match.text]
  ] | sort' |
underscore print > manual.json
