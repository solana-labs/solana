#!/usr/bin/env python3

import matplotlib
matplotlib.use('Agg')

import matplotlib.pyplot as plt
import json
import sys

stages_to_counters = {}
stages_to_time = {}

if len(sys.argv) != 2:
    print(f"USAGE: {sys.argv[0]} <input file>")
    sys.exit(1)

with open(sys.argv[1]) as fh:
    for line in fh.readlines():
        if "COUNTER" in line:
            json_part = line[line.find("{"):]
            x = json.loads(json_part)
            counter = x['name']
            if not (counter in stages_to_counters):
                stages_to_counters[counter] = []
                stages_to_time[counter] = []
            stages_to_counters[counter].append(x['counts'])
            stages_to_time[counter].append(x['now'])

fig, ax = plt.subplots()

for stage in stages_to_counters.keys():
    plt.plot(stages_to_time[stage], stages_to_counters[stage], label=stage)

plt.xlabel('ms')
plt.ylabel('count')

plt.legend(bbox_to_anchor=(0., 1.02, 1., .102), loc=3,
           ncol=2, mode="expand", borderaxespad=0.)

plt.locator_params(axis='x', nbins=10)
plt.grid(True)

plt.savefig("perf.pdf")
