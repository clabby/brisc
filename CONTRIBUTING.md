# Contributing

Thank you for wanting to contribute! Before contributing to this repository,
please read through this document and discuss the change you wish to make via issue.

## Dependencies

Before working with this repository locally, you'll need to install the following dependencies.

- The [Rust toolchain][rust]
- [Docker][docker], for cross-compilation.

## Pull Request Process

1. Before anything, [create an issue][create-an-issue] to discuss the change you're
   wanting to make, if it is significant or changes functionality. Feel free to skip this step for trivial changes.
1. Once your change is implemented, ensure that all checks are passing before creating a PR. The full CI pipeline can
   be run locally via the `justfile`s in the repository.
1. Make sure to update any documentation that has gone stale as a result of the change, in the markdown files and in rustdoc comments.
1. Once you have sign-off from a maintainer, you may merge your pull request yourself if you have permissions to do so.
   If not, the maintainer who approves your pull request will add it to the merge queue.

<!-- Links -->

[rust]: https://rustup.rs/
[docker]: https://www.docker.com/get-started/

[create-an-issue]: https://github.com/clabby/brisc/issues/new
