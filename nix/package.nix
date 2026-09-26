{
  lib,
  rustPlatform,
  git,
}:

rustPlatform.buildRustPackage {
  pname = "wsx";
  version = "0.1.1";

  # Everything except the trees nothing in the build reads. Keep this an
  # exclude list, not an include list: the crate and its tests reach well
  # outside `src/`, and an include list breaks quietly when that set grows.
  # At the time of writing the build reads `skills/` (include_str!),
  # `tests/fixtures/`, `examples/` (a cargo example target), and, from the
  # test suite, `docs/examples/` and `docs/book/src/configuration/themes.md`.
  # Only `site/` is large, and it holds media that no test opens.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.difference ../. (
      lib.fileset.unions [
        ../.github
        ../Formula
        ../demo
        ../flake.lock
        ../flake.nix
        ../nix
        ../scripts
        ../site
      ]
    );
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    # sessionx is a git dependency, so Cargo.lock carries no registry
    # checksum for it and Nix needs the hash stated here. Refresh it with:
    #   nix-prefetch-git --url https://github.com/bakedbean/sessionx --rev <rev>
    outputHashes = {
      "sessionx-0.1.0" = "sha256-BVbYPIYWLdmS08RP91ZySWA0jGY2JVCB8fLyD9zxzMU=";
    };
  };

  # The suite shells out to `git` to build fixture repositories. It writes a
  # per-repo identity itself, so the binary and a writable HOME are enough.
  nativeCheckInputs = [ git ];

  preCheck = ''
    export HOME
    HOME=$(mktemp -d)
  '';

  # Tests that cannot run reliably in a build sandbox. Everything else runs,
  # and CI covers all three on real runners, where the whole suite passes.
  #
  # The first two read the live process table through `ps` to check process
  # ancestry, and a sandbox does not give them one. The third drives a PTY
  # and asserts on how much output came back; it races under the sandbox,
  # passing on one build and failing on the next.
  checkFlags = [
    "--skip=commands::external::tests::spawned_command_does_not_descend_from_this_process"
    "--skip=desktop::menubar::jump::jump_tests::ancestor_pids_walks_ps"
    "--skip=app::input::tests::leader::dashboard_no_submit_chip_stages_reply_draft_then_enter_sends_once"
  ];

  meta = {
    description = "Terminal UI for managing coding agent sessions in git worktrees";
    homepage = "https://github.com/bakedbean/workspacex";
    license = lib.licenses.mit;
    mainProgram = "wsx";
    platforms = lib.platforms.unix;
  };
}
