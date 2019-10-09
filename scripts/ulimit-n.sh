# |source| this file
#
# Adjust the maximum number of files that may be opened to as large as possible.
#

maxOpenFds=65000

if [[ $(ulimit -n) -lt $maxOpenFds ]]; then
  ulimit -n $maxOpenFds 2>/dev/null || {
    echo "Error: nofiles too small: $(ulimit -n). Failed to run \"ulimit -n $maxOpenFds\"";
    if [[ $(uname) = Darwin ]]; then
      echo "Try running |sudo launchctl limit maxfiles 65536 200000| first"
    fi
  }
fi
