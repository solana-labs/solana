#!/usr/bin/env python
import sys, json

data=json.load(sys.stdin)
print[\
   ([result['series'][0]['columns'][1].encode(), result['series'][0]['values'][0][1]]) \
   for result in data['results']]
