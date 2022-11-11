---
draft: false 
date: 2022-11-11
authors:
  - domenkozar
---

# Hello world: devenv 0.1

After lengthy conversations at [NixCon 2022](https://2022.nixcon.org/)
about Developer Experience and current painpoints around documentation, I've started 
[hacking and experimenting](https://github.com/cachix/devenv/commit/17512cf32528039090563438f7c103350810c2ce).

The goal is to bring the strengths of
Nix to the world with what we have best to offer, and I'm happy to announce:

[devenv: Fast, Declarative, Reproducible, and Composable Developer Environments](https://devenv.sh)


## Local containerless environments

One of the reasons why developer environments are moving into
the cloud are the lack of good tooling how to make those environments reproducible.

In the last decade we've doubled down on shipping binary blobs in containerized
environments.

Just as we went from virtual machines to containers, we can make one step further
and create guarantees at the package level and treat those as a building block.

``devenv`` 0.1 release brings the basic building blocks for many
possibilities of what can be built in the future.

I invite you to [explore the documentation](https://devenv.sh/getting-started/) and give it a try.

## Summary

I'm looking forward in what ways the developer community
uses devenv and **stay tuned for roadmap updates by subscribing
at our newsletter** at the bottom of the page.

Domen