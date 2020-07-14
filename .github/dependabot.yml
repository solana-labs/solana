# To get started with Dependabot version updates, you'll need to specify which
# package ecosystems to update and where the package manifests are located.
# Please see the documentation for all configuration options:
# https://help.github.com/github/administering-a-repository/configuration-options-for-dependency-updates

version: 2
updates:
- package-ecosystem: cargo
  directory: "/"
  schedule:
    interval: daily
    time: "01:00"
    timezone: America/Los_Angeles
  #labels:
  #  - "automerge"
  open-pull-requests-limit: 3
  
- package-ecosystem: npm
  directory: "/web3.js"
  schedule:
    interval: daily
    time: "01:00"
    timezone: America/Los_Angeles
  labels:
    - "automerge"
  commit-message:
    prefix: "chore:"
  open-pull-requests-limit: 3
  
- package-ecosystem: npm
  directory: "/explorer"
  schedule:
    interval: daily
    time: "01:00"
    timezone: America/Los_Angeles
  labels:
    - "automerge"
  commit-message:
    prefix: "chore:"
    include: "scope"
  open-pull-requests-limit: 3
