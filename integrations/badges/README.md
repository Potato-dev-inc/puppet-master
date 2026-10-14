# README badges

## Add to Cursor (works today)

```markdown
[![Add Puppet Master to Cursor](https://img.shields.io/badge/Cursor-Add_Puppet_Master_MCP-black?logo=cursor)](cursor://anysphere.cursor-deeplink/mcp/install?name=puppet-master&config=eyJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsIkBwdXBwZXQtbWFzdGVyL21jcCJdfQ%3D%3D)
```

GitHub strips custom URL schemes from Markdown links, so a repository README needs the HTTPS redirect page below. In docs, the link works as is. The `config` value is the base64 of `{"command":"npx","args":["-y","@puppet-master/mcp"]}`. Until a current `@puppet-master/mcp` is published (see `../README.md`, Known gaps), prefer the in-app installer.

## Run in Puppet Master (planned)

```markdown
[![Run in Puppet Master](https://img.shields.io/badge/Run_in-Puppet_Master-black)](https://github.com/Potato-dev-inc/puppet-master/releases)
```

This snippet links to the installers, which works today. The "open this repo in PM" behaviour needs two things that do not exist yet: the `puppetmaster://open?repo=<git url>&ref=<branch>` deep link in the app, and an HTTPS redirect page that opens it and falls back to the installer. When both ship, change only the link target to that page.

## Works with Puppet Master

```markdown
[![Works with Puppet Master](https://img.shields.io/badge/Works_with-Puppet_Master-black)](https://github.com/Potato-dev-inc/puppet-master)
```

Only for agents that ship a PM agent definition; the claim should be backed by a conformance run, not made by default.
