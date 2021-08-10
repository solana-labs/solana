#!/usr/bin/env python3
#from dateutil import parser
from datetime import datetime
from datetime import timedelta

import json
import sys
import re

def get_time(line):
    split = line.split(" ")
    time_str = split[0].strip("[")
    #parser.parse(time_str)
    # datetime doesn't support ns, so remove those
    time_str = time_str[:-4]
    return datetime.strptime(time_str, "%Y-%m-%dT%H:%M:%S.%f")


secs = float(sys.argv[2])

input_file = sys.argv[1]

last_time = None
stats = {}
count = 0
total_count = 0
num_groups = 0
total_stats = {}
max_count = 0
min_count = 1000000000

with open(input_file) as fh:
    for line in fh.readlines():
        try:
            time = get_time(line)
        except:
            continue
        if not last_time:
            last_time = time
        datapoint_str = "datapoint:"
        timing_str = "ledger processing timing: Ex"
        datapoint = line.find(datapoint_str)
        if datapoint == -1:
            datapoint = line.find(timing_str)
            line = line.replace("details: ExecuteDetailsTimings ","").replace("{ ","").replace("} ","").replace(": ","=").replace(",","")
            split = line[datapoint + len(timing_str) + 1:].split(" ")
            split = split[1:]
        else:
            if line.find("process_blockstore_from_root") == -1 and line.find("datapoint: accounts_index") == -1 and line.find("accounts_db_store_timings") == -1:
                continue
            split = line[datapoint + len(datapoint_str) + 1:].split(" ")
            split = split[1:]
        #print(split)
        for s in split:
            x = s.split("=")
            if len(x) < 2:
                continue
            if x[0] == "per_program_timings":
                print("exit")
                break
            #print(x)
            name = x[0]
            try:
                value = int(x[1].strip("\ni"))
            except:
                break
            if name in stats:
                stats[name] += value
            else:
                stats[name] = value
        #print(split)
        count += 1

        if time > last_time + timedelta(seconds = secs):
            print("{} {} ".format(count, last_time), end="")
            for (name, total) in stats.items():
                if name != "fetch_entries_fail_time":
                    print("{}: {:.1f} ".format(name, total))
            print("")

            if False:
                avg_replay_total = stats['replay_total_elapsed'] / (1000 * count)
                print("avg replay_total_elapsed: {:.2f}".format(avg_replay_total))

                avg_replay_time = stats['replay_time'] / (1000 * count)
                print("avg replay_time: {:.2f}".format(avg_replay_time))

            print("")
            stats = {}
            if count > max_count:
                max_count = count
            if count < min_count:
                min_count = count
            total_count += count
            num_groups += 1
            count = 0
            last_time = time

    if count > 0:
        print("{} {} ".format(count, last_time), end="")
        for (name, total) in stats.items():
            if name != "fetch_entries_fail_time":
                print("{}: {:.1f} ".format(name, total))
        print("")

if num_groups == 0:
    num_groups = 1
print("{} average slots: {:.2f} min: {} max: {}".format(num_groups, total_count / num_groups, min_count, max_count))
