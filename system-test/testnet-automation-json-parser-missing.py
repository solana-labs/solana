#!/usr/bin/env python3
import sys, json

data=json.load(sys.stdin)

# this code is designed for influx queries where 'no data' means 0
if 'results' in data:
   for result in data['results']:
      val = "0"
      if 'series' in result:
         val = str(result['series'][0]['values'][0][1])
      print(val)
else:
   print("No results returned from CURL request")
