#!/usr/bin/env python3
#
# Adjusts the testnet monitor dashboard for the specified release channel
#

import sys
import json

if len(sys.argv) != 4:
    print('Error: Dashboard or Channel not specified')
    sys.exit(1)

dashboard_json = sys.argv[1]
channel = sys.argv[2]
output_dashboard_json = sys.argv[3]

if channel not in ['edge', 'beta', 'stable', 'local']:
    print('Error: Unknown channel:', channel)
    sys.exit(2)

with open(dashboard_json, 'r') as read_file:
    data = json.load(read_file)

if channel == 'local':
    data['title'] = 'Local Testnet Monitor'
    data['uid'] = 'local'
    data['links'] = []
    data['templating']['list'] = [{'current': {'text': '$datasource',
                                               'value': '$datasource'},
                                   'hide': 1,
                                   'label': 'Data Source',
                                   'name': 'datasource',
                                   'options': [],
                                   'query': 'influxdb',
                                   'refresh': 1,
                                   'regex': '',
                                   'type': 'datasource'},
                                  {'allValue': None,
                                   'current': {'text': 'testnet',
                                               'value': 'testnet'},
                                   'hide': 1,
                                   'includeAll': False,
                                   'label': 'Testnet',
                                   'multi': False,
                                   'name': 'testnet',
                                   'options': [{'selected': True,
                                                'text': 'testnet',
                                                'value': 'testnet'}],
                                   'query': 'testnet',
                                   'type': 'custom'}]

elif channel == 'stable':
    # Stable dashboard only allows the user to select between the stable
    # testnet databases
    data['title'] = 'Testnet Monitor'
    data['uid'] = 'testnet'
    data['templating']['list'] = [{'current': {'text': '$datasource',
                                               'value': '$datasource'},
                                   'hide': 1,
                                   'label': 'Data Source',
                                   'name': 'datasource',
                                   'options': [],
                                   'query': 'influxdb',
                                   'refresh': 1,
                                   'regex': '',
                                   'type': 'datasource'},
                                  {'allValue': None,
                                   'current': {'text': 'testnet',
                                               'value': 'testnet'},
                                   'hide': 1,
                                   'includeAll': False,
                                   'label': 'Testnet',
                                   'multi': False,
                                   'name': 'testnet',
                                   'options': [{'selected': False,
                                                'text': 'testnet',
                                                'value': 'testnet'},
                                               {'selected': True,
                                                'text': 'testnet-perf',
                                                'value': 'testnet-perf'}],
                                   'query': 'testnet,testnet-perf',
                                   'type': 'custom'}]
else:
    # Non-stable dashboard only allows the user to select between all testnet
    # databases
    data['title'] = 'Testnet Monitor ({})'.format(channel)
    data['uid'] = 'testnet-' + channel
    data['templating']['list'] = [{'current': {'text': '$datasource',
                                               'value': '$datasource'},
                                   'hide': 1,
                                   'label': 'Data Source',
                                   'name': 'datasource',
                                   'options': [],
                                   'query': 'influxdb',
                                   'refresh': 1,
                                   'regex': '',
                                   'type': 'datasource'},
                                   {'allValue': ".*",
                                   'current': {'text': 'testnet',
                                               'value': 'testnet'},
                                   'datasource': '$datasource',
                                   'hide': 1,
                                   'includeAll': False,
                                   'label': 'Testnet',
                                   'multi': False,
                                   'name': 'testnet',
                                   'options': [],
                                   'query': 'show databases',
                                   'refresh': 1,
                                   'regex': 'testnet.*',
                                   'sort': 1,
                                   'tagValuesQuery': '',
                                   'tags': [],
                                   'tagsQuery': '',
                                   'type': 'query',
                                   'useTags': False},
                                   {'allValue': ".*",
                                    'datasource': '$datasource',
                                    'hide': 0,
                                    'includeAll': True,
                                    'label': 'HostID',
                                    'multi': False,
                                    'name': 'hostid',
                                    'options': [],
                                    'query': 'SELECT DISTINCT(\"host_id\") FROM \"$testnet\".\"autogen\".\"counter-fullnode-new\" ',
                                    'refresh': 2,
                                    'regex': '',
                                    'sort': 1,
                                    'tagValuesQuery': '',
                                    'tags': [],
                                    'tagsQuery': '',
                                    'type': 'query',
                                    'useTags': False}]

with open(output_dashboard_json, 'w') as write_file:
    json.dump(data, write_file, indent=2)
