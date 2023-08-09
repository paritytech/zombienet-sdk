# 🚧⚠️ [WIP] ZombieNet SDK  ⚠️🚧


[Rust Docs](https://paritytech.github.io/zombienet-sdk)

# The Vision

This issue will track the progress of the new ZombieNet SDK.

We want to create a new SDK for `ZombieNet` that allow users to build more complex use cases and interact with the network in a more flexible and programatic way.
The SDK will provide a set of `building blocks` that users can combine in order to spawn and interact (test/query/etc) with the network providing a *fluent* api to craft different topologies and assertions to the running network. The new `SDK` will support the same range of `providers` and configurations that can be created in the current version (v1).

We also want to continue supporting the `CLI` interface *but* should be updated to use the `SDK` under the hood.

# The Plan

We plan to divide the work phases to. ensure we cover all the requirement and inside each phase in small tasks, covering one of the building blocks and the interaction between them. 

## Prototype building blocks

Prototype each building block with a clear interface and how to interact with it
- [Building block Network #2](https://github.com/paritytech/zombienet-sdk/issues/2)
- [Building block Node #3](https://github.com/paritytech/zombienet-sdk/issues/3)
- [Building block NodeGroup #4](https://github.com/paritytech/zombienet-sdk/issues/4)
- [Building block Parachain #5](https://github.com/paritytech/zombienet-sdk/issues/5)
- [Building block Collator #6](https://github.com/paritytech/zombienet-sdk/issues/6)
- [Building block CollatorGroup #7](https://github.com/paritytech/zombienet-sdk/issues/7)
- [Building block Assertion #8](https://github.com/paritytech/zombienet-sdk/issues/8)

## Integrate, test interactions and document

We want to integrate the interactions for all building blocks and document the way that they work together.

- [Spawning Integration #9](https://github.com/paritytech/zombienet-sdk/issues/9)
- [Assertion Integration #10](https://github.com/paritytech/zombienet-sdk/issues/10)
- [Documentation #11](https://github.com/paritytech/zombienet-sdk/issues/11)

## Refactor `CLI` and ensure backwards compatibility

Refactor the `CLI` module to use the new `SDK` under the hood.

- [Refactor CLI #12](https://github.com/paritytech/zombienet-sdk/issues/12)
- [Ensure that spawning from toml works #13](https://github.com/paritytech/zombienet-sdk/issues/13)
- [Ensure that test-runner from DSL works #14](https://github.com/paritytech/zombienet-sdk/issues/14)

## ROADMAP

## Infra
- Chaos testing, add examples and explore possibilities in `native` and `podman` provider
- Add `docker` provider
- Add `nomad` provider
- Create [helm chart](https://helm.sh/docs/topics/charts/) to allow other use zombienet in k8s
- Auth system to not use k8s users
- Create GitHub Action and publish in NPM marketplace (Completed)
- Rename `@paritytech/zombienet` npm package to `zombienet`. Keep all zombienet modules under `@zombienet/*` org (Completed)

## Internal teams
- Add more teams (wip)

## Registry
- Create decorators registry and allow override by paras (wip)
- Explore how to get info from paras.

## Functional tasks
- Add subxt integration, allow to compile/run on the fly
- Move parser to pest (wip)
- Detach phases and use JSON to communicate instead of `paths`
- Add relative values assertions (for metrics/scripts)
- Allow to define nodes that are not started in the launching phase and can be started by the test-runner
- Allow to define `race` assertions
- Rust integration -> Create multiples libs (crates)
- Explore backchannel use case
- Add support to run test agains a running network (wip)
- Add more CLI subcommands
- Add js/subxt snippets ready to use in assertions (e.g transfers)
- Add XCM support in built-in assertions
- Add `ink! smart contract` support
- Add support to start from a live network (fork-off) [check subalfred]
- Create "default configuration" - (if `zombieconfig.json` exists in same dir with zombienet then the config applied in it will override the default configuration of zombienet.  E.G if user wants to have as default `native` instead of `k8s` he can add  to 

## UI
- Create UI to create `.zndls` and `network` files.
- Improve VSCode extension (grammar/snippets/syntax highlighting/file validations) ([repo](https://github.com/paritytech/zombienet-vscode-extension))
- Create UI app (desktop) to run zombienet without the need of terminal.
