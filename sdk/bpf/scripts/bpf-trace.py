#!/usr/bin/env python

# 
# Prints a BPF program call trace with instruction counts for each call
#
# This script requires a dump file containing the instruction dump of the ELF
# and a trace file that contains the trace output of the BPF VM
#
# You can create the dump file with do.sh:
#  $ do.sh dump <project>
# Or directly:
#  $ llvm-objdump -print-imm-hex --source --disassemble <ELF file path>
#
# You can create the trace file by running the program and setting RUST_LOG:
# Set RUST_LOG
#  $ export RUST_LOG=solana_rbpf=trace
# Capture output via `> trace.txt 2>&1`
#

import re
import sys

# regular expressions to search for
rxs_symbols = {
    'start' : re.compile(r'Disassembly of section \.text.*\n'),
    'symbol': re.compile(r'^(?!LBB)([<\S].+)\n'),
}
rxs_ixs = {
    'ixs': re.compile(r' *(\d+) *.*\n'),
}
rxs_call = {
    'entry': re.compile(r'BPF: +0 .+pc *(\d+) *.*\n'),
    'call' : re.compile(r'BPF: +(\d+) .+pc *(\d+) *call.*\n'),
    'exit' : re.compile(r'BPF: +(\d+) .+exit\n'),
}
rxs_called = {
    'pc': re.compile(r'BPF: +\d+ .+pc +(\d+).*\n'),
}

# parse a line
def parse_line(rxs, line):
    for key, rx in rxs.items():
        match = rx.search(line)
        if match:
            return key, match
    return None, None

# main
if __name__ == '__main__':

    if len(sys.argv) < 3:
        print('Error: Must specify a dump and trace file')
        sys.exit('    Usage: ' + sys.argv[0] + ' dump_file trace_file')
    dumppath = sys.argv[1]
    tracepath = sys.argv[2]
    
    # parse the dump file to create a map
    # of instruction numbers to symbols
    symbols = {}
    with open(dumppath, 'r') as file_object:
        line = file_object.readline()
        while line:
            key, match = parse_line(rxs_symbols, line)
            if key == 'start':
                line = file_object.readline()
                while line:
                    key, match = parse_line(rxs_symbols, line)
                    if key == 'symbol':
                        symbol =  match.group(1)
                        line = file_object.readline()
                        key, match = parse_line(rxs_ixs, line)
                        if key == 'ixs':
                            ixs = int(match.group(1))
                            symbols[ixs] = symbol
                    line = file_object.readline()
            line = file_object.readline()
    if len(symbols) == 0:
        sys.exit("Error: No instruction dump in: " + dumppath)
    
    # parse the trace file to build a call list
    calls = [] # all the calls made
    with open(tracepath, 'r') as file_object:
        exits = [] # instruction counts at each call exit
        call_nums = [] # used to match up exits to calls
        frame = 1
        num_calls = 0
        line = file_object.readline()
        while line:
            key, match = parse_line(rxs_call, line)
            if key == 'call':
                ixs_count = int(match.group(1))
                line = file_object.readline()
                key, match = parse_line(rxs_called, line)
                if key == 'pc':
                    called_pc = int(match.group(1))
                    calls.append((ixs_count, frame, symbols[called_pc]))
                    call_nums.append(num_calls)
                    exits.append(0)
                    num_calls += 1
                    frame += 1
            else:
                if key == 'entry':
                        pc = int(match.group(1))
                        calls.append((0, 0, symbols[pc]))
                        call_nums.append(num_calls)
                        exits.append(0)
                        num_calls += 1
                elif key == 'exit':
                    ixs_count = int(match.group(1))
                    num = call_nums.pop()
                    exits[num] = ixs_count
                    frame -= 1
                line = file_object.readline()

    # print the call trace with instruction counts for each call
    for call, exit in zip(calls, exits):
        print("%7d: %s %s" % (exit - call[0], ' |  ' * call[1], call[2]))

