{
  lib,
  rustPlatform,
  git,
}:

rustPlatform.buildRustPackage {
  pname = "wsx";
  version = "0.1.0";

  # Everything except the trees the build never reads. This is an exclude
  # list rather than an include list on purpose: `src/` reaches outside
  # itself with `include_str!` (src/agent/skill.rs pulls in skills/), so an
  # include list breaks quietly the next time that happens.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.difference ../. (
      lib.fileset.unions [
        ../.github
        ../Formula
        ../demo
        ../docs
        ../examples
        ../flake.lock
        ../flake.nix
        ../harness
        ../nix
        ../sandbox
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
      "sessionx-0.1.0" = "sha256-+MeBis8cqHpNF/IjCY4gpeBCpADVqFJCplCwKdfnyJg=";
    };
  };

  # The suite shells out to `git` to build fixture repositories. It writes a
  # per-repo identity itself, so the binary and a writable HOME are enough.
  nativeCheckInputs = [ git ];

  preCheck = ''
    export HOME
    HOME=$(mktemp -d)
  '';

  # These two read the live process table through `ps` to check process
  # ancestry. A build sandbox does not give them one, so they cannot pass
  # here. CI runs the whole suite on real runners.
  checkFlags = [
    "--skip=commands::external::tests::spawned_command_does_not_descend_from_this_process"
    "--skip=desktop::menubar::jump::jump_tests::ancestor_pids_walks_ps"
  ];

  meta = {
    description = "Terminal UI for managing coding agent sessions in git worktrees";
    homepage = "https://github.com/bakedbean/workspacex";
    license = lib.licenses.mit;
    mainProgram = "wsx";
    platforms = lib.platforms.unix;
  };
}
