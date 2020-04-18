#!/usr/bin/env python3
#
# Adjusts the testnet monitor dashboard for the specified release channel
#

import sys
import json

if len(sys.argv) != 3:
    print('Error: Dashboard or Channel not specified')
    sys.exit(1)

dashboard_json = sys.argv[1]
channel = sys.argv[2]

if channel not in ['edge', 'beta', 'stable', 'local']:
    print('Error: Unknown channel:', channel)
    sys.exit(2)

with open(dashboard_json, 'r') as read_file:
    data = json.load(read_file)

if channel == 'local':
    data['title'] = 'Local Cluster Monitor'
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
                                   'type': 'custom'},
                                   {'allValue': ".*",
                                    'datasource': '$datasource',
                                    'hide': 0,
                                    'includeAll': True,
                                    'label': 'HostID',
                                    'multi': False,
                                    'name': 'hostid',
                                    'options': [],
                                    'query': 'SELECT DISTINCT(\"id\") FROM \"$testnet\".\"autogen\".\"validator-new\" ',
                                    'refresh': 2,
                                    'regex': '',
                                    'sort': 1,
                                    'tagValuesQuery': '',
                                    'tags': [],
                                    'tagsQuery': '',
                                    'type': 'query',
                                    'useTags': False}]

elif channel == 'stable':
    # Stable dashboard only allows the user to select between public clusters
    data['title'] = 'Cluster Telemetry'
    data['uid'] = 'monitor'
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
                                   'current': {'text': 'Mainnet Beta',
                                               'value': 'mainnet-beta'},
                                   'hide': 1,
                                   'includeAll': False,
                                   'label': 'Testnet',
                                   'multi': False,
                                   'name': 'testnet',
                                   'options': [{'selected': True,
                                                'text': 'Devnet',
                                                'value': 'devnet'},
                                               {'selected': False,
                                                'text': 'Mainnet Beta',
                                                'value': 'mainnet-beta'},
                                               {'selected': False,
                                                'text': 'Testnet',
                                                'value': 'tds'}],
                                   'query': 'devnet,mainnet-beta,tds',
                                   'type': 'custom'},
                                   {'allValue': ".*",
                                    'datasource': '$datasource',
                                    'hide': 0,
                                    'includeAll': True,
                                    'label': 'HostID',
                                    'multi': False,
                                    'name': 'hostid',
                                    'options': [],
                                    'query': 'SELECT DISTINCT(\"id\") FROM \"$testnet\".\"autogen\".\"validator-new\" ',
                                    'refresh': 2,
                                    'regex': '',
                                    'sort': 1,
                                    'tagValuesQuery': '',
                                    'tags': [],
                                    'tagsQuery': '',
                                    'type': 'query',
                                    'useTags': False}]
else:
    # Non-stable dashboard includes all the dev clusters
    data['title'] = 'Cluster Telemetry ({})'.format(channel)
    data['uid'] = 'monitor-' + channel
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
                                   'current': {'text': 'Developer Testnet',
                                               'value': 'devnet'},
                                   'datasource': '$datasource',
                                   'hide': 1,
                                   'includeAll': False,
                                   'label': 'Testnet',
                                   'multi': False,
                                   'name': 'testnet',
                                   'options': [],
                                   'query': 'show databases',
                                   'refresh': 1,
                                   'regex': '(devnet|tds|mainnet-beta|testnet.*)',
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
                                    'query': 'SELECT DISTINCT(\"id\") FROM \"$testnet\".\"autogen\".\"validator-new\" ',
                                    'refresh': 2,
                                    'regex': '',
                                    'sort': 1,
                                    'tagValuesQuery': '',
                                    'tags': [],
                                    'tagsQuery': '',
                                    'type': 'query',
                                    'useTags': False}]

with open(dashboard_json, 'w') as write_file:
    json.dump(data, write_file, indent=2)
