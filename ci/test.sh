#!/bin/bash

# Get the directory of the current script
script_dir_by_bash_source=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

script_dir_by_0=$(cd "$(dirname "$0")" && pwd)

echo "script_dir_by_bash_source = $script_dir_by_bash_source"
echo "script_dir_by_0 = $script_dir_by_0"
