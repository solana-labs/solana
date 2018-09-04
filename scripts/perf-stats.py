#!/usr/bin/env python3

import json
import sys

stages_data = {}

if len(sys.argv) != 2:
    print("USAGE: {} <input file>".format(sys.argv[0]))
    sys.exit(1)

with open(sys.argv[1]) as fh:
    for line in fh.readlines():
        if "COUNTER" in line:
            json_part = line[line.find("{"):]
            x = json.loads(json_part)
            counter = x['name']
            if not (counter in stages_data):
                stages_data[counter] = {'first_ts': x['now'], 'last_ts': x['now'], 'last_count': 0,
                                        'data': [], 'max_speed': 0, 'min_speed': 9999999999.0,
                                        'count': 0,
                                        'max_speed_ts': 0, 'min_speed_ts': 0}
            stages_data[counter]['count'] += 1
            count_since_last = x['counts'] - stages_data[counter]['last_count']
            time_since_last = float(x['now'] - stages_data[counter]['last_ts'])
            if time_since_last > 1:
                speed = 1000.0 * (count_since_last / time_since_last)
                stages_data[counter]['data'].append(speed)
                if speed > stages_data[counter]['max_speed']:
                    stages_data[counter]['max_speed'] = speed
                    stages_data[counter]['max_speed_ts'] = x['now']
                if speed < stages_data[counter]['min_speed']:
                    stages_data[counter]['min_speed'] = speed
                    stages_data[counter]['min_speed_ts'] = x['now']
            stages_data[counter]['last_ts'] = x['now']
            stages_data[counter]['last_count'] = x['counts']

for stage in stages_data.keys():
    stages_data[stage]['data'].sort()
    #mean_index = stages_data[stage]['count'] / 2
    mean = 0
    average = 0
    eightieth = 0
    data_len = len(stages_data[stage]['data'])
    mean_index = int(data_len / 2)
    eightieth_index = int(data_len * 0.8)
    #print("mean idx: {} data.len: {}".format(mean_index, data_len))
    if data_len > 0:
        mean = stages_data[stage]['data'][mean_index]
        average = float(sum(stages_data[stage]['data'])) / data_len
        eightieth = stages_data[stage]['data'][eightieth_index]
    print("stage: {} max: {:,.2f} min: {:.2f} count: {} total: {} mean: {:,.2f} average: {:,.2f} 80%: {:,.2f}".format(stage,
                                                       stages_data[stage]['max_speed'],
                                                       stages_data[stage]['min_speed'],
                                                       stages_data[stage]['count'],
                                                       stages_data[stage]['last_count'],
                                                       mean, average, eightieth))
    num = 5
    idx = -1
    if data_len >= num:
        print("    top {}: ".format(num), end='')
        for x in range(0, num):
            print("{:,.2f}  ".format(stages_data[stage]['data'][idx]), end='')
            idx -= 1
            if stages_data[stage]['data'][idx] < average:
                break
        print("")
    print("    max_ts: {} min_ts: {}".format(stages_data[stage]['max_speed_ts'], stages_data[stage]['min_speed_ts']))
    print("\n")

