#!/usr/bin/env python3
import sys, json

data=json.load(sys.stdin)

if 'results' in data:
   for result in data['results']:
      if 'series' in result:
         print(result['series'][0]['columns'][1] + ': ' + str(result['series'][0]['values'][0][1]))
      else:
         print("An expected result from CURL request is missing")
else:
   print("No results returned from CURL request")
