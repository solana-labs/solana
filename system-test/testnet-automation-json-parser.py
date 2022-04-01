#!/usr/bin/env python3
import sys, json, argparse

parser = argparse.ArgumentParser()
parser.add_argument("--empty_error", action="store_true", help="If present, do not print error message")
args = parser.parse_args()

data=json.load(sys.stdin)

if 'results' in data:
   for result in data['results']:
      if 'series' in result:
         print(result['series'][0]['columns'][1] + ': ' + str(result['series'][0]['values'][0][1]))
      elif not args.empty_error:
         print("An expected result from CURL request is missing")
elif not args.empty_error:
   print("No results returned from CURL request")
